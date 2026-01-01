// SPDX-License-Identifier: Apache-2.0
// Copyright 2025, Acropolis team.

//! Delegation state (d_state) parsing from Cardano snapshots.
//!
//! This module handles the `d_state` structure from the CertState,
//! which contains stake delegation information including:
//! - Unified map (umap) with stake credentials, rewards, deposits, and delegations
//! - Genesis delegations
//! - Instantaneous rewards (MIRs)
//!
//! CDDL specification:
//! ```cddl
//! d_state = [
//!   ds_unified : umap,
//!   ds_future_gen_delegs : { * future_gen_deleg => gen_deleg_pair },
//!   ds_gen_delegs : gen_delegs,
//!   ds_i_rewards : instantaneous_rewards
//! ]
//!
//! umap = [
//!   um_elems : {* credential => um_elem },
//!   um_pointers : {* pointer => credential }
//! ]
//!
//! um_elem = [
//!   um_e_reward_deposit : strict_maybe<rdpair>,
//!   um_e_pointer_set : set<pointer>,
//!   um_e_s_pool : strict_maybe<keyhash_stakepool>,
//!   um_e_drep : strict_maybe<drep>,
//! ]
//!
//! rdpair = [
//!   rdpair_reward : compactform_coin,
//!   rdpair_deposit : compactform_coin,
//! ]
//!
//! pointer = [slot_no, tx_ix, cert_ix]
//!
//! instantaneous_rewards = [
//!   ir_reserves : { * credential_staking => coin },
//!   ir_treasury : { * credential_staking => coin },
//!   ir_delta_reserves : delta_coin,
//!   ir_delta_treasury : delta_coin,
//! ]
//! ```

use anyhow::{anyhow, Context, Result};
use minicbor::Decoder;
use std::collections::{BTreeMap, HashMap};
use tracing::info;

use crate::stake_addresses::{AccountState, StakeAddressState};
use crate::{DRepChoice, Lovelace, NetworkId, PoolId, StakeAddress, StakeCredential};

use super::streaming_snapshot::{DRep, SnapshotSet, StrictMaybe};

// =============================================================================
// Types
// =============================================================================

/// Reward and deposit pair from the unified map.
///
/// CDDL: `rdpair = [rdpair_reward, rdpair_deposit]`
pub type RDPair = (Lovelace, Lovelace);

/// Pointer to a certificate in the chain.
///
/// CDDL: `pointer = [slot_no, tx_ix, cert_ix]`
pub type Pointer = (u64, u64, u64);

/// Element in the unified map (um_elem).
///
/// CDDL:
/// ```cddl
/// um_elem = [
///   um_e_reward_deposit : strict_maybe<rdpair>,
///   um_e_pointer_set : set<pointer>,
///   um_e_s_pool : strict_maybe<keyhash_stakepool>,
///   um_e_drep : strict_maybe<drep>,
/// ]
/// ```
#[derive(Debug)]
pub struct UMapElem {
    /// Reward and deposit amounts (if registered)
    pub reward_deposit: StrictMaybe<RDPair>,
    /// Set of pointers to certificates
    pub pointers: SnapshotSet<Pointer>,
    /// Delegated stake pool (if any)
    pub pool: StrictMaybe<PoolId>,
    /// Delegated DRep (if any)
    pub drep: StrictMaybe<DRep>,
}

impl<'b, C> minicbor::Decode<'b, C> for UMapElem {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        Ok(UMapElem {
            reward_deposit: d.decode_with(ctx)?,
            pointers: d.decode_with(ctx)?,
            pool: d.decode_with(ctx)?,
            drep: d.decode_with(ctx)?,
        })
    }
}

/// Unified map (umap) containing stake credential mappings.
///
/// CDDL:
/// ```cddl
/// umap = [
///   um_elems : {* credential => um_elem },
///   um_pointers : {* pointer => credential }
/// ]
/// ```
#[derive(Debug)]
pub struct UMap {
    /// Map of stake credentials to their unified map elements
    pub elems: BTreeMap<StakeCredential, UMapElem>,
    /// Reverse pointer map: pointer -> credential
    pub pointers: HashMap<Pointer, StakeCredential>,
}

/// Result of parsing instantaneous_rewards (MIRs).
///
/// CDDL:
/// ```cddl
/// instantaneous_rewards = [
///   ir_reserves : { * credential_staking => coin },
///   ir_treasury : { * credential_staking => coin },
///   ir_delta_reserves : delta_coin,
///   ir_delta_treasury : delta_coin,
/// ]
/// ```
#[derive(Debug, Default)]
pub struct InstantaneousRewards {
    /// Rewards from reserves
    pub from_reserves: HashMap<StakeCredential, Lovelace>,
    /// Rewards from treasury
    pub from_treasury: HashMap<StakeCredential, Lovelace>,
    /// Delta to apply to reserves
    pub delta_reserves: i64,
    /// Delta to apply to treasury
    pub delta_treasury: i64,
}

impl InstantaneousRewards {
    /// Get combined rewards for a credential from both reserves and treasury
    pub fn get_combined(&self, credential: &StakeCredential) -> Lovelace {
        self.from_reserves.get(credential).copied().unwrap_or(0)
            + self.from_treasury.get(credential).copied().unwrap_or(0)
    }

    /// Get all combined rewards as a map
    pub fn combined_rewards(&self) -> HashMap<StakeCredential, Lovelace> {
        let mut combined = self.from_reserves.clone();
        for (credential, amount) in &self.from_treasury {
            *combined.entry(credential.clone()).or_insert(0) += amount;
        }
        combined
    }
}

/// Delegation state (d_state) from the ledger.
///
/// CDDL:
/// ```cddl
/// d_state = [
///   ds_unified : umap,
///   ds_future_gen_delegs : { * future_gen_deleg => gen_deleg_pair },
///   ds_gen_delegs : gen_delegs,
///   ds_i_rewards : instantaneous_rewards
/// ]
/// ```
#[derive(Debug)]
pub struct DState {
    /// Unified map with stake credentials, rewards, deposits, and delegations
    pub unified: UMap,
    /// Instantaneous rewards (MIRs)
    pub instant_rewards: InstantaneousRewards,
    // Note: future_gen_delegs and gen_delegs are skipped as they're not needed
}

// =============================================================================
// Parsing
// =============================================================================

/// Parse the unified map (umap) from the decoder.
fn parse_umap(decoder: &mut Decoder) -> Result<UMap> {
    let umap_len = decoder
        .array()
        .context("Failed to parse umap array")?
        .ok_or_else(|| anyhow!("umap must be definite-length array"))?;

    if umap_len < 2 {
        return Err(anyhow!(
            "umap array too short: expected 2 elements, got {umap_len}"
        ));
    }

    // Parse um_elems [0]: {* credential => um_elem}
    let elems: BTreeMap<StakeCredential, UMapElem> = decoder
        .decode()
        .context("Failed to parse um_elems map")?;

    // Parse um_pointers [1]: {* pointer => credential}
    let pointers: HashMap<Pointer, StakeCredential> = decoder
        .decode()
        .context("Failed to parse um_pointers map")?;

    info!(
        "      Parsed umap: {} credentials, {} pointers",
        elems.len(),
        pointers.len()
    );

    Ok(UMap { elems, pointers })
}

/// Parse instantaneous_rewards from the decoder.
fn parse_instantaneous_rewards(decoder: &mut Decoder) -> Result<InstantaneousRewards> {
    let ir_len = decoder
        .array()
        .context("Failed to parse instantaneous_rewards array")?
        .ok_or_else(|| anyhow!("instantaneous_rewards must be definite-length array"))?;

    if ir_len < 4 {
        return Err(anyhow!(
            "instantaneous_rewards array too short: expected 4 elements, got {ir_len}"
        ));
    }

    // Parse ir_reserves [0]: { * credential => coin }
    let from_reserves: HashMap<StakeCredential, Lovelace> = decoder
        .decode()
        .context("Failed to parse ir_reserves")?;

    // Parse ir_treasury [1]: { * credential => coin }
    let from_treasury: HashMap<StakeCredential, Lovelace> = decoder
        .decode()
        .context("Failed to parse ir_treasury")?;

    // Parse ir_delta_reserves [2]
    let delta_reserves: i64 = decoder
        .decode()
        .context("Failed to parse ir_delta_reserves")?;

    // Parse ir_delta_treasury [3]
    let delta_treasury: i64 = decoder
        .decode()
        .context("Failed to parse ir_delta_treasury")?;

    let total_mir_rewards: Lovelace = from_reserves.values().sum::<Lovelace>()
        + from_treasury.values().sum::<Lovelace>();

    if total_mir_rewards > 0 || delta_reserves != 0 || delta_treasury != 0 {
        info!(
            "      Parsed instantaneous_rewards: {} from reserves, {} from treasury, \
             delta_reserves={}, delta_treasury={}",
            from_reserves.len(),
            from_treasury.len(),
            delta_reserves,
            delta_treasury
        );
    }

    Ok(InstantaneousRewards {
        from_reserves,
        from_treasury,
        delta_reserves,
        delta_treasury,
    })
}

/// Parse d_state (delegation state) from the decoder.
///
/// The decoder should be positioned at the start of the d_state array.
pub fn parse_dstate(decoder: &mut Decoder) -> Result<DState> {
    let dstate_len = decoder
        .array()
        .context("Failed to parse d_state array")?
        .ok_or_else(|| anyhow!("d_state must be definite-length array"))?;

    if dstate_len < 4 {
        return Err(anyhow!(
            "d_state array too short: expected 4 elements, got {dstate_len}"
        ));
    }

    // Parse ds_unified [0]: umap
    let unified = parse_umap(decoder).context("Failed to parse ds_unified")?;

    // Skip ds_future_gen_delegs [1]
    decoder.skip().context("Failed to skip ds_future_gen_delegs")?;

    // Skip ds_gen_delegs [2]
    decoder.skip().context("Failed to skip ds_gen_delegs")?;

    // Parse ds_i_rewards [3]: instantaneous_rewards
    let instant_rewards = parse_instantaneous_rewards(decoder)
        .context("Failed to parse ds_i_rewards")?;

    Ok(DState {
        unified,
        instant_rewards,
    })
}

// =============================================================================
// Helper methods
// =============================================================================

impl UMapElem {
    /// Get the reward amount, or 0 if not registered
    pub fn reward(&self) -> Lovelace {
        match &self.reward_deposit {
            StrictMaybe::Just((reward, _)) => *reward,
            StrictMaybe::Nothing => 0,
        }
    }

    /// Get the deposit amount, or 0 if not registered
    pub fn deposit(&self) -> Lovelace {
        match &self.reward_deposit {
            StrictMaybe::Just((_, deposit)) => *deposit,
            StrictMaybe::Nothing => 0,
        }
    }

    /// Check if this credential is registered (has reward/deposit info)
    pub fn is_registered(&self) -> bool {
        matches!(self.reward_deposit, StrictMaybe::Just(_))
    }
}

impl DState {
    /// Get total unclaimed rewards across all credentials
    pub fn total_rewards(&self) -> Lovelace {
        self.unified.elems.values().map(|e| e.reward()).sum()
    }

    /// Get total deposits across all credentials
    pub fn total_deposits(&self) -> Lovelace {
        self.unified.elems.values().map(|e| e.deposit()).sum()
    }

    /// Get the number of registered stake credentials
    pub fn registered_count(&self) -> usize {
        self.unified.elems.values().filter(|e| e.is_registered()).count()
    }

    /// Convert the delegation state to a list of AccountState.
    ///
    /// This combines regular rewards from the unified map with instant rewards (MIRs).
    pub fn to_accounts(&self, network: &NetworkId) -> Vec<AccountState> {
        self.unified
            .elems
            .iter()
            .map(|(credential, elem)| {
                let stake_address = StakeAddress::new(credential.clone(), network.clone());

                // Get regular rewards + instant rewards (MIRs)
                let rewards = elem.reward() + self.instant_rewards.get_combined(credential);

                // Convert SPO delegation
                let delegated_spo = match &elem.pool {
                    StrictMaybe::Just(pool_id) => Some(*pool_id),
                    StrictMaybe::Nothing => None,
                };

                // Convert DRep delegation
                let delegated_drep = match &elem.drep {
                    StrictMaybe::Just(drep) => Some(match drep {
                        DRep::Key(hash) => DRepChoice::Key(*hash),
                        DRep::Script(hash) => DRepChoice::Script(*hash),
                        DRep::Abstain => DRepChoice::Abstain,
                        DRep::NoConfidence => DRepChoice::NoConfidence,
                    }),
                    StrictMaybe::Nothing => None,
                };

                AccountState {
                    stake_address,
                    address_state: StakeAddressState {
                        registered: true, // Accounts in DState are registered by definition
                        utxo_value: 0,    // Will be populated from UTXO parsing
                        rewards,
                        delegated_spo,
                        delegated_drep,
                    },
                }
            })
            .collect()
    }
}
