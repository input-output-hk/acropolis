//! Reward snapshot types and CBOR decoding for pulsing reward updates.
//!
//! This module handles the `pulsing_rew_update` structure from the NewEpochState,
//! which contains reward calculation state during epoch transitions.
//!
//! CDDL specification:
//! ```cddl
//! pulsing_rew_update =
//!   [
//!     0, ; pulsing
//!     reward_snapshot,
//!     pulser,
//!   ] /
//!   [
//!     1, ; complete
//!     reward_update,
//!   ]
//!
//! reward_snapshot =
//!   [
//!     reward_snapshot_fees : coin,
//!     reward_snapshot_prot_ver : prot_ver,
//!     reward_snapshot_nm : non_myopic,
//!     reward_snapshot_delta_r1 : coin,
//!     reward_snapshot_r : coin,
//!     reward_snapshot_delta_t1 : coin,
//!     reward_snapshot_likelihoods : {* key_hash<stake_pool> => likelihood },
//!     reward_snapshot_leaders : {* credential_staking => set<reward> }
//!   ]
//! ```

use minicbor::data::Type;
use minicbor::Decoder;
use serde::{Deserialize, Serialize};

use crate::protocol_params::ProtocolVersion;
use crate::rational_number::RationalNumber;
use crate::{Lovelace, PoolId, StakeCredential};

use super::mark_set_go::VMap;
use super::streaming_snapshot::SnapshotSet;

// =============================================================================
// Likelihood
// =============================================================================

/// Likelihood values for stake pool performance estimation.
/// Encoded as a sequence of log-likelihood values.
/// Uses RationalNumber to preserve precision from CBOR rational encoding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Likelihood(pub Vec<RationalNumber>);

impl<'b, C> minicbor::Decode<'b, C> for Likelihood {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        // Likelihood is encoded as an array of floats (or rationals)
        let len = d.array()?;
        let mut values = Vec::new();

        match len {
            Some(n) => {
                for _ in 0..n {
                    let value = decode_likelihood_value(d)?;
                    values.push(value);
                }
            }
            None => {
                while d.datatype()? != Type::Break {
                    let value = decode_likelihood_value(d)?;
                    values.push(value);
                }
                d.skip()?; // consume break
            }
        }

        Ok(Likelihood(values))
    }
}

fn decode_likelihood_value(d: &mut Decoder) -> Result<RationalNumber, minicbor::decode::Error> {
    // Check for CBOR tag 30 (rational number)
    if d.datatype()? == Type::Tag {
        let tag = d.tag()?;
        if tag.as_u64() == 30 {
            // Rational number: [numerator, denominator]
            d.array()?;
            let num: u64 = d.decode()?;
            let den: u64 = d.decode()?;
            return Ok(RationalNumber::from(num, den));
        }
    }

    // Try as float or integer - convert to rational representation
    match d.datatype()? {
        Type::F16 | Type::F32 | Type::F64 => {
            // For floats, we approximate as a rational
            // This is a fallback - ideally all values should be tag 30 rationals
            let f = d.f64()?;
            // Use a large denominator to preserve precision
            let scale = 1_000_000_000_000u64;
            let num = (f * scale as f64) as u64;
            Ok(RationalNumber::from(num, scale))
        }
        Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
            let num = d.u64()?;
            Ok(RationalNumber::from(num, 1))
        }
        other => Err(minicbor::decode::Error::message(format!(
            "unexpected type for likelihood value: {:?}",
            other
        ))),
    }
}

// =============================================================================
// Reward
// =============================================================================

/// Reward type enumeration matching Cardano ledger's RewardType
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RewardType {
    /// Rewards for pool members (delegators)
    Member,
    /// Rewards for pool operators (leaders)
    Leader,
}

impl<'b, C> minicbor::Decode<'b, C> for RewardType {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        match d.u8()? {
            0 => Ok(RewardType::Member),
            1 => Ok(RewardType::Leader),
            n => Err(minicbor::decode::Error::message(format!(
                "invalid reward type: {}",
                n
            ))),
        }
    }
}

/// A single reward entry
/// reward = [reward_type, pool_id, amount]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Reward {
    pub reward_type: RewardType,
    pub pool_id: PoolId,
    pub amount: Lovelace,
}

impl<'b, C> minicbor::Decode<'b, C> for Reward {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let reward_type = RewardType::decode(d, _ctx)?;
        let pool_id: PoolId = d.decode()?;
        let amount: Lovelace = d.decode()?;

        Ok(Reward {
            reward_type,
            pool_id,
            amount,
        })
    }
}

// =============================================================================
// NonMyopic
// =============================================================================

/// Non-myopic member rewards used for stake pool ranking.
/// Maps pool IDs to their historical performance likelihood.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NonMyopic {
    /// Map of pool ID to likelihood values
    pub likelihoods: VMap<PoolId, Likelihood>,
    /// Reward pot used for calculations
    pub reward_pot: Lovelace,
}

impl<'b, C> minicbor::Decode<'b, C> for NonMyopic {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let likelihoods: VMap<PoolId, Likelihood> = VMap::decode(d, ctx)?;
        let reward_pot: Lovelace = d.decode()?;

        Ok(NonMyopic {
            likelihoods,
            reward_pot,
        })
    }
}

// =============================================================================
// RewardSnapshot
// =============================================================================

/// Reward snapshot containing all data needed for reward calculation during pulsing.
///
/// This structure captures the state needed to compute rewards incrementally
/// across multiple blocks (pulsing) rather than all at once at epoch boundary.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RewardSnapshot {
    /// Fees collected during the epoch
    pub fees: Lovelace,
    /// Protocol version at snapshot time
    pub protocol_version: ProtocolVersion,
    /// Non-myopic member rewards data for pool ranking
    pub non_myopic: NonMyopic,
    /// Delta R1: Change to reserves for rewards
    pub delta_r1: Lovelace,
    /// R: Total rewards available
    pub r: Lovelace,
    /// Delta T1: Change to treasury
    pub delta_t1: Lovelace,
    /// Pool likelihoods for reward calculation
    pub likelihoods: VMap<PoolId, Likelihood>,
    /// Leader rewards per stake credential
    pub leaders: VMap<StakeCredential, SnapshotSet<Reward>>,
}

impl<'b, C> minicbor::Decode<'b, C> for RewardSnapshot {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;

        let fees: Lovelace = d.decode()?;
        let protocol_version = decode_protocol_version(d)?;
        let non_myopic: NonMyopic = NonMyopic::decode(d, ctx)?;
        let delta_r1: Lovelace = d.decode()?;
        let r: Lovelace = d.decode()?;
        let delta_t1: Lovelace = d.decode()?;
        let likelihoods: VMap<PoolId, Likelihood> = VMap::decode(d, ctx)?;
        let leaders: VMap<StakeCredential, SnapshotSet<Reward>> = VMap::decode(d, ctx)?;

        Ok(RewardSnapshot {
            fees,
            protocol_version,
            non_myopic,
            delta_r1,
            r,
            delta_t1,
            likelihoods,
            leaders,
        })
    }
}

fn decode_protocol_version(d: &mut Decoder) -> Result<ProtocolVersion, minicbor::decode::Error> {
    d.array()?;
    let major: u64 = d.decode()?;
    let minor: u64 = d.decode()?;
    Ok(ProtocolVersion { major, minor })
}

// =============================================================================
// RewardUpdate (for complete state)
// =============================================================================

/// Completed reward update containing final reward distribution.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RewardUpdate {
    /// Change to treasury
    pub delta_treasury: i64,
    /// Change to reserves
    pub delta_reserves: i64,
    /// Rewards per stake credential
    pub rewards: VMap<StakeCredential, SnapshotSet<Reward>>,
    /// Change to fees
    pub delta_fees: i64,
    /// Non-myopic data for next epoch
    pub non_myopic: NonMyopic,
}

impl<'b, C> minicbor::Decode<'b, C> for RewardUpdate {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;

        let delta_treasury: i64 = d.decode()?;
        let delta_reserves: i64 = d.decode()?;
        let rewards: VMap<StakeCredential, SnapshotSet<Reward>> = VMap::decode(d, ctx)?;
        let delta_fees: i64 = d.decode()?;
        let non_myopic: NonMyopic = NonMyopic::decode(d, ctx)?;

        Ok(RewardUpdate {
            delta_treasury,
            delta_reserves,
            rewards,
            delta_fees,
            non_myopic,
        })
    }
}

// =============================================================================
// Pulser (opaque for now)
// =============================================================================

/// Pulser state - the incremental reward calculation state.
/// For now we skip over this as it's complex internal state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pulser {
    // Opaque - we skip over the actual content for now
    _private: (),
}

impl Pulser {
    /// Skip over pulser data in the CBOR stream
    pub fn skip(d: &mut Decoder) -> Result<(), minicbor::decode::Error> {
        d.skip()
    }
}

// =============================================================================
// PulsingRewardUpdate
// =============================================================================

/// Pulsing reward update state from NewEpochState.
///
/// During epoch transitions, rewards are calculated incrementally ("pulsing")
/// across multiple blocks. This enum represents either the in-progress
/// pulsing state or the completed reward update.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PulsingRewardUpdate {
    /// Pulsing in progress - contains snapshot and pulser state
    Pulsing {
        /// The reward snapshot with calculation inputs
        snapshot: RewardSnapshot,
    },
    /// Reward calculation complete
    Complete {
        /// The final reward update
        update: RewardUpdate,
    },
}

impl<'b, C> minicbor::Decode<'b, C> for PulsingRewardUpdate {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let variant = d.u8()?;

        match variant {
            0 => {
                // Pulsing variant: [0, reward_snapshot, pulser]
                let snapshot = RewardSnapshot::decode(d, ctx)?;
                Pulser::skip(d)?; // Skip pulser state
                Ok(PulsingRewardUpdate::Pulsing { snapshot })
            }
            1 => {
                // Complete variant: [1, reward_update]
                let update = RewardUpdate::decode(d, ctx)?;
                Ok(PulsingRewardUpdate::Complete { update })
            }
            n => Err(minicbor::decode::Error::message(format!(
                "invalid pulsing_rew_update variant: {}",
                n
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reward_type_decode() {
        // Member = 0
        let bytes = [0x00];
        let mut decoder = Decoder::new(&bytes);
        let rt: RewardType = decoder.decode().unwrap();
        assert_eq!(rt, RewardType::Member);

        // Leader = 1
        let bytes = [0x01];
        let mut decoder = Decoder::new(&bytes);
        let rt: RewardType = decoder.decode().unwrap();
        assert_eq!(rt, RewardType::Leader);
    }
}
