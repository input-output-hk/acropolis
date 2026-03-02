// SPDX-License-Identifier: Apache-2.0
// Copyright © 2025, Acropolis team.

//! Streaming snapshot parser with callback interface for bootstrap process.
//!
//! This module provides a callback-based streaming parser for Cardano snapshots
//! that allows processing large snapshots without loading the entire structure
//! into memory. It's designed for the bootstrap process to distribute state
//! via message bus.
//!
//! The parser navigates the NewEpochState structure and invokes callbacks for:
//! - UTXOs (per-entry callback for each UTXO)
//! - Stake pools (bulk callback with all pool data)
//! - Stake accounts (bulk callback with delegations and rewards)
//! - DReps (bulk callback with governance info)
//! - Proposals (bulk callback with active governance actions)
//!
//! Parses CBOR dumps from Cardano Haskell node's GetCBOR ledger-state query.
//! These snapshots represent the internal `NewEpochState` type and are not formally
//! specified - see: https://github.com/IntersectMBO/cardano-ledger/blob/33e90ea03447b44a389985ca2b158568e5f4ad65/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState/Types.hs#L121-L131
//! and https://github.com/rrruko/nes-cddl-hs/blob/main/nes.cddl

use anyhow::{anyhow, Context, Result};
use minicbor::data::Type;
use minicbor::Decoder;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::net::{Ipv4Addr, Ipv6Addr};
use std::path::{Path, PathBuf};
use tracing::{error, info};

use crate::epoch_snapshot::SnapshotsContainer;
use crate::hash::Hash;
use crate::ledger_state::SPOState;
use crate::snapshot::utxo::{SnapshotUTxO, UtxoEntry};
use crate::snapshot::RawSnapshot;
pub use crate::stake_addresses::{AccountState, StakeAddressState};
pub use crate::{
    Constitution, DRepChoice, DRepCredential, DRepRecord, EpochBootstrapData, Lovelace,
    MultiHostName, NetworkId, PoolId, PoolMetadata, PoolRegistration, Ratio, Relay, SingleHostAddr,
    SingleHostName, StakeAddress, StakeCredential,
};
use crate::{DataHash, Epoch, PoolBlockProduction, Pots, ProtocolParamUpdate, RewardParams};
// Import snapshot parsing support
use super::mark_set_go::{RawSnapshotsContainer, SnapshotsCallback};
use super::reward_snapshot::PulsingRewardUpdate;

/// Result of parsing pulsing_rew_update, containing rewards and pot deltas
#[derive(Debug, Default)]
pub struct PulsingRewardResult {
    /// Map of stake credentials to their total rewards
    pub rewards: HashMap<StakeCredential, u64>,
    /// Delta to apply to treasury (positive = increase)
    pub delta_treasury: i64,
    /// Delta to apply to reserves (positive = increase, but typically negative)
    pub delta_reserves: i64,
    /// Delta to apply to fees/deposits (stored inverted in CBOR)
    pub delta_fees: i64,
}

/// Result of parsing instantaneous_rewards, containing rewards and pot deltas
#[derive(Debug, Default)]
pub struct InstantRewardsResult {
    /// Map of stake credentials to their total instant rewards
    pub rewards: HashMap<StakeCredential, u64>,
    /// Delta to apply to treasury from MIR
    pub delta_treasury: i64,
    /// Delta to apply to reserves from MIR
    pub delta_reserves: i64,
}

// -----------------------------------------------------------------------------
// Cardano Ledger Types (for decoding with minicbor)
// -----------------------------------------------------------------------------

/*
 * This was replaced with the StakeCredential defined in types.rs, but the implementation here is much
 * cleaner for parsing CBOR files from Haskell Node, using hash.rs types. For CBOR parsing, we need to
 * change the decode from using d.decode_with(ctx) (which expects arrays) tousing d.bytes() which
 * expects raw bytes.
/// Stake credential - can be a key hash or script hash
/// Order matters for Ord/PartialOrd - ScriptHash must come first for compatibility with Haskell
#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Hash)]
pub enum StakeCredential {
    ScriptHash(ScriptHash),
    AddrKeyhash(AddrKeyhash), // NOTE: lower case h from hash.rs version
}
*/

impl<'b, C> minicbor::decode::Decode<'b, C> for StakeCredential {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let variant = d.u16()?;

        // CDDL: credential = [0, addr_keyhash // 1, scripthash]
        // Variant 0 = key hash, Variant 1 = script hash
        match variant {
            0 => {
                // AddrKeyHash variant - key hash credential
                let bytes = d.bytes()?;
                let key_hash = bytes.try_into().map_err(|_| {
                    minicbor::decode::Error::message(
                        "invalid length for AddrKeyHash in StakeCredential",
                    )
                })?;
                Ok(StakeCredential::AddrKeyHash(key_hash))
            }
            1 => {
                // ScriptHash variant - script hash credential
                let bytes = d.bytes()?;
                let key_hash = bytes.try_into().map_err(|_| {
                    minicbor::decode::Error::message(
                        "invalid length for ScriptHash in StakeCredential",
                    )
                })?;
                Ok(StakeCredential::ScriptHash(key_hash))
            }
            _ => Err(minicbor::decode::Error::message(
                "invalid variant id for StakeCredential",
            )),
        }
    }
}

impl<C> minicbor::encode::Encode<C> for StakeCredential {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        // CDDL: credential = [0, addr_keyhash // 1, scripthash]
        match self {
            StakeCredential::AddrKeyHash(key_hash) => {
                // AddrKeyHash is variant 0 (key hash)
                e.array(2)?;
                e.encode_with(0, ctx)?;
                e.encode_with(key_hash, ctx)?;
                Ok(())
            }
            StakeCredential::ScriptHash(key_hash) => {
                // ScriptHash is variant 1 (script hash)
                e.array(2)?;
                e.encode_with(1, ctx)?;
                e.encode_with(key_hash, ctx)?;
                Ok(())
            }
        }
    }
}

/// Maybe type (optional with explicit encoding)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrictMaybe<T> {
    Nothing,
    Just(T),
}

impl<'b, C, T> minicbor::Decode<'b, C> for StrictMaybe<T>
where
    T: minicbor::Decode<'b, C>,
{
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        match d.datatype()? {
            Type::Null | Type::Undefined => {
                d.skip()?;
                Ok(StrictMaybe::Nothing)
            }
            Type::Array | Type::ArrayIndef => {
                // Try the array-wrapper shape first:
                //   []      -> Nothing
                //   [value] -> Just value
                //
                // If probing fails, fall back to decoding a direct T (which itself might be an
                // array, such as DRep).
                let mut probe = d.clone();
                match probe.array()? {
                    Some(0) => {
                        d.array()?;
                        Ok(StrictMaybe::Nothing)
                    }
                    Some(1) => {
                        if T::decode(&mut probe, ctx).is_ok() {
                            d.array()?;
                            let value = T::decode(d, ctx)?;
                            Ok(StrictMaybe::Just(value))
                        } else {
                            let value = T::decode(d, ctx)?;
                            Ok(StrictMaybe::Just(value))
                        }
                    }
                    None => match probe.datatype()? {
                        Type::Break => {
                            d.array()?;
                            d.skip()?;
                            Ok(StrictMaybe::Nothing)
                        }
                        _ => {
                            if T::decode(&mut probe, ctx).is_ok()
                                && matches!(probe.datatype()?, Type::Break)
                            {
                                d.array()?;
                                let value = T::decode(d, ctx)?;
                                d.skip()?;
                                Ok(StrictMaybe::Just(value))
                            } else {
                                let value = T::decode(d, ctx)?;
                                Ok(StrictMaybe::Just(value))
                            }
                        }
                    },
                    Some(_) => {
                        let value = T::decode(d, ctx)?;
                        Ok(StrictMaybe::Just(value))
                    }
                }
            }
            _ => {
                let value = T::decode(d, ctx)?;
                Ok(StrictMaybe::Just(value))
            }
        }
    }
}

/// Anchor (URL + content hash)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Anchor {
    pub url: String,
    pub content_hash: Hash<32>,
}

impl<'b, C> minicbor::Decode<'b, C> for Anchor {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        // URL can be either bytes or text string
        let url = match d.datatype()? {
            Type::Bytes => {
                let url_bytes = d.bytes()?;
                String::from_utf8_lossy(url_bytes).to_string()
            }
            Type::String => d.str()?.to_string(),
            _ => {
                return Err(minicbor::decode::Error::message(
                    "Expected bytes or string for URL",
                ));
            }
        };
        let content_hash = Hash::<32>::decode(d, ctx)?;
        Ok(Anchor { url, content_hash })
    }
}

/// Set type (encoded as array, sometimes with CBOR tag 258)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotSet<T>(pub Vec<T>);

impl<T> SnapshotSet<T> {
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }
}

impl<T> From<Vec<T>> for SnapshotSet<T> {
    fn from(vec: Vec<T>) -> Self {
        SnapshotSet(vec)
    }
}

impl<T> From<SnapshotSet<T>> for Vec<T> {
    fn from(set: SnapshotSet<T>) -> Self {
        set.0
    }
}

impl<'b, C, T> minicbor::Decode<'b, C> for SnapshotSet<T>
where
    T: minicbor::Decode<'b, C>,
{
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        // Sets might be tagged with CBOR tag 258
        if matches!(d.datatype()?, Type::Tag) {
            d.tag()?;
        }

        let vec: Vec<T> = d.decode_with(ctx)?;
        Ok(SnapshotSet(vec))
    }
}

impl<C, T> minicbor::Encode<C> for SnapshotSet<T>
where
    T: minicbor::Encode<C>,
{
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.encode_with(&self.0, ctx)?;
        Ok(())
    }
}

/// DRep credential for governance delegation (internal CBOR type)
#[derive(Serialize, Deserialize, Debug, PartialEq, PartialOrd, Eq, Ord, Clone)]
pub enum DRep {
    Key(AddrKeyhash),
    Script(ScriptHash),
    Abstain,
    NoConfidence,
}

impl<'b, C> minicbor::Decode<'b, C> for DRep {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let variant = d.u16()?;

        match variant {
            0 => Ok(DRep::Key(d.decode_with(ctx)?)),
            1 => Ok(DRep::Script(d.decode_with(ctx)?)),
            2 => Ok(DRep::Abstain),
            3 => Ok(DRep::NoConfidence),
            _ => Err(minicbor::decode::Error::message(
                "invalid variant id for DRep",
            )),
        }
    }
}

impl<C> minicbor::Encode<C> for DRep {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        match self {
            DRep::Key(h) => {
                e.array(2)?;
                e.encode_with(0, ctx)?;
                e.encode_with(h, ctx)?;
                Ok(())
            }
            DRep::Script(h) => {
                e.array(2)?;
                e.encode_with(1, ctx)?;
                e.encode_with(h, ctx)?;
                Ok(())
            }
            DRep::Abstain => {
                e.array(1)?;
                e.encode_with(2, ctx)?;
                Ok(())
            }
            DRep::NoConfidence => {
                e.array(1)?;
                e.encode_with(3, ctx)?;
                Ok(())
            }
        }
    }
}

/// Account state from ledger (internal CBOR type for decoding)
///
/// This is converted to AccountState for the external API.
#[derive(Debug)]
struct SnapshotAccountValue {
    pub balance: Lovelace,
    pub pool: StrictMaybe<PoolId>,
    pub drep: StrictMaybe<DRep>,
}

impl<'b, C> minicbor::Decode<'b, C> for SnapshotAccountValue {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let len = d.array()?;

        // Conway account state: [balance, deposit, stake_pool_delegation, drep_delegation]
        let balance = d.decode_with(ctx)?;
        d.skip()?; // deposit
        let pool = d.decode_with(ctx)?;
        let drep = d.decode_with(ctx)?;

        skip_remaining_array_items(d, len, 4)?;

        Ok(Self {
            balance,
            pool,
            drep,
        })
    }
}

impl SnapshotAccountValue {
    fn to_normalized(&self) -> NormalizedAccount {
        NormalizedAccount {
            rewards: self.balance,
            delegated_spo: strict_maybe_copy(&self.pool),
            delegated_drep: strict_maybe_cloned(&self.drep).map(drep_to_choice),
        }
    }
}

#[derive(Debug, Clone)]
struct NormalizedAccount {
    rewards: Lovelace,
    delegated_spo: Option<PoolId>,
    delegated_drep: Option<DRepChoice>,
}

// -----------------------------------------------------------------------------
// Type decoders for snapshot compatibility
// -----------------------------------------------------------------------------

pub use crate::types::AddrKeyhash;
pub use crate::types::ScriptHash;

pub struct SnapshotContext {
    pub network: NetworkId,
}

impl AsRef<SnapshotContext> for SnapshotContext {
    fn as_ref(&self) -> &Self {
        self
    }
}

fn skip_remaining_array_items(
    d: &mut Decoder<'_>,
    len: Option<u64>,
    already_read: u64,
) -> Result<(), minicbor::decode::Error> {
    match len {
        Some(array_len) => {
            for _ in already_read..array_len {
                d.skip()?;
            }
        }
        None => loop {
            match d.datatype()? {
                Type::Break => {
                    d.skip()?;
                    break;
                }
                _ => d.skip()?,
            }
        },
    }

    Ok(())
}

fn strict_maybe_copy<T: Copy>(value: &StrictMaybe<T>) -> Option<T> {
    match value {
        StrictMaybe::Nothing => None,
        StrictMaybe::Just(inner) => Some(*inner),
    }
}

fn strict_maybe_cloned<T: Clone>(value: &StrictMaybe<T>) -> Option<T> {
    match value {
        StrictMaybe::Nothing => None,
        StrictMaybe::Just(inner) => Some(inner.clone()),
    }
}

fn drep_to_choice(drep: DRep) -> DRepChoice {
    match drep {
        DRep::Key(hash) => DRepChoice::Key(hash),
        DRep::Script(hash) => DRepChoice::Script(hash),
        DRep::Abstain => DRepChoice::Abstain,
        DRep::NoConfidence => DRepChoice::NoConfidence,
    }
}

fn decode_stake_address_compat<'b, C>(
    d: &mut Decoder<'b>,
    ctx: &mut C,
) -> Result<StakeAddress, minicbor::decode::Error>
where
    C: AsRef<SnapshotContext>,
{
    match d.datatype()? {
        Type::Bytes => {
            let bytes = d.bytes()?;

            match bytes.len() {
                // RewardAccount encoding (bytes)
                29 => StakeAddress::from_binary(bytes)
                    .map_err(|e| minicbor::decode::Error::message(e.to_string())),
                // Key hash encoding (raw bytes)
                28 => {
                    let hash = Hash::<28>::try_from(bytes)
                        .map_err(|e| minicbor::decode::Error::message(e.to_string()))?;
                    Ok(StakeAddress::new(
                        StakeCredential::AddrKeyHash(hash),
                        ctx.as_ref().network.clone(),
                    ))
                }
                len => Err(minicbor::decode::Error::message(format!(
                    "Unexpected stake credential/address byte length: {len}"
                ))),
            }
        }
        Type::Array | Type::ArrayIndef => {
            let credential = d.decode_with(ctx)?;
            Ok(StakeAddress::new(credential, ctx.as_ref().network.clone()))
        }
        other => Err(minicbor::decode::Error::message(format!(
            "Expected stake credential/address bytes or array, got {other:?}"
        ))),
    }
}

struct SnapshotOption<T>(pub Option<T>);

impl<'b, C, T> minicbor::Decode<'b, C> for SnapshotOption<T>
where
    T: minicbor::Decode<'b, C>,
{
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        match d.datatype()? {
            Type::Null | Type::Undefined => {
                d.skip()?;
                Ok(SnapshotOption(None))
            }
            // Some ledger versions encode `StrictMaybe a` as:
            //   []      -> Nothing
            //   [a]     -> Just a
            // We support that shape here in addition to plain `a` / null.
            Type::Array | Type::ArrayIndef => {
                let mut probe = d.clone();
                match probe.array()? {
                    Some(0) => {
                        d.array()?;
                        Ok(SnapshotOption(None))
                    }
                    Some(1) => {
                        d.array()?;
                        let t = T::decode(d, ctx)?;
                        Ok(SnapshotOption(Some(t)))
                    }
                    None => match probe.datatype()? {
                        Type::Break => {
                            d.array()?;
                            d.skip()?;
                            Ok(SnapshotOption(None))
                        }
                        _ => {
                            let t = T::decode(d, ctx)?;
                            Ok(SnapshotOption(Some(t)))
                        }
                    },
                    _ => {
                        let t = T::decode(d, ctx)?;
                        Ok(SnapshotOption(Some(t)))
                    }
                }
            }
            _ => {
                let t = T::decode(d, ctx)?;
                Ok(SnapshotOption(Some(t)))
            }
        }
    }
}

pub struct SnapshotPoolRegistration(pub PoolRegistration);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotPoolRegistration
where
    C: AsRef<SnapshotContext>,
{
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let len = d.array()?;
        let operator = d.decode_with(ctx).map_err(|e| {
            minicbor::decode::Error::message(format!("failed to decode operator PoolId: {e}"))
        })?;
        let vrf_key_hash = d.decode_with(ctx).map_err(|e| {
            minicbor::decode::Error::message(format!("failed to decode vrf key hash: {e}"))
        })?;
        let pledge = d.decode_with(ctx).map_err(|e| {
            minicbor::decode::Error::message(format!("failed to decode pledge: {e}"))
        })?;
        let cost = d
            .decode_with(ctx)
            .map_err(|e| minicbor::decode::Error::message(format!("failed to decode cost: {e}")))?;
        let margin = SnapshotRatio::decode(d, ctx)
            .map_err(|e| minicbor::decode::Error::message(format!("failed to decode margin: {e}")))?
            .0;
        let reward_account = decode_stake_address_compat(d, ctx).map_err(|e| {
            minicbor::decode::Error::message(format!("failed to decode reward account: {e}"))
        })?;
        let pool_owners = SnapshotSet::<SnapshotStakeAddress>::decode(d, ctx)
            .map_err(|e| minicbor::decode::Error::message(format!("failed to decode owners: {e}")))?
            .0
            .into_iter()
            .map(|a| a.0)
            .collect();
        let relays = Vec::<SnapshotRelay>::decode(d, ctx)
            .map_err(|e| minicbor::decode::Error::message(format!("failed to decode relays: {e}")))?
            .into_iter()
            .map(|r| r.0)
            .collect();
        let pool_metadata = SnapshotOption::<SnapshotPoolMetadata>::decode(d, ctx)
            .map_err(|e| {
                minicbor::decode::Error::message(format!("failed to decode pool metadata: {e}"))
            })?
            .0
            .map(|m| m.0);

        // Newer stake pool state variants may append extra fields after PoolRegistration.
        skip_remaining_array_items(d, len, 9)?;

        Ok(Self(PoolRegistration {
            operator,
            vrf_key_hash,
            pledge,
            cost,
            margin,
            reward_account,
            pool_owners,
            relays,
            pool_metadata,
        }))
    }
}

/// Future pool params can be encoded without the operator field because the map key is already
/// the pool id. We decode this shape and reattach the operator from the map key.
struct SnapshotPoolRegistrationWithoutOperator(pub PoolRegistration);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotPoolRegistrationWithoutOperator
where
    C: AsRef<SnapshotContext>,
{
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let len = d.array()?;
        let vrf_key_hash = d.decode_with(ctx).map_err(|e| {
            minicbor::decode::Error::message(format!("failed to decode vrf key hash: {e}"))
        })?;
        let pledge = d.decode_with(ctx).map_err(|e| {
            minicbor::decode::Error::message(format!("failed to decode pledge: {e}"))
        })?;
        let cost = d
            .decode_with(ctx)
            .map_err(|e| minicbor::decode::Error::message(format!("failed to decode cost: {e}")))?;
        let margin = SnapshotRatio::decode(d, ctx)
            .map_err(|e| minicbor::decode::Error::message(format!("failed to decode margin: {e}")))?
            .0;
        let reward_account = decode_stake_address_compat(d, ctx).map_err(|e| {
            minicbor::decode::Error::message(format!("failed to decode reward account: {e}"))
        })?;
        let pool_owners = SnapshotSet::<SnapshotStakeAddress>::decode(d, ctx)
            .map_err(|e| minicbor::decode::Error::message(format!("failed to decode owners: {e}")))?
            .0
            .into_iter()
            .map(|a| a.0)
            .collect();
        let relays = Vec::<SnapshotRelay>::decode(d, ctx)
            .map_err(|e| minicbor::decode::Error::message(format!("failed to decode relays: {e}")))?
            .into_iter()
            .map(|r| r.0)
            .collect();
        let pool_metadata = SnapshotOption::<SnapshotPoolMetadata>::decode(d, ctx)
            .map_err(|e| {
                minicbor::decode::Error::message(format!("failed to decode pool metadata: {e}"))
            })?
            .0
            .map(|m| m.0);

        // Newer stake pool state variants may append extra fields after StakePoolParams.
        skip_remaining_array_items(d, len, 8)?;

        Ok(Self(PoolRegistration {
            operator: PoolId::default(),
            vrf_key_hash,
            pledge,
            cost,
            margin,
            reward_account,
            pool_owners,
            relays,
            pool_metadata,
        }))
    }
}

struct SnapshotRatio(pub Ratio);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotRatio {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        // UnitInterval might be tagged (tag 30 for rational)
        if matches!(d.datatype()?, Type::Tag) {
            d.tag()?;
        }
        d.array()?;
        let numerator = d.u64()?;
        let denominator = d.u64()?;
        Ok(Self(Ratio {
            numerator,
            denominator,
        }))
    }
}

// Network types for pool relays
pub type SnapshotPort = u32;

struct SnapshotStakeAddress(pub StakeAddress);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotStakeAddress
where
    C: AsRef<SnapshotContext>,
{
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        Ok(Self(decode_stake_address_compat(d, ctx)?))
    }
}

struct SnapshotRelay(pub Relay);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotRelay {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let tag = d.u32()?;

        match tag {
            0 => {
                // SingleHostAddr
                let port = Option::<SnapshotPort>::decode(d, ctx)?.map(|p| p as u16);
                let ipv4 = Option::<Ipv4Addr>::decode(d, ctx)?;
                let ipv6 = Option::<Ipv6Addr>::decode(d, ctx)?;
                Ok(Self(Relay::SingleHostAddr(SingleHostAddr {
                    port,
                    ipv4,
                    ipv6,
                })))
            }
            1 => {
                // SingleHostName
                let port = Option::<SnapshotPort>::decode(d, ctx)?.map(|p| p as u16);
                let dns_name = d.str()?.to_string();
                Ok(Self(Relay::SingleHostName(SingleHostName {
                    port,
                    dns_name,
                })))
            }
            2 => {
                // MultiHostName
                let dns_name = d.str()?.to_string();
                Ok(Self(Relay::MultiHostName(MultiHostName { dns_name })))
            }
            _ => Err(minicbor::decode::Error::message("Invalid relay tag")),
        }
    }
}

struct SnapshotPoolMetadata(pub PoolMetadata);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotPoolMetadata {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let url = d.str()?.to_string();
        let hash = DataHash::decode(d, ctx)?;
        Ok(SnapshotPoolMetadata(PoolMetadata { url, hash }))
    }
}

// -----------------------------------------------------------------------------
// DRep State
// -----------------------------------------------------------------------------

/// DRep state from ledger
#[derive(Debug, Clone)]
pub struct DRepState {
    pub expiry: Epoch,
    pub anchor: StrictMaybe<Anchor>,
    pub deposit: Lovelace,
    pub delegators: SnapshotSet<StakeCredential>,
}

impl<'b, C> minicbor::Decode<'b, C> for DRepState {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        // DRepState might be tagged or just an array - check what we have
        if matches!(d.datatype()?, Type::Tag) {
            d.tag()?; // skip the tag
        }

        d.array()?;
        let expiry = d.u64()?;
        let anchor = StrictMaybe::<Anchor>::decode(d, ctx)?;
        let deposit = d.u64()?;

        // Delegators set might be tagged (CBOR tag 258 for sets)
        if matches!(d.datatype()?, Type::Tag) {
            d.tag()?; // skip the tag
        }
        let delegators = SnapshotSet::<StakeCredential>::decode(d, ctx)?;

        Ok(DRepState {
            expiry,
            anchor,
            deposit,
            delegators,
        })
    }
}

// -----------------------------------------------------------------------------
// Ledger types for DState parsing
// -----------------------------------------------------------------------------

/// DRep information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DRepInfo {
    /// DRep credential
    pub drep_id: DRepCredential,
    /// Lovelace deposit amount
    pub deposit: u64,
    /// Optional anchor (URL and hash)
    pub anchor: Option<AnchorInfo>,
}

/// Governance proposal
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GovernanceProposal {
    /// Lovelace deposit amount
    pub deposit: u64,
    /// Bech32-encoded stake address of proposer
    pub reward_account: String,
    /// Bech32-encoded governance action ID
    pub gov_action_id: String,
    /// Governance action type
    pub gov_action: String,
    /// Anchor information
    pub anchor: AnchorInfo,
}

/// Anchor information (reference URL and data hash) - for OpenAPI compatibility
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnchorInfo {
    /// IPFS or HTTP(S) URL containing anchor data
    pub url: String,
    /// Hex-encoded hash of the anchor data
    pub data_hash: String,
}

/// Snapshot metadata extracted before streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    /// Epoch number
    pub epoch: u64,
    /// Pot balances
    pub pot_balances: Pots,
    /// Total number of UTXOs (for progress tracking)
    pub utxo_count: Option<u64>,
    /// Block production statistics for previous epoch
    pub blocks_previous_epoch: Vec<PoolBlockProduction>,
    /// Block production statistics for current epoch
    pub blocks_current_epoch: Vec<PoolBlockProduction>,
}

// -----------------------------------------------------------------------------
// Callback Traits
// -----------------------------------------------------------------------------

/// Callback invoked for each UTXO entry (streaming)
pub trait UtxoCallback {
    /// Called once per UTXO entry
    fn on_utxo(&mut self, utxo: UtxoEntry) -> Result<()>;
}

pub trait EpochCallback {
    fn on_epoch(&mut self, data: EpochBootstrapData) -> Result<()>;
}

/// Callback invoked with bulk stake pool data
pub trait PoolCallback {
    /// Called once with all pool data
    fn on_pools(&mut self, spo_state: SPOState) -> Result<()>;
}

/// Data needed to bootstrap accounts state, parsed from the snapshot.
/// This is a pure data structure - the publisher is responsible for
/// converting it to the appropriate message type.
#[derive(Debug, Clone)]
pub struct AccountsBootstrapData {
    /// Epoch number this snapshot is for
    pub epoch: u64,
    /// All account states (stake addresses with delegations and balances)
    pub accounts: Vec<AccountState>,
    /// All registered stake pools with their full registration data
    pub pools: Vec<PoolRegistration>,
    /// Pool IDs that are retiring
    pub retiring_pools: Vec<PoolId>,
    /// All registered DReps with their deposits (credential, deposit amount)
    pub dreps: Vec<(DRepCredential, u64)>,
    /// Treasury, reserves, and deposits for the snapshot epoch
    pub pots: Pots,
    /// Pot deltas to apply at epoch boundary transition
    pub pot_deltas: crate::messages::BootstrapPotDeltas,
    /// Fully processed bootstrap snapshots (mark/set/go) for rewards calculation.
    /// Empty (default) for pre-Shelley eras.
    pub snapshots: SnapshotsContainer,
}

/// Callback invoked with accounts bootstrap data
pub trait AccountsCallback {
    /// Called once with all data needed to bootstrap accounts state
    fn on_accounts(&mut self, data: AccountsBootstrapData) -> Result<()>;
}

/// Callback invoked with bulk DRep data
pub trait DRepCallback {
    /// Called once with all DRep data
    fn on_dreps(&mut self, epoch: u64, dreps: HashMap<DRepCredential, DRepRecord>) -> Result<()>;
}

/// Callback invoked with bulk governance proposal data
pub trait ProposalCallback {
    /// Called once with all proposals
    fn on_proposals(&mut self, proposals: Vec<GovernanceProposal>) -> Result<()>;
}

/// Callback invoked with Governance State ProtocolParameters (previous, current, future)
pub trait GovernanceProtocolParametersCallback {
    /// Called once with all proposals
    fn on_gs_protocol_parameters(
        &mut self,
        epoch: u64,
        previous_reward_params: RewardParams,
        current_reward_params: RewardParams,
        params: ProtocolParamUpdate,
    ) -> Result<()>;
}

/// Callback invoked with full governance state from the snapshot
pub trait GovernanceStateCallback {
    /// Called once with the full governance state (proposals, votes, committee, constitution)
    fn on_governance_state(&mut self, state: super::governance::GovernanceState) -> Result<()>;
}

/// Combined callback handler for all snapshot data
pub trait SnapshotCallbacks:
    UtxoCallback
    + PoolCallback
    + AccountsCallback
    + DRepCallback
    + GovernanceProtocolParametersCallback
    + GovernanceStateCallback
    + ProposalCallback
    + SnapshotsCallback
    + EpochCallback
{
    /// Called before streaming begins with metadata
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()>;

    /// Called after all streaming is complete
    fn on_complete(&mut self) -> Result<()>;
}

// -----------------------------------------------------------------------------
// Internal Types
// -----------------------------------------------------------------------------

#[expect(dead_code)]
struct ParsedMetadata {
    epoch: u64,
    treasury: u64,
    reserves: u64,
    pools: SPOState,
    dreps: Vec<DRepInfo>,
    accounts: Vec<AccountState>,
    blocks_previous_epoch: Vec<PoolBlockProduction>,
    blocks_current_epoch: Vec<PoolBlockProduction>,
    utxo_position: u64,
}

#[expect(dead_code)]
struct ParsedMetadataWithoutUtxoPosition {
    epoch: u64,
    treasury: u64,
    reserves: u64,
    pools: SPOState,
    dreps: Vec<DRepInfo>,
    accounts: Vec<AccountState>,
    blocks_previous_epoch: Vec<PoolBlockProduction>,
    blocks_current_epoch: Vec<PoolBlockProduction>,
}

// -----------------------------------------------------------------------------
// Streaming Parser
// -----------------------------------------------------------------------------

/// Streaming snapshot parser with callback interface
pub struct StreamingSnapshotParser {
    file_path: String,
    chunk_size: usize,
}

/// Chunked CBOR reader for large files (infrastructure for future optimization)
struct ChunkedCborReader {
    file: File,
    file_size: u64,
}

impl ChunkedCborReader {
    fn new(mut file: File, _chunk_size: usize) -> Result<Self> {
        let file_size = file.seek(SeekFrom::End(0))?;
        file.seek(SeekFrom::Start(0))?;

        Ok(ChunkedCborReader { file, file_size })
    }
}

impl StreamingSnapshotParser {
    fn utxo_sidecar_path(snapshot_path: &Path) -> Option<PathBuf> {
        let file_name = snapshot_path.file_name()?.to_str()?;

        // If parser is already pointed at a UTxO file, keep it as-is.
        if file_name.starts_with("utxos.") {
            return Some(snapshot_path.to_path_buf());
        }

        let suffix = file_name.strip_prefix("nes.").unwrap_or(file_name);
        let parent = snapshot_path.parent().unwrap_or_else(|| Path::new(""));
        Some(parent.join(format!("utxos.{suffix}")))
    }

    fn find_utxo_sidecar_path(&self) -> Option<PathBuf> {
        let snapshot_path = Path::new(&self.file_path);
        let candidate = Self::utxo_sidecar_path(snapshot_path);

        match candidate {
            Some(path) if path.exists() => Some(path),
            _ => None,
        }
    }

    fn parse_empty_utxo_placeholder_bytes(file: &mut File, offset: u64) -> Result<u64> {
        // Empty map placeholder requires only a tiny probe:
        // header + optional break token for indefinite maps.
        const PROBE_SIZE: usize = 64;

        file.seek(SeekFrom::Start(offset))?;
        let mut buffer = [0u8; PROBE_SIZE];
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            return Err(anyhow!(
                "Expected empty UTxO placeholder map at offset {offset}, but reached EOF"
            ));
        }

        let mut decoder = Decoder::new(&buffer[..bytes_read]);
        match decoder.datatype()? {
            Type::Map | Type::MapIndef => {}
            other => {
                return Err(anyhow!(
                    "Expected UTxO placeholder map at offset {offset}, got {other:?}"
                ));
            }
        }

        let map_len = decoder.map()?;
        match map_len {
            Some(0) => Ok(decoder.position() as u64),
            Some(len) => Err(anyhow!(
                "Expected empty UTxO placeholder map at offset {offset}, found {len} embedded entries (embedded UTxOs are no longer supported)"
            )),
            None => {
                if matches!(decoder.datatype(), Ok(Type::Break)) {
                    decoder.skip()?;
                    Ok(decoder.position() as u64)
                } else {
                    Err(anyhow!(
                        "Expected empty indefinite UTxO placeholder map at offset {offset}, found embedded entries (embedded UTxOs are no longer supported)"
                    ))
                }
            }
        }
    }

    fn progress_bar(progress_pct: f64) -> String {
        let width = 20usize;
        let filled = ((progress_pct.clamp(0.0, 100.0) / 100.0) * width as f64).round() as usize;
        let filled = filled.min(width);
        format!("[{}{}]", "=".repeat(filled), " ".repeat(width - filled))
    }

    fn bytes_to_mb(bytes: u64) -> f64 {
        bytes as f64 / 1024.0 / 1024.0
    }

    /// Create a new streaming parser for the given snapshot file
    pub fn new(file_path: impl Into<String>) -> Self {
        Self {
            file_path: file_path.into(),
            chunk_size: 16 * 1024 * 1024, // 16MB chunks
        }
    }

    /// Create a new streaming parser with custom chunk size
    pub fn with_chunk_size(file_path: impl Into<String>, chunk_size: usize) -> Self {
        Self {
            file_path: file_path.into(),
            chunk_size,
        }
    }

    /// Parse the snapshot file and invoke callbacks
    ///
    /// This method navigates the NewEpochState structure:
    /// ```text
    /// NewEpochState = [
    ///   0: epoch_no,
    ///   1: blocks_previous_epoch,
    ///   2: blocks_current_epoch,
    ///   3: EpochState = [
    ///        0: AccountState = [treasury, reserves],
    ///        1: LedgerState = [
    ///             0: CertState = [
    ///                  0: VState = [dreps, cc, dormant_epoch],
    ///                  1: PState = [pools, future_pools, retiring, deposits],
    ///                  2: DState = [unified_rewards, fut_gen_deleg, gen_deleg, instant_rewards],
    ///                ],
    ///             1: UTxOState = [
    ///                  0: utxos (map: TxIn -> TxOut),
    ///                  1: deposits,
    ///                  2: fees,
    ///                  3: gov_state,
    ///                  4: donations,
    ///                ],
    ///           ],
    ///        2: PParams,
    ///        3: PParamsPrevious,
    ///      ],
    ///   4: PoolDistr,
    ///   5: StakeDistr,
    /// ]
    /// ```
    pub fn parse<C: SnapshotCallbacks>(&self, callbacks: &mut C, network: NetworkId) -> Result<()> {
        let file = File::open(&self.file_path)
            .context(format!("Failed to open snapshot file: {}", self.file_path))?;

        let mut ctx = SnapshotContext {
            network: network.clone(),
        };

        let mut chunked_reader = ChunkedCborReader::new(file, self.chunk_size)?;

        // Phase 1: Parse metadata efficiently using larger buffer to handle protocol parameters
        // Read initial portion for metadata parsing (512MB to handle large protocol parameters)
        let metadata_size = 512 * 1024 * 1024; // 512MB for metadata parsing (increased for PParams)
        let actual_metadata_size = metadata_size.min(chunked_reader.file_size as usize);

        // Read metadata portion
        let metadata_buffer = {
            let mut buffer = vec![0u8; actual_metadata_size];
            chunked_reader.file.seek(SeekFrom::Start(0))?;
            chunked_reader.file.read_exact(&mut buffer)?;
            buffer
        };

        // Parse metadata using decoder - scope it to prevent accidental reuse
        let (
            epoch,
            blocks_previous_epoch,
            blocks_current_epoch,
            treasury,
            reserves,
            dreps,
            pools,
            accounts,
            utxo_file_position,
            instant_rewards_result,
        ) = {
            let mut decoder = Decoder::new(&metadata_buffer);

            // Navigate to NewEpochState root array
            let new_epoch_state_len = decoder
                .array()
                .context("Failed to parse NewEpochState root array")?
                .ok_or_else(|| anyhow!("NewEpochState must be a definite-length array"))?;

            if new_epoch_state_len < 4 {
                return Err(anyhow!(
                    "NewEpochState array too short: expected at least 4 elements, got {new_epoch_state_len}"
                ));
            }

            // Extract epoch number [0]
            let epoch = decoder.u64().context("Failed to parse epoch number")?;
            info!(epoch, snapshot = %self.file_path);

            // Parse blocks_previous_epoch [1] and blocks_current_epoch [2]
            let blocks_previous_epoch =
                Self::parse_blocks_with_epoch(&mut decoder, epoch.saturating_sub(1))
                    .context("Failed to parse blocks_previous_epoch")?;
            let blocks_current_epoch = Self::parse_blocks_with_epoch(&mut decoder, epoch)
                .context("Failed to parse blocks_current_epoch")?;

            // Navigate to EpochState [3]
            let epoch_state_len = decoder
                .array()
                .context("Failed to parse EpochState array")?
                .ok_or_else(|| anyhow!("EpochState must be a definite-length array"))?;

            if epoch_state_len < 3 {
                return Err(anyhow!(
                    "EpochState array too short: expected at least 3 elements, got {epoch_state_len}"
                ));
            }

            // Extract AccountState [3][0]: [treasury, reserves]
            // Note: In Conway era, AccountState is just [treasury, reserves], not a full map
            let account_state_len = decoder
                .array()
                .context("Failed to parse AccountState array")?
                .ok_or_else(|| anyhow!("AccountState must be a definite-length array"))?;

            if account_state_len < 2 {
                return Err(anyhow!(
                    "AccountState array too short: expected at least 2 elements, got {account_state_len}"
                ));
            }

            // Parse treasury and reserves (can be negative in CBOR, so decode as i64 first)
            let treasury_i64: i64 = decoder.decode().context("Failed to parse treasury")?;
            let reserves_i64: i64 = decoder.decode().context("Failed to parse reserves")?;
            let treasury =
                u64::try_from(treasury_i64).map_err(|_| anyhow!("treasury was negative"))?;
            let reserves =
                u64::try_from(reserves_i64).map_err(|_| anyhow!("reserves was negative"))?;

            // Skip any remaining AccountState fields
            for i in 2..account_state_len {
                decoder.skip().context(format!("Failed to skip AccountState[{i}]"))?;
            }

            // Note: We defer the on_metadata callback until after we parse deposits from UTxOState[1]

            // Navigate to LedgerState [3][1]
            let ledger_state_len = decoder
                .array()
                .context("Failed to parse LedgerState array")?
                .ok_or_else(|| anyhow!("LedgerState must be a definite-length array"))?;

            if ledger_state_len < 2 {
                return Err(anyhow!(
                    "LedgerState array too short: expected at least 2 elements, got {ledger_state_len}"
                ));
            }

            // Parse CertState [3][1][0] to extract DReps and pools
            // CertState (ARRAY) - DReps, pools, accounts
            //       - [0] VotingState - DReps at [3][1][0][0][0]
            //       - [1] PoolState - pools at [3][1][0][1][0]
            //       - [2] DelegationState - accounts at [3][1][0][2][0][0]
            // CertState = [VState, PState, DState]
            let cert_state_len = decoder
                .array()
                .context("Failed to parse CertState array")?
                .ok_or_else(|| anyhow!("CertState must be a definite-length array"))?;

            if cert_state_len < 3 {
                return Err(anyhow!(
                    "CertState array too short: expected at least 3 elements, got {cert_state_len}"
                ));
            }

            // Parse VState [3][1][0][0] for DReps, which also skips committee_state and dormant_epoch.
            // TODO: We may need to return to these later if we implement committee tracking.
            let dreps =
                Self::parse_vstate(&mut decoder).context("Failed to parse VState for DReps")?;

            // Parse PState [3][1][0][1] for pools. Include full error chain here because some
            // callers stringify the error, which otherwise only keeps the top-level context.
            let pools = Self::parse_pstate(&mut decoder, &mut ctx).map_err(|error| {
                anyhow!(
                    "Failed to parse PState for pools at byte {}: {error:#}",
                    decoder.position()
                )
            })?;

            // Parse DState [3][1][0][2] for accounts/delegations
            // DState is an array: [unified_rewards, fut_gen_deleg, gen_deleg, instant_rewards]
            let dstate_len = decoder.array().context("Failed to parse DState array")?;

            if let Some(len) = dstate_len {
                if len < 4 {
                    return Err(anyhow!(
                        "DState array too short: expected at least 4 elements, got {len}"
                    ));
                }
            }

            let accounts_map =
                Self::parse_dstate_accounts_map(&mut decoder, &mut ctx, "DState[0] accounts")?;

            // Epoch State / Ledger State / Cert State / Delegation state / dsFutureGenDelegs
            decoder.skip().context("Failed to skip DState[1] future genesis delegations")?;

            // Epoch State / Ledger State / Cert State / Delegation state / dsGenDelegs
            decoder.skip().context("Failed to skip DState[2] genesis delegations")?;

            // Epoch State / Ledger State / Cert State / Delegation state / dsIRewards
            // Parse instant rewards (MIRs) and combine with regular rewards
            // Structure: [ir_reserves, ir_treasury, ir_delta_reserves, ir_delta_treasury]
            let instant_rewards_result = Self::parse_instant_rewards(&mut decoder)
                .context("Failed to parse instant rewards")?;

            if let Some(len) = dstate_len {
                for i in 4..len {
                    decoder.skip().context(format!("Failed to skip DState[{i}]"))?;
                }
            }

            // Convert to AccountState for API, combining regular rewards with instant rewards
            let accounts: Vec<AccountState> = accounts_map
                .into_iter()
                .map(|(credential, account)| {
                    // Convert StakeCredential to stake address representation
                    let stake_address = StakeAddress::new(credential.clone(), network.clone());

                    // Add instant rewards (MIRs) if any
                    let mir_rewards =
                        instant_rewards_result.rewards.get(&credential).copied().unwrap_or(0);
                    let rewards = account.rewards + mir_rewards;

                    AccountState {
                        stake_address,
                        address_state: StakeAddressState {
                            registered: true, // Accounts in DState are registered by definition
                            utxo_value: 0,    // Will be populated from UTXO parsing
                            rewards,
                            delegated_spo: account.delegated_spo,
                            delegated_drep: account.delegated_drep,
                        },
                    }
                })
                .collect();

            // Navigate to UTxOState [3][1][1]
            let utxo_state_len = decoder
                .array()
                .context("Failed to parse UTxOState array")?
                .ok_or_else(|| anyhow!("UTxOState must be a definite-length array"))?;

            if utxo_state_len < 1 {
                return Err(anyhow!(
                    "UTxOState array too short: expected at least 1 element, got {utxo_state_len}"
                ));
            }

            // Record the position before UTXO streaming - this is where UTXOs start in the file
            let utxo_file_position = decoder.position() as u64;

            // Return all the parsed metadata values
            (
                epoch,
                blocks_previous_epoch,
                blocks_current_epoch,
                treasury,
                reserves,
                dreps,
                pools,
                accounts,
                utxo_file_position,
                instant_rewards_result,
            )
        }; // decoder goes out of scope here

        let snapshot_path = Path::new(&self.file_path);
        let utxo_file_path = self.find_utxo_sidecar_path().ok_or_else(|| {
            anyhow!(
                "Expected split snapshot sidecar UTxO file for snapshot {}",
                snapshot_path.display()
            )
        })?;

        // Continue remainder parsing from the snapshot file.
        // New snapshot format expects an empty UTxO placeholder map in NES.
        let mut snapshot_file = File::open(&self.file_path).context(format!(
            "Failed to open snapshot file for remainder parsing: {}",
            self.file_path
        ))?;
        let utxo_placeholder_bytes =
            Self::parse_empty_utxo_placeholder_bytes(&mut snapshot_file, utxo_file_position)?;

        let mut utxo_file = File::open(&utxo_file_path).context(format!(
            "Failed to open UTXO source file: {}",
            utxo_file_path.display()
        ))?;

        let position_after_utxos = utxo_file_position + utxo_placeholder_bytes;
        let snapshot_file_size = snapshot_file.metadata()?.len();
        let nes_remainder_bytes = snapshot_file_size.saturating_sub(position_after_utxos);
        let utxo_sidecar_total_bytes = utxo_file.metadata()?.len();
        let total_progress_bytes =
            utxo_sidecar_total_bytes.saturating_add(nes_remainder_bytes).max(1);

        info!(snapshot = %snapshot_path.display(), utxo_sidecar = %utxo_file_path.display());
        info!(
            phase = "start",
            progress_pct = 0.0,
            progress_bar = %Self::progress_bar(0.0),
            loaded_mb = 0.0,
            total_mb = Self::bytes_to_mb(total_progress_bytes)
        );

        // TRUE STREAMING: Process UTXOs one by one with minimal memory usage
        utxo_file.seek(SeekFrom::Start(0))?;
        let (utxo_count, bytes_consumed_from_file, stake_utxo_values) =
            Self::stream_utxos(&mut utxo_file, callbacks, total_progress_bytes, 0)
                .context("Failed to stream UTXOs with true streaming")?;

        snapshot_file.seek(SeekFrom::Start(position_after_utxos))?;

        // ========================================================================
        // HYBRID APPROACH: MEMORY-BASED PARSING OF REMAINDER
        // ========================================================================
        // After extensive analysis, the remaining snapshot data (deposits, fees,
        // protocol parameters, and mark/set snapshots) can be efficiently
        // parsed by reading the entire remainder of the file into memory (~500MB)
        // rather than streaming. This is much smaller than the full 2.5GB file.
        //
        // The CBOR structure from this point:
        // UTxOState[1] = deposits
        // UTxOState[2] = fees
        // UTxOState[3] = gov_state
        // UTxOState[4] = donations
        // EpochState[2] = PParams (100-300MB)
        // EpochState[3] = PParamsPrev (100-300MB)
        // EpochState[4] = SnapShots (100+ MB stake distribution)
        //
        // This hybrid approach allows us to:
        // 1. Continue using efficient UTXO streaming (11M UTXOs in 5s)
        // 2. Parse remaining sections using snapshot.rs functions
        // 3. Access mark/set snapshots that were previously unreachable
        // ========================================================================

        // Calculate remaining file size from current position
        let remaining_bytes = nes_remainder_bytes;

        // Read the entire remainder of the file into memory
        let mut remainder_buffer = Vec::with_capacity(remaining_bytes as usize);
        snapshot_file.read_to_end(&mut remainder_buffer)?;
        let loaded_bytes = utxo_sidecar_total_bytes.saturating_add(remainder_buffer.len() as u64);
        let progress_pct =
            (loaded_bytes as f64 / total_progress_bytes as f64 * 100.0).clamp(0.0, 100.0);
        let progress_pct = (progress_pct * 10.0).round() / 10.0;
        info!(
            phase = "nes_remainder_loaded",
            progress_pct,
            progress_bar = %Self::progress_bar(progress_pct),
            loaded_mb = Self::bytes_to_mb(loaded_bytes),
            total_mb = Self::bytes_to_mb(total_progress_bytes)
        );

        let remainder_mb = remainder_buffer.len() as f64 / 1024.0 / 1024.0;

        // Create decoder for the remainder buffer
        let mut remainder_decoder = Decoder::new(&remainder_buffer);

        // Parse remaining UTxOState elements: deposits, fees, gov_state, donations
        // UTxOState = [utxos (already consumed), deposits, fees, gov_state, donations]

        // Parse deposits (UTxOState[1])
        let deposits = remainder_decoder.decode::<u64>().unwrap_or(0);

        // Parse fees (UTxOState[2]) - cumulative fees in UTxO state
        // Note: us_fees contains fees from both current AND previous epoch. We subtract
        // fee_ss (previous epoch's fees from snapshots) later to get current epoch only.
        let us_fees = remainder_decoder.decode::<u64>().unwrap_or(0);

        // Parse governance state using the governance module
        // gov_state = [proposals, committee, constitution, current_pparams, previous_pparams, future_pparams, drep_pulsing_state]
        let governance_state = super::governance::parse_gov_state(&mut remainder_decoder, epoch)
            .context("Failed to parse governance state")?;

        let governance_proposals_count = governance_state.proposals.len();
        let governance_votes_count = governance_state.votes.len();

        // Emit governance protocol parameters callback
        callbacks.on_gs_protocol_parameters(
            epoch,
            governance_state.previous_reward_params.clone(),
            governance_state.current_reward_params.clone(),
            governance_state.protocol_params.clone(),
        )?;

        // Extract governance deposit info before passing state to callback
        // Each enacted or expired governance action gets its deposit refunded
        // Pending and enacted proposals have deposits that are included in us_deposited but should be excluded
        let pending_proposal_deposits: u64 =
            governance_state.proposals.iter().map(|p| p.proposal_procedure.deposit).sum();
        let enacted_proposal_deposits: u64 =
            governance_state.enacted_actions.iter().map(|p| p.proposal_procedure.deposit).sum();

        // Subtract pending and enacted governance proposal deposits from us_deposited
        // The snapshot's us_deposited includes these, but they shouldn't be in our deposits pot
        // Enacted proposals will be refunded at epoch boundary, but snapshot is taken before that
        let governance_deposits = pending_proposal_deposits + enacted_proposal_deposits;
        let deposits = deposits.saturating_sub(governance_deposits);

        // Extract pool deposit from protocol parameters before consuming governance_state
        let _stake_pool_deposit = match governance_state.protocol_params.pool_deposit {
            Some(deposit) => deposit,
            None => {
                return Err(anyhow::anyhow!(
                    "Stake pool deposit must exist in protocol params"
                ));
            }
        };

        // Emit governance state callback
        callbacks.on_governance_state(governance_state)?;

        // Epoch State / Ledger State / UTxO State / utxosStakeDistr
        remainder_decoder.skip()?;

        // Epoch State / Ledger State / UTxO State / utxosDonation
        // Treasury donations accumulate during epoch and are added to treasury at epoch boundary
        let donations: u64 = remainder_decoder.decode().unwrap_or(0);

        // Parse mark/set snapshots (EpochState[2])
        let snapshots_result =
            Self::parse_snapshots_with_hybrid_approach(&mut remainder_decoder, &mut ctx, epoch);

        // Skip non_myopic (EpochState[3])
        remainder_decoder.skip()?;

        // Exit EpochState, now at NewEpochState level
        // Parse pulsing_rew_update (NewEpochState[4]) to get reward snapshot and pot deltas
        let pulsing_result = Self::parse_pulsing_reward_update(&mut remainder_decoder)?;

        // Convert block production data to HashMap<PoolId, usize> for snapshot processing
        let blocks_prev_map: std::collections::HashMap<PoolId, usize> =
            blocks_previous_epoch.iter().map(|p| (p.pool_id, p.block_count as usize)).collect();
        let blocks_curr_map: std::collections::HashMap<PoolId, usize> =
            blocks_current_epoch.iter().map(|p| (p.pool_id, p.block_count as usize)).collect();

        let raw_snapshots = snapshots_result.context("Failed to parse mark/set snapshots")?;
        let fees_prev_epoch = raw_snapshots.fees;
        let bootstrap_snapshots = raw_snapshots.into_snapshots_container(
            epoch,
            &blocks_prev_map,
            &blocks_curr_map,
            network.clone(),
        );
        callbacks.on_snapshots(bootstrap_snapshots.clone())?;

        // Build pool registrations list for AccountsBootstrapMessage
        let pool_registrations: Vec<PoolRegistration> = pools.pools.values().cloned().collect();
        let retiring_pools: Vec<PoolId> = pools
            .retiring
            .iter()
            .filter(|(_, retiring_epoch)| **retiring_epoch == epoch)
            .map(|(pool_id, _)| *pool_id)
            .collect();

        // Convert DRepInfo to (credential, deposit) tuples
        let drep_deposits: Vec<(DRepCredential, u64)> =
            dreps.iter().map(|(cred, record)| (cred.clone(), record.deposit)).collect();

        // Calculate total DRep deposits
        let total_drep_deposits: u64 = drep_deposits.iter().map(|(_, d)| d).sum();

        // Subtract DRep deposits from us_deposited
        // The snapshot's us_deposited includes DRep deposits, but they shouldn't be in our deposits pot
        let deposits = deposits.saturating_sub(total_drep_deposits);

        // Merge UTXO values and reward updates into accounts.
        // Keep unregistered accounts if they carry UTxO or reward balances so
        // withdrawals and stake deltas can resolve those credentials after bootstrap.
        let registered_credentials: std::collections::HashSet<_> =
            accounts.iter().map(|a| a.stake_address.credential.clone()).collect();

        let mut accounts_by_credential: std::collections::HashMap<StakeCredential, AccountState> =
            accounts
                .into_iter()
                .map(|account| (account.stake_address.credential.clone(), account))
                .collect();

        for (credential, &utxo_value) in &stake_utxo_values {
            let entry =
                accounts_by_credential.entry(credential.clone()).or_insert_with(|| AccountState {
                    stake_address: StakeAddress::new(credential.clone(), network.clone()),
                    address_state: StakeAddressState {
                        registered: false,
                        utxo_value: 0,
                        rewards: 0,
                        delegated_spo: None,
                        delegated_drep: None,
                    },
                });
            entry.address_state.utxo_value = utxo_value;
        }

        for (credential, &pulsing_reward) in &pulsing_result.rewards {
            let entry =
                accounts_by_credential.entry(credential.clone()).or_insert_with(|| AccountState {
                    stake_address: StakeAddress::new(credential.clone(), network.clone()),
                    address_state: StakeAddressState {
                        registered: false,
                        utxo_value: 0,
                        rewards: 0,
                        delegated_spo: None,
                        delegated_drep: None,
                    },
                });
            entry.address_state.rewards =
                entry.address_state.rewards.saturating_add(pulsing_reward);
        }

        // Registered DState accounts already include MIR rewards from instant_rewards_result.
        // Only add MIR rewards here for credentials absent from DState.
        for (credential, &mir_reward) in &instant_rewards_result.rewards {
            if registered_credentials.contains(credential) {
                continue;
            }

            let entry =
                accounts_by_credential.entry(credential.clone()).or_insert_with(|| AccountState {
                    stake_address: StakeAddress::new(credential.clone(), network.clone()),
                    address_state: StakeAddressState {
                        registered: false,
                        utxo_value: 0,
                        rewards: 0,
                        delegated_spo: None,
                        delegated_drep: None,
                    },
                });
            entry.address_state.rewards = entry.address_state.rewards.saturating_add(mir_reward);
        }

        let pulsing_rewards_total: u64 = pulsing_result.rewards.values().sum();
        let unregistered_accounts =
            accounts_by_credential.values().filter(|a| !a.address_state.registered).count();
        let reward_only_accounts = accounts_by_credential
            .values()
            .filter(|a| !a.address_state.registered && a.address_state.utxo_value == 0)
            .count();

        let accounts_with_utxo_values: Vec<AccountState> =
            accounts_by_credential.into_values().collect();

        // Calculate summary statistics
        let total_utxo_value: u64 = stake_utxo_values.values().sum();
        let total_rewards: u64 =
            accounts_with_utxo_values.iter().map(|a| a.address_state.rewards).sum();
        let delegated_count = accounts_with_utxo_values
            .iter()
            .filter(|a| a.address_state.delegated_spo.is_some())
            .count();

        // Calculate governance proposal deposit refunds for enacted proposals
        // We use enacted_proposal_deposits which was calculated earlier by summing each proposal's
        // actual deposit field. For expired proposals, we only have IDs (no deposit amounts),
        // so we cannot include them here. This is acceptable because expired proposal refunds
        // should be tracked when proposals are processed, not at epoch boundary.
        let gov_deposit_refunds = enacted_proposal_deposits;

        let total_deposit_refunds = gov_deposit_refunds;

        // Combine pot deltas from pulsing_rew_update and instant_rewards,
        // plus governance proposal deposit refunds and treasury donations.
        //
        // Use checked arithmetic to detect overflow
        let donations_i64 = i64::try_from(donations).expect("donations exceeds i64::MAX");
        let total_deposit_refunds_i64 =
            i64::try_from(total_deposit_refunds).expect("total_deposit_refunds exceeds i64::MAX");

        let delta_treasury = pulsing_result
            .delta_treasury
            .checked_add(instant_rewards_result.delta_treasury)
            .and_then(|v| v.checked_add(donations_i64))
            .expect("overflow computing delta_treasury");

        let delta_reserves = pulsing_result
            .delta_reserves
            .checked_add(instant_rewards_result.delta_reserves)
            .expect("overflow computing delta_reserves");

        let delta_deposits = -total_deposit_refunds_i64;

        let pot_deltas = crate::messages::BootstrapPotDeltas {
            delta_treasury,
            delta_reserves,
            delta_deposits,
        };

        // Build the accounts bootstrap data
        let pools_total = pool_registrations.len();
        let dreps_total = drep_deposits.len();
        let accounts_total = accounts_with_utxo_values.len();
        info!(
            epoch,
            utxos = utxo_count,
            utxo_sidecar_bytes = bytes_consumed_from_file,
            pools = pools_total,
            dreps = dreps_total,
            accounts = accounts_total,
            delegated_accounts = delegated_count,
            utxo_ada = total_utxo_value / 1_000_000,
            rewards_ada = total_rewards / 1_000_000,
            pulsing_rewards_ada = pulsing_rewards_total / 1_000_000,
            governance_proposals = governance_proposals_count,
            governance_votes = governance_votes_count,
            remainder_mb = remainder_mb,
            unregistered_accounts,
            reward_only_accounts
        );

        let accounts_bootstrap_data = AccountsBootstrapData {
            epoch,
            accounts: accounts_with_utxo_values,
            pools: pool_registrations,
            retiring_pools,
            dreps: drep_deposits,
            pots: Pots {
                reserves,
                treasury,
                deposits,
            },
            pot_deltas,
            snapshots: bootstrap_snapshots,
        };

        // Emit bulk callbacks
        callbacks.on_pools(pools)?;
        callbacks.on_dreps(epoch, dreps)?;
        callbacks.on_accounts(accounts_bootstrap_data)?;
        callbacks.on_proposals(Vec::new())?; // TODO: Parse from GovState

        // Calculate current epoch fees: us_fees contains cumulative fees, subtract previous epoch's
        let total_fees_current = us_fees.saturating_sub(fees_prev_epoch);
        let epoch_bootstrap = EpochBootstrapData::new(
            epoch,
            &blocks_previous_epoch,
            &blocks_current_epoch,
            total_fees_current,
        );
        callbacks.on_epoch(epoch_bootstrap)?;

        let snapshot_metadata = SnapshotMetadata {
            epoch,
            pot_balances: Pots {
                reserves,
                treasury,
                deposits,
            },
            utxo_count: Some(utxo_count),
            blocks_previous_epoch,
            blocks_current_epoch,
        };
        callbacks.on_metadata(snapshot_metadata)?;

        info!(
            epoch,
            treasury_ada = treasury / 1_000_000,
            reserves_ada = reserves / 1_000_000,
            deposits_ada = deposits / 1_000_000,
            pending_deposits_ada = pending_proposal_deposits / 1_000_000,
            enacted_deposits_ada = enacted_proposal_deposits / 1_000_000,
            donations_ada = donations / 1_000_000
        );
        info!(
            phase = "complete",
            progress_pct = 100.0,
            progress_bar = %Self::progress_bar(100.0),
            loaded_mb = Self::bytes_to_mb(total_progress_bytes),
            total_mb = Self::bytes_to_mb(total_progress_bytes)
        );

        callbacks.on_complete()?;

        Ok(())
    }

    /// STREAMING: Process UTXOs with chunked buffering and incremental parsing
    ///
    /// Returns a tuple of:
    /// - UTXO count
    /// - Bytes consumed from file
    /// - Map of stake credentials to accumulated UTXO values
    fn stream_utxos<C: UtxoCallback>(
        file: &mut File,
        callbacks: &mut C,
        total_progress_bytes: u64,
        progress_offset_bytes: u64,
    ) -> Result<(u64, u64, HashMap<StakeCredential, u64>)> {
        // OPTIMIZED: Balance between memory usage and performance
        // Based on experiment: avg=194 bytes, max=22KB per entry

        const READ_CHUNK_SIZE: usize = 16 * 1024 * 1024; // 16MB read chunks for I/O efficiency
        const PARSE_BUFFER_SIZE: usize = 64 * 1024 * 1024; // 64MB parse buffer (vs 2.1GB)
        const MAX_ENTRY_SIZE: usize = 32 * 1024; // 32KB safety margin

        let mut buffer = Vec::with_capacity(PARSE_BUFFER_SIZE);
        let mut utxo_count = 0u64;
        let mut total_bytes_processed = 0usize;
        let mut total_bytes_read_from_file = 0u64;

        // Accumulate UTXO values by stake credential for SPDD generation
        let mut stake_values: HashMap<StakeCredential, u64> = HashMap::new();

        // Read a larger initial buffer for better performance
        let mut chunk = vec![0u8; READ_CHUNK_SIZE];
        let initial_read = file.read(&mut chunk)?;
        chunk.truncate(initial_read);
        buffer.extend_from_slice(&chunk);
        total_bytes_read_from_file += initial_read as u64;

        // Parse map header first
        let mut decoder = Decoder::new(&buffer);
        // Use u64::MAX for indefinite-length CBOR maps
        let map_len = (decoder.map()?).unwrap_or(u64::MAX);

        let header_consumed = decoder.position();
        buffer.drain(0..header_consumed);
        total_bytes_processed += header_consumed;

        let mut entries_processed = 0u64;
        let mut max_single_entry_size = 0usize;

        // Process entries incrementally
        while entries_processed < map_len {
            // Ensure we have enough data in buffer - use larger reads for efficiency
            while buffer.len() < MAX_ENTRY_SIZE && entries_processed < map_len {
                let mut chunk = vec![0u8; READ_CHUNK_SIZE];
                let bytes_read = file.read(&mut chunk)?;
                if bytes_read == 0 {
                    break; // EOF
                }
                chunk.truncate(bytes_read);
                buffer.extend_from_slice(&chunk);
                total_bytes_read_from_file += bytes_read as u64;
            }

            // Batch process multiple UTXOs when buffer is large enough
            let mut batch_processed = 0;
            let mut entry_decoder = Decoder::new(&buffer);
            let mut last_good_position = 0;

            // Process as many UTXOs as possible from current buffer
            loop {
                let position_before = entry_decoder.position();

                // Check for indefinite map break
                if map_len == u64::MAX && matches!(entry_decoder.datatype(), Ok(Type::Break)) {
                    entries_processed = map_len; // Exit outer loop
                    break;
                }

                // Try to parse one UTXO entry
                match Self::parse_single_utxo(&mut entry_decoder) {
                    Ok(utxo) => {
                        let bytes_consumed = entry_decoder.position();
                        let entry_size = bytes_consumed - position_before;
                        max_single_entry_size = max_single_entry_size.max(entry_size);

                        // Track total UTXO value
                        let coin = utxo.coin();

                        // Accumulate UTXO value by stake credential for SPDD
                        if let Some(stake_cred) = utxo.extract_stake_credential() {
                            *stake_values.entry(stake_cred).or_insert(0) += coin;
                        }

                        // Emit the UTXO
                        callbacks.on_utxo(utxo)?;
                        utxo_count += 1;
                        entries_processed += 1;
                        batch_processed += 1;
                        last_good_position = bytes_consumed;

                        // Progress reporting - less frequent for better performance
                        if utxo_count.is_multiple_of(1000000) {
                            let global_bytes =
                                progress_offset_bytes.saturating_add(total_bytes_read_from_file);
                            let progress_pct = (global_bytes as f64 / total_progress_bytes as f64
                                * 100.0)
                                .clamp(0.0, 100.0);
                            let progress_pct = (progress_pct * 10.0).round() / 10.0;
                            info!(
                                phase = "utxo_stream",
                                utxos_streamed = utxo_count,
                                progress_pct,
                                progress_bar = %Self::progress_bar(progress_pct),
                                loaded_mb = Self::bytes_to_mb(global_bytes),
                                total_mb = Self::bytes_to_mb(total_progress_bytes)
                            );
                        }

                        // Continue processing if we have more data and haven't hit limits
                        if entries_processed >= map_len
                            || entry_decoder.position()
                                >= buffer.len().saturating_sub(MAX_ENTRY_SIZE)
                        {
                            break; // Exit batch processing loop
                        }
                    }
                    Err(_) => {
                        // Couldn't parse - might need more data or hit an error
                        if entry_decoder.position() == position_before {
                            // No progress made - need more data
                            break; // Exit batch processing loop to read more data
                        } else {
                            // Made some progress but failed - skip this entry
                            last_good_position = entry_decoder.position();
                            entries_processed += 1;

                            if entries_processed >= map_len {
                                break;
                            }
                        }
                    }
                }
            }

            // Remove all processed data from buffer
            if last_good_position > 0 {
                buffer.drain(0..last_good_position);
                total_bytes_processed += last_good_position;
            }

            // If we didn't process any entries and buffer is small, read more data
            if batch_processed == 0 && entries_processed < map_len && buffer.len() < MAX_ENTRY_SIZE
            {
                if buffer.len() >= MAX_ENTRY_SIZE {
                    return Err(anyhow!(
                        "Failed to parse UTXO entry after reading {} bytes",
                        buffer.len()
                    ));
                }
                continue; // Go back to read more data
            }

            // Safety check - prevent buffer from growing beyond reasonable limits
            if buffer.len() > PARSE_BUFFER_SIZE * 2 {
                return Err(anyhow!("Buffer grew too large: {} bytes", buffer.len()));
            }
        }

        info!(
            utxos_processed = utxo_count,
            streamed_mb = total_bytes_processed as f64 / 1024.0 / 1024.0,
            peak_buffer_mb = PARSE_BUFFER_SIZE / 1024 / 1024,
            largest_entry_bytes = max_single_entry_size
        );

        // After successfully parsing all UTXOs, we need to consume the break token
        // that ends the indefinite-length UTXO map if present
        if !buffer.is_empty() {
            let mut decoder = Decoder::new(&buffer);
            match decoder.datatype() {
                Ok(Type::Break) => {
                    decoder.skip()?; // Consume the break that ends the UTXO map

                    // Update our tracking to account for the consumed break token
                    let break_bytes_consumed = decoder.position();
                    buffer.drain(0..break_bytes_consumed);
                }
                Ok(_) => {
                    // No break token, this is a definite-length map - continue normal parsing
                }
                Err(e) => {
                    error!(error = %e, "Unable to inspect trailing UTxO token");
                }
            }
        }

        // Calculate how many bytes we actually consumed from the file
        // This is the total bytes processed minus any remaining buffer content
        let bytes_consumed_from_file = total_bytes_read_from_file - buffer.len() as u64;

        Ok((utxo_count, bytes_consumed_from_file, stake_values))
    }

    /// Parse a single block production entry from a map (producer pool ID -> block count)
    /// The CBOR structure maps pool IDs to block counts (not individual blocks)
    fn parse_single_block_production_entry(
        decoder: &mut Decoder,
        epoch: u64,
    ) -> Result<PoolBlockProduction> {
        // Parse the pool ID (key) - stored as bytes (28 bytes for pool ID)
        let pool_id_bytes = decoder.bytes().context("Failed to parse pool ID bytes")?;

        // Parse the block count (value) - how many blocks this pool produced
        let block_count = decoder.u8().context("Failed to parse block count")?;

        // Convert pool ID bytes to hex string
        let pool_id =
            hex::encode(pool_id_bytes).parse::<PoolId>().context("Failed to parse pool ID")?;

        Ok(PoolBlockProduction {
            pool_id,
            block_count,
            epoch,
        })
    }

    /// Parse blocks from the CBOR decoder (either previous or current epoch blocks)
    fn parse_blocks_with_epoch(
        decoder: &mut Decoder,
        epoch: u64,
    ) -> Result<Vec<PoolBlockProduction>> {
        // Blocks are typically encoded as an array or map
        match decoder.datatype().context("Failed to read blocks datatype")? {
            Type::Array | Type::ArrayIndef => {
                let len = decoder.array().context("Failed to parse blocks array")?;
                let blocks = Vec::new();

                // Handle definite-length array
                if let Some(block_count) = len {
                    for _i in 0..block_count {
                        // Each block might be encoded as an array or map
                        // For now, skip individual blocks since we don't know the exact format
                        // This is a placeholder - the actual format needs to be determined from real data
                        decoder.skip().context("Failed to skip block entry")?;
                    }
                } else {
                    // Indefinite-length array
                    loop {
                        match decoder.datatype()? {
                            Type::Break => {
                                decoder.skip()?;
                                break;
                            }
                            _ => {
                                decoder.skip().context("Failed to skip block entry")?;
                            }
                        }
                    }
                }

                Ok(blocks)
            }
            Type::Map | Type::MapIndef => {
                // Blocks are stored as a map: PoolID -> block_count (u8)
                let len = decoder.map().context("Failed to parse blocks map")?;

                let mut block_productions = Vec::new();

                // Parse map content
                if let Some(entry_count) = len {
                    for _i in 0..entry_count {
                        // Parse pool ID -> block count
                        match Self::parse_single_block_production_entry(decoder, epoch) {
                            Ok(production) => {
                                block_productions.push(production);
                            }
                            Err(_) => {
                                // Skip failed entries
                                decoder.skip().context("Failed to skip map key")?;
                                decoder.skip().context("Failed to skip map value")?;
                            }
                        }
                    }
                } else {
                    // Indefinite map
                    loop {
                        match decoder.datatype()? {
                            Type::Break => {
                                decoder.skip()?;
                                break;
                            }
                            _ => {
                                match Self::parse_single_block_production_entry(decoder, epoch) {
                                    Ok(production) => {
                                        block_productions.push(production);
                                    }
                                    Err(_) => {
                                        // Skip failed entries
                                        decoder.skip().context("Failed to skip map key")?;
                                        decoder.skip().context("Failed to skip map value")?;
                                    }
                                }
                            }
                        }
                    }
                }

                Ok(block_productions)
            }
            simple_type => {
                // If it's a simple value or other type, skip it for now
                // Try to get more details about simple types
                match simple_type {
                    Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
                        decoder.u64().context("Failed to read block integer value")?;
                    }
                    Type::Bytes => {
                        decoder.bytes().context("Failed to read block bytes")?;
                    }
                    Type::String => {
                        decoder.str().context("Failed to read block text")?;
                    }
                    Type::Null => {
                        decoder.skip()?;
                    }
                    _ => {
                        decoder.skip().context("Failed to skip blocks value")?;
                    }
                }

                Ok(Vec::new())
            }
        }
    }

    /// Parse instant rewards (MIRs) from DState
    ///
    /// instantaneous_rewards = [
    ///   ir_reserves : { * credential_staking => coin },
    ///   ir_treasury : { * credential_staking => coin },
    ///   ir_delta_reserves : delta_coin,
    ///   ir_delta_treasury : delta_coin,
    /// ]
    ///
    /// Returns combined rewards map and pot deltas from MIR transfers
    fn parse_instant_rewards(decoder: &mut Decoder) -> Result<InstantRewardsResult> {
        let ir_len = decoder
            .array()
            .context("Failed to parse instant_rewards array")?
            .ok_or_else(|| anyhow!("instant_rewards must be a definite-length array"))?;

        if ir_len < 4 {
            return Err(anyhow!(
                "instant_rewards array too short: expected 4 elements, got {ir_len}"
            ));
        }

        // Parse ir_reserves and ir_treasury: { * credential_staking => coin }
        let ir_reserves: HashMap<StakeCredential, u64> = decoder.decode()?;
        let ir_treasury: HashMap<StakeCredential, u64> = decoder.decode()?;

        // Parse ir_delta_reserves and ir_delta_treasury
        let delta_reserves: i64 = decoder.decode()?;
        let delta_treasury: i64 = decoder.decode()?;

        // Combine rewards from both sources
        let mut combined = ir_reserves;
        for (credential, amount) in ir_treasury {
            *combined.entry(credential).or_insert(0) += amount;
        }

        Ok(InstantRewardsResult {
            rewards: combined,
            delta_treasury,
            delta_reserves,
        })
    }

    /// Parse pulsing_rew_update to extract reward information and pot deltas.
    ///
    /// The pulsing_rew_update is wrapped in a StrictMaybe, so we first check if it's
    /// present before parsing using the PulsingRewardUpdate type.
    ///
    /// Returns rewards map and pot deltas (treasury/reserves changes to apply).
    fn parse_pulsing_reward_update(decoder: &mut Decoder) -> Result<PulsingRewardResult> {
        // Check if strict_maybe is empty or has content
        match decoder.array()? {
            Some(0) => return Ok(PulsingRewardResult::default()),
            Some(1) => {}
            Some(other) => {
                return Err(anyhow!(
                    "Invalid strict_maybe length for pulsing_rew_update: {}",
                    other
                ));
            }
            None => {
                return Err(anyhow!("pulsing_rew_update must be definite-length array"));
            }
        };

        // Parse using the proper PulsingRewardUpdate type
        let pulsing_update: PulsingRewardUpdate =
            decoder.decode().context("Failed to decode PulsingRewardUpdate")?;

        // Extract rewards and pot deltas based on variant
        let result = match pulsing_update {
            PulsingRewardUpdate::Pulsing { snapshot } => {
                let rewards = snapshot
                    .leaders
                    .0
                    .iter()
                    .map(|(cred, rewards)| (cred.clone(), rewards.iter().map(|r| r.amount).sum()))
                    .collect();
                PulsingRewardResult {
                    rewards,
                    delta_treasury: snapshot.delta_t1 as i64,
                    delta_reserves: -(snapshot.delta_r1 as i64), // Reserves decrease
                    delta_fees: 0, // Pulsing variant doesn't have delta_fees
                }
            }
            PulsingRewardUpdate::Complete { update } => {
                let rewards = update
                    .rewards
                    .0
                    .iter()
                    .map(|(cred, rewards)| (cred.clone(), rewards.iter().map(|r| r.amount).sum()))
                    .collect();
                PulsingRewardResult {
                    rewards,
                    delta_treasury: update.delta_treasury,
                    delta_reserves: -update.delta_reserves, // Negate because it's stored inverted
                    delta_fees: -update.delta_fees,         // Negate because it's stored inverted
                }
            }
        };

        Ok(result)
    }

    /// Parse a single UTXO entry from the streaming buffer
    fn parse_single_utxo(decoder: &mut Decoder) -> Result<UtxoEntry> {
        // Parse key: TransactionInput (array [tx_hash, output_index])
        decoder.array().context("Failed to parse TxIn array")?;
        let utxo: SnapshotUTxO = decoder.decode().context("Failed to parse UTxO")?;
        Ok(utxo.0)
    }

    /// VState = [dreps_map, committee_state, dormant_epoch]
    fn parse_vstate(decoder: &mut Decoder) -> Result<HashMap<DRepCredential, DRepRecord>> {
        // Parse VState array
        let vstate_len = decoder
            .array()
            .context("Failed to parse VState array")?
            .ok_or_else(|| anyhow!("VState must be a definite-length array"))?;

        if vstate_len < 1 {
            return Err(anyhow!(
                "VState array too short: expected at least 1 element, got {vstate_len}"
            ));
        }

        // Parse DReps map [0]: StakeCredential -> DRepState
        // Using minicbor's Decode trait - much simpler than manual parsing!
        let dreps_map: BTreeMap<StakeCredential, DRepState> = decoder.decode()?;
        let dreps: HashMap<DRepCredential, DRepRecord> = dreps_map
            .into_iter()
            .map(|(cred, state)| {
                let anchor = match state.anchor {
                    StrictMaybe::Just(a) => Some(crate::Anchor {
                        url: a.url,
                        data_hash: a.content_hash.to_vec(),
                    }),
                    StrictMaybe::Nothing => None,
                };

                let record = DRepRecord {
                    deposit: state.deposit,
                    anchor,
                };

                (cred, record)
            })
            .collect();

        // Skip committee_state [1] and dormant_epoch [2] if present
        for i in 1..vstate_len {
            decoder.skip().context(format!("Failed to skip VState[{i}]"))?;
        }

        Ok(dreps)
    }

    fn decode_account_state_map(
        decoder: &mut Decoder,
        ctx: &mut SnapshotContext,
        map_name: &str,
    ) -> Result<BTreeMap<StakeCredential, NormalizedAccount>> {
        let decoded: BTreeMap<StakeCredential, SnapshotAccountValue> = decoder
            .decode_with(ctx)
            .map_err(|err| {
                error!(
                    map = map_name,
                    byte_offset = decoder.position(),
                    error = %err
                );
                err
            })
            .context(format!("Failed to decode {map_name}"))?;

        Ok(decoded
            .into_iter()
            .map(|(credential, value)| (credential, value.to_normalized()))
            .collect())
    }

    fn parse_dstate_accounts_map(
        decoder: &mut Decoder,
        ctx: &mut SnapshotContext,
        map_name: &str,
    ) -> Result<BTreeMap<StakeCredential, NormalizedAccount>> {
        match decoder
            .datatype()
            .with_context(|| format!("Failed to inspect datatype for {map_name}"))?
        {
            Type::Map | Type::MapIndef => Self::decode_account_state_map(decoder, ctx, map_name),
            other => Err(anyhow!(
                "Unexpected {map_name} datatype: expected map, got {other:?}"
            )),
        }
    }

    fn parse_pool_params_map(
        decoder: &mut Decoder,
        ctx: &mut SnapshotContext,
        map_name: &str,
    ) -> Result<BTreeMap<PoolId, PoolRegistration>> {
        if matches!(decoder.datatype()?, Type::Tag) {
            decoder.tag()?;
        }

        let map: BTreeMap<PoolId, SnapshotPoolRegistrationWithoutOperator> = decoder
            .decode_with(ctx)
            .map_err(|err| {
                error!(
                    map = map_name,
                    byte_offset = decoder.position(),
                    error = %err
                );
                err
            })
            .context(format!("Failed to decode {map_name}"))?;

        Ok(map
            .into_iter()
            .map(|(pool_id, pool)| {
                let mut pool_registration = pool.0;
                pool_registration.operator = pool_id;
                (pool_id, pool_registration)
            })
            .collect())
    }

    fn parse_retiring_map(
        decoder: &mut Decoder,
        map_name: &str,
    ) -> Result<BTreeMap<PoolId, Epoch>> {
        if matches!(decoder.datatype()?, Type::Tag) {
            decoder.tag()?;
        }

        let map: BTreeMap<PoolId, Epoch> = decoder
            .decode()
            .map_err(|err| {
                error!(
                    map = map_name,
                    byte_offset = decoder.position(),
                    error = %err
                );
                err
            })
            .context(format!("Failed to decode {map_name}"))?;

        Ok(map)
    }

    fn parse_pstate_new_layout(
        decoder: &mut Decoder,
        ctx: &mut SnapshotContext,
        pstate_len: u64,
    ) -> Result<SPOState> {
        if pstate_len < 4 {
            return Err(anyhow!(
                "New-era PState array too short: expected at least 4 elements, got {pstate_len}"
            ));
        }

        // [0] psVRFKeyHashes
        if matches!(decoder.datatype()?, Type::Tag) {
            decoder.tag()?;
        }
        decoder.skip().context("Failed to skip PState[0] VRF key hash map (new-era layout)")?;

        // [1] psStakePools
        let pools = Self::parse_pool_params_map(decoder, ctx, "PState[1] stake_pools")?;

        // [2] psFutureStakePoolParams
        let updates =
            Self::parse_pool_params_map(decoder, ctx, "PState[2] future_stake_pool_params")?;

        // [3] psRetiring
        let retiring = Self::parse_retiring_map(decoder, "PState[3] retiring")?;

        for i in 4..pstate_len {
            decoder.skip().context(format!("Failed to skip PState[{i}]"))?;
        }

        Ok(SPOState {
            pools,
            updates,
            retiring,
        })
    }

    /// Parse PState to extract stake pools.
    ///
    /// New-era shape:
    ///   [vrf_key_hashes_map, stake_pools_map, future_stake_pool_params_map, retiring_map]
    pub fn parse_pstate(decoder: &mut Decoder, ctx: &mut SnapshotContext) -> Result<SPOState> {
        let pstate_len = decoder
            .array()
            .context("Failed to parse PState array")?
            .ok_or_else(|| anyhow!("PState must be a definite-length array"))?;

        if pstate_len < 4 {
            return Err(anyhow!(
                "PState array too short: expected at least 4 elements, got {pstate_len}"
            ));
        }

        Self::parse_pstate_new_layout(decoder, ctx, pstate_len)
    }

    /// Parse snapshots using hybrid approach with memory-based parsing
    /// Uses snapshot.rs functions to parse mark and set snapshots from buffer
    /// We expect the following structure:
    /// Epoch State / Snapshots / Mark
    /// Epoch State / Snapshots / Set
    /// Epoch State / Snapshots / Go
    /// Epoch State / Snapshots / Fee
    fn parse_snapshots_with_hybrid_approach(
        decoder: &mut Decoder,
        ctx: &mut SnapshotContext,
        _epoch: u64,
    ) -> Result<RawSnapshotsContainer> {
        let snapshots_len = decoder
            .array()
            .context("Failed to parse SnapShots array")?
            .ok_or_else(|| anyhow!("SnapShots must be a definite-length array"))?;

        if snapshots_len != 4 {
            return Err(anyhow!(
                "SnapShots array must have exactly 4 elements (Mark, Set, Go, Fee), got {snapshots_len}"
            ));
        }

        // Parse Mark and Set snapshots
        let mark_snapshot =
            RawSnapshot::parse(decoder, ctx, "Mark").context("Failed to parse Mark snapshot")?;
        let set_snapshot =
            RawSnapshot::parse(decoder, ctx, "Set").context("Failed to parse Set snapshot")?;
        decoder.skip()?;
        let fees = decoder.decode::<u64>().context("Failed to parse fees from snapshots")?;

        Ok(RawSnapshotsContainer {
            mark: mark_snapshot,
            set: set_snapshot,
            fees,
        })
    }
}

// -----------------------------------------------------------------------------
// Helper: Simple callback handler for testing
// -----------------------------------------------------------------------------

/// Simple callback handler that collects all data in memory (for testing)
#[derive(Debug, Default)]
pub struct CollectingCallbacks {
    pub metadata: Option<SnapshotMetadata>,
    pub utxos: Vec<UtxoEntry>,
    pub pools: SPOState,
    pub accounts: Vec<AccountState>,
    pub dreps: HashMap<DRepCredential, DRepRecord>,
    pub proposals: Vec<GovernanceProposal>,
    pub epoch: EpochBootstrapData,
    pub snapshots: Option<RawSnapshotsContainer>,
    pub previous_reward_params: RewardParams,
    pub current_reward_params: RewardParams,
    pub protocol_parameters: ProtocolParamUpdate,
    pub governance_state: Option<super::governance::GovernanceState>,
}

impl UtxoCallback for CollectingCallbacks {
    fn on_utxo(&mut self, utxo: UtxoEntry) -> Result<()> {
        self.utxos.push(utxo);
        Ok(())
    }
}

impl EpochCallback for CollectingCallbacks {
    fn on_epoch(&mut self, data: EpochBootstrapData) -> Result<()> {
        self.epoch = data;
        Ok(())
    }
}

impl PoolCallback for CollectingCallbacks {
    fn on_pools(&mut self, pools: SPOState) -> Result<()> {
        self.pools = pools;
        Ok(())
    }
}

impl AccountsCallback for CollectingCallbacks {
    fn on_accounts(&mut self, data: AccountsBootstrapData) -> Result<()> {
        self.accounts = data.accounts;
        Ok(())
    }
}

impl DRepCallback for CollectingCallbacks {
    fn on_dreps(&mut self, _epoch: u64, dreps: HashMap<DRepCredential, DRepRecord>) -> Result<()> {
        self.dreps = dreps;
        Ok(())
    }
}

impl ProposalCallback for CollectingCallbacks {
    fn on_proposals(&mut self, proposals: Vec<GovernanceProposal>) -> Result<()> {
        self.proposals = proposals;
        Ok(())
    }
}

impl GovernanceProtocolParametersCallback for CollectingCallbacks {
    fn on_gs_protocol_parameters(
        &mut self,
        _epoch: u64,
        previous_reward_params: RewardParams,
        current_reward_params: RewardParams,
        params: ProtocolParamUpdate,
    ) -> Result<()> {
        // epoch is already stored in metadata
        self.previous_reward_params = previous_reward_params;
        self.current_reward_params = current_reward_params;
        self.protocol_parameters = params;
        Ok(())
    }
}

impl GovernanceStateCallback for CollectingCallbacks {
    fn on_governance_state(&mut self, state: super::governance::GovernanceState) -> Result<()> {
        self.governance_state = Some(state);
        Ok(())
    }
}

impl SnapshotCallbacks for CollectingCallbacks {
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()> {
        self.metadata = Some(metadata);
        Ok(())
    }

    fn on_complete(&mut self) -> Result<()> {
        Ok(())
    }
}

impl SnapshotsCallback for CollectingCallbacks {
    fn on_snapshots(&mut self, _snapshots: SnapshotsContainer) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::{Address, NativeAssets, TxHash, UTXOValue, UTxOIdentifier, Value};
    use minicbor::Encoder;

    use super::*;

    #[test]
    fn test_collecting_callbacks() {
        let mut callbacks = CollectingCallbacks::default();

        // Test metadata callback
        callbacks
            .on_metadata(SnapshotMetadata {
                epoch: 507,
                pot_balances: Pots {
                    reserves: 1000000,
                    treasury: 2000000,
                    deposits: 500000,
                },
                utxo_count: Some(100),
                blocks_previous_epoch: Vec::new(),
                blocks_current_epoch: Vec::new(),
            })
            .unwrap();

        assert_eq!(callbacks.metadata.as_ref().unwrap().epoch, 507);
        assert_eq!(
            callbacks.metadata.as_ref().unwrap().pot_balances.treasury,
            2000000
        );

        // Test UTXO callback
        callbacks
            .on_utxo(UtxoEntry {
                id: UTxOIdentifier::new(TxHash::new(<[u8; 32]>::default()), 0),
                value: UTXOValue {
                    address: Address::None,
                    value: Value {
                        lovelace: 5000000,
                        assets: NativeAssets::default(),
                    },
                    datum: None,
                    script_ref: None,
                },
            })
            .unwrap();

        assert_eq!(callbacks.utxos.len(), 1);
        assert_eq!(callbacks.utxos[0].value.value.lovelace, 5000000);
    }

    #[test]
    fn test_utxo_sidecar_path_from_nes_filename() {
        let path = Path::new("preview/nes.1234.abcdef.cbor");
        let sidecar = StreamingSnapshotParser::utxo_sidecar_path(path).unwrap();
        assert_eq!(
            sidecar,
            Path::new("preview/utxos.1234.abcdef.cbor").to_path_buf()
        );
    }

    #[test]
    fn test_utxo_sidecar_path_from_legacy_snapshot_filename() {
        let path = Path::new("preview/1234.abcdef.cbor");
        let sidecar = StreamingSnapshotParser::utxo_sidecar_path(path).unwrap();
        assert_eq!(
            sidecar,
            Path::new("preview/utxos.1234.abcdef.cbor").to_path_buf()
        );
    }

    #[test]
    fn test_utxo_sidecar_path_keeps_utxo_filename() {
        let path = Path::new("preview/utxos.1234.abcdef.cbor");
        let sidecar = StreamingSnapshotParser::utxo_sidecar_path(path).unwrap();
        assert_eq!(sidecar, path.to_path_buf());
    }

    #[test]
    fn test_strict_maybe_decode_compatibility() {
        let mut buf = Vec::new();
        let mut enc = Encoder::new(&mut buf);
        enc.array(4).unwrap();
        enc.null().unwrap();
        enc.array(0).unwrap();
        enc.array(1).unwrap();
        enc.u64(42).unwrap();
        enc.u64(99).unwrap();

        let mut dec = Decoder::new(&buf);
        dec.array().unwrap();

        let null_case: StrictMaybe<u64> = dec.decode().unwrap();
        let empty_wrapper_case: StrictMaybe<u64> = dec.decode().unwrap();
        let single_wrapper_case: StrictMaybe<u64> = dec.decode().unwrap();
        let direct_case: StrictMaybe<u64> = dec.decode().unwrap();

        assert!(matches!(null_case, StrictMaybe::Nothing));
        assert!(matches!(empty_wrapper_case, StrictMaybe::Nothing));
        assert!(matches!(single_wrapper_case, StrictMaybe::Just(42)));
        assert!(matches!(direct_case, StrictMaybe::Just(99)));
    }

    #[test]
    fn test_account_decode_conway_shape() {
        fn decode_account(bytes: &[u8]) -> SnapshotAccountValue {
            let mut dec = Decoder::new(bytes);
            dec.decode().unwrap()
        }

        // Conway: [balance, deposit, pool, drep]
        let mut conway_buf = Vec::new();
        let mut conway_enc = Encoder::new(&mut conway_buf);
        conway_enc.array(4).unwrap();
        conway_enc.u64(300).unwrap();
        conway_enc.u64(40).unwrap();
        conway_enc.bytes(&[0x33; 28]).unwrap();
        conway_enc.array(1).unwrap();
        conway_enc.u16(2).unwrap(); // DRep::Abstain

        let conway = decode_account(&conway_buf).to_normalized();
        assert_eq!(conway.rewards, 300);
        assert!(conway.delegated_spo.is_some());
        assert!(matches!(conway.delegated_drep, Some(DRepChoice::Abstain)));
    }

    #[test]
    fn test_account_decode_legacy_shape_rejected() {
        // Legacy: [StrictMaybe (reward,deposit), pointers, pool, drep]
        let mut legacy_buf = Vec::new();
        let mut legacy_enc = Encoder::new(&mut legacy_buf);
        legacy_enc.array(4).unwrap();
        legacy_enc.array(1).unwrap();
        legacy_enc.array(2).unwrap();
        legacy_enc.u64(100).unwrap();
        legacy_enc.u64(50).unwrap();
        legacy_enc.array(0).unwrap();
        legacy_enc.null().unwrap();
        legacy_enc.null().unwrap();

        let mut dec = Decoder::new(&legacy_buf);
        let decoded: Result<SnapshotAccountValue, _> = dec.decode();
        assert!(decoded.is_err());
    }

    fn encode_margin(enc: &mut Encoder<&mut Vec<u8>>) {
        enc.array(2).unwrap();
        enc.u64(1).unwrap();
        enc.u64(2).unwrap();
    }

    fn encode_pool_params_without_operator(enc: &mut Encoder<&mut Vec<u8>>, seed: u8) {
        let reward_address = StakeAddress::new(
            StakeCredential::AddrKeyHash(Hash::new([seed.wrapping_add(1); 28])),
            NetworkId::Testnet,
        )
        .to_binary();

        enc.array(8).unwrap();
        enc.bytes(&[seed.wrapping_add(2); 32]).unwrap(); // vrf
        enc.u64(1_000).unwrap(); // pledge
        enc.u64(340).unwrap(); // cost
        encode_margin(enc);
        enc.bytes(&reward_address).unwrap(); // reward account
        enc.array(1).unwrap(); // owners
        enc.bytes(&[seed.wrapping_add(3); 28]).unwrap();
        enc.array(0).unwrap(); // relays
        enc.null().unwrap(); // metadata
    }

    fn encode_new_layout_pstate(future_mode: &str) -> Vec<u8> {
        let mut buf = Vec::new();
        let mut enc = Encoder::new(&mut buf);

        enc.array(4).unwrap();

        // PState[0] psVRFKeyHashes
        enc.map(0).unwrap();

        // PState[1] psStakePools (no operator in value)
        enc.map(1).unwrap();
        enc.bytes(&[0x11; 28]).unwrap();
        encode_pool_params_without_operator(&mut enc, 0x21);

        // PState[2] psFutureStakePoolParams
        match future_mode {
            "empty" => {
                enc.map(0).unwrap();
            }
            "without_operator" => {
                enc.map(1).unwrap();
                enc.bytes(&[0x23; 28]).unwrap();
                encode_pool_params_without_operator(&mut enc, 0x32);
            }
            other => panic!("unsupported future mode {other}"),
        }

        // PState[3] psRetiring
        enc.map(0).unwrap();

        buf
    }

    #[test]
    fn test_parse_pstate_new_layout_with_empty_future_map() {
        let bytes = encode_new_layout_pstate("empty");
        let mut decoder = Decoder::new(&bytes);
        let mut ctx = SnapshotContext {
            network: NetworkId::Testnet,
        };

        let parsed = StreamingSnapshotParser::parse_pstate(&mut decoder, &mut ctx).unwrap();
        assert_eq!(parsed.pools.len(), 1);
        assert_eq!(parsed.updates.len(), 0);
        assert_eq!(parsed.retiring.len(), 0);
    }

    #[test]
    fn test_parse_pstate_new_layout_without_operator_future_map() {
        let bytes = encode_new_layout_pstate("without_operator");
        let mut decoder = Decoder::new(&bytes);
        let mut ctx = SnapshotContext {
            network: NetworkId::Testnet,
        };

        let parsed = StreamingSnapshotParser::parse_pstate(&mut decoder, &mut ctx).unwrap();
        assert_eq!(parsed.pools.len(), 1);
        assert_eq!(parsed.updates.len(), 1);
        assert_eq!(parsed.retiring.len(), 0);
    }
}
