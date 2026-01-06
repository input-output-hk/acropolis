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
use tracing::info;

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
use crate::{PoolBlockProduction, Pots, ProtocolParamUpdate, RewardParams};
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

pub type Epoch = u64;

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
            Type::Array | Type::ArrayIndef => {
                let len = d.array()?;
                if len == Some(0) {
                    Ok(StrictMaybe::Nothing)
                } else {
                    let value = T::decode(d, ctx)?;
                    Ok(StrictMaybe::Just(value))
                }
            }
            _ => Err(minicbor::decode::Error::message("Expected array for Maybe")),
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
                ))
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
pub struct Account {
    pub rewards_and_deposit: StrictMaybe<(Lovelace, Lovelace)>,
    pub pointers: SnapshotSet<(u64, u64, u64)>,
    pub pool: StrictMaybe<PoolId>,
    pub drep: StrictMaybe<DRep>,
}

impl<'b, C> minicbor::Decode<'b, C> for Account {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        Ok(Account {
            rewards_and_deposit: d.decode_with(ctx)?,
            pointers: d.decode_with(ctx)?,
            pool: d.decode_with(ctx)?,
            drep: d.decode_with(ctx)?,
        })
    }
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
        let _len = d.array()?;
        Ok(Self(PoolRegistration {
            operator: d.decode_with(ctx)?,
            vrf_key_hash: d.decode_with(ctx)?,
            pledge: d.decode_with(ctx)?,
            cost: d.decode_with(ctx)?,
            margin: SnapshotRatio::decode(d, ctx)?.0,
            reward_account: SnapshotStakeAddress::decode(d, ctx)?.0,
            pool_owners: SnapshotSet::<SnapshotStakeAddressFromCred>::decode(d, ctx)?
                .0
                .into_iter()
                .map(|a| a.0)
                .collect(),
            relays: Vec::<SnapshotRelay>::decode(d, ctx)?.into_iter().map(|r| r.0).collect(),
            pool_metadata: SnapshotOption::<SnapshotPoolMetadata>::decode(d, ctx)?.0.map(|m| m.0),
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

impl<'b, C> minicbor::Decode<'b, C> for SnapshotStakeAddress {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let bytes = d.bytes()?;
        let bytes = bytes.to_vec();
        Ok(Self(StakeAddress::from_binary(&bytes).map_err(|e| {
            minicbor::decode::Error::message(e.to_string())
        })?))
    }
}

struct SnapshotStakeAddressFromCred(pub StakeAddress);

impl<'b, C> minicbor::Decode<'b, C> for SnapshotStakeAddressFromCred
where
    C: AsRef<SnapshotContext>,
{
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let bytes = d.bytes()?;
        let bytes = Hash::<28>::try_from(bytes)
            .map_err(|e| minicbor::decode::Error::message(e.to_string()))?;
        Ok(Self(StakeAddress::new(
            StakeCredential::AddrKeyHash(bytes),
            ctx.as_ref().network.clone(),
        )))
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
        let hash = Hash::<32>::decode(d, ctx)?.to_vec();
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
            info!("Parsing snapshot for epoch {}", epoch);

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

            // Parse PState [3][1][0][1] for pools
            let pools = Self::parse_pstate(&mut decoder, &mut ctx)
                .context("Failed to parse PState for pools")?;

            // Parse DState [3][1][0][2] for accounts/delegations
            // DState is an array: [unified_rewards, fut_gen_deleg, gen_deleg, instant_rewards]
            decoder.array().context("Failed to parse DState array")?;

            // Parse unified rewards - it's actually an array containing the map
            // UMap structure: [rewards_map, ...]
            let umap_len = decoder.array().context("Failed to parse UMap array")?;

            // Parse the rewards map [0]: StakeCredential -> Account
            let accounts_map: BTreeMap<StakeCredential, Account> = decoder.decode()?;

            // Skip remaining UMap elements if any
            if let Some(len) = umap_len {
                for _ in 1..len {
                    decoder.skip()?;
                }
            }

            // Epoch State / Ledger State / Cert State / Delegation state / dsFutureGenDelegs
            decoder.skip()?;

            // Epoch State / Ledger State / Cert State / Delegation state / dsGenDelegs
            decoder.skip()?;

            // Epoch State / Ledger State / Cert State / Delegation state / dsIRewards
            // Parse instant rewards (MIRs) and combine with regular rewards
            // Structure: [ir_reserves, ir_treasury, ir_delta_reserves, ir_delta_treasury]
            let instant_rewards_result = Self::parse_instant_rewards(&mut decoder)
                .context("Failed to parse instant rewards")?;

            // Log instant rewards deltas
            info!(
                "Instant rewards deltas: delta_treasury={}, delta_reserves={}",
                instant_rewards_result.delta_treasury, instant_rewards_result.delta_reserves
            );

            // Convert to AccountState for API, combining regular rewards with instant rewards
            let accounts: Vec<AccountState> = accounts_map
                .into_iter()
                .map(|(credential, account)| {
                    // Convert StakeCredential to stake address representation
                    let stake_address = StakeAddress::new(credential.clone(), network.clone());

                    // Extract rewards from rewards_and_deposit (first element of tuple)
                    let regular_rewards = match &account.rewards_and_deposit {
                        StrictMaybe::Just((reward, _deposit)) => *reward,
                        StrictMaybe::Nothing => 0,
                    };

                    // Add instant rewards (MIRs) if any
                    let mir_rewards =
                        instant_rewards_result.rewards.get(&credential).copied().unwrap_or(0);
                    let rewards = regular_rewards + mir_rewards;

                    // Convert SPO delegation from StrictMaybe<PoolId> to Option<KeyHash>
                    // PoolId is Hash<28>, we need to convert to Vec<u8>
                    let delegated_spo = match &account.pool {
                        StrictMaybe::Just(pool_id) => Some(*pool_id),
                        StrictMaybe::Nothing => None,
                    };

                    // Convert DRep delegation from StrictMaybe<DRep> to Option<DRepChoice>
                    let delegated_drep = match &account.drep {
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

        // Read only the UTXO section from the file (not the entire file!)
        let mut utxo_file = File::open(&self.file_path).context(format!(
            "Failed to open snapshot file for UTXO reading: {}",
            self.file_path
        ))?;

        // TRUE STREAMING: Process UTXOs one by one with minimal memory usage
        utxo_file.seek(SeekFrom::Start(utxo_file_position))?;
        let (utxo_count, bytes_consumed_from_file, stake_utxo_values) =
            Self::stream_utxos(&mut utxo_file, callbacks)
                .context("Failed to stream UTXOs with true streaming")?;

        // After UTXOs, parse deposits from UTxOState[1]
        // Reset our file pointer to a position after UTXOs
        let position_after_utxos = utxo_file_position + bytes_consumed_from_file;
        utxo_file.seek(SeekFrom::Start(position_after_utxos))?;

        info!(
            "    UTXO parsing complete. File positioned at byte {} for remainder parsing",
            position_after_utxos
        );

        // ========================================================================
        // HYBRID APPROACH: MEMORY-BASED PARSING OF REMAINDER
        // ========================================================================
        // After extensive analysis, the remaining snapshot data (deposits, fees,
        // protocol parameters, and mark/set/go snapshots) can be efficiently
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
        // 3. Access mark/set/go snapshots that were previously unreachable
        // ========================================================================

        // Calculate remaining file size from current position
        let current_file_size = utxo_file.metadata()?.len();
        let remaining_bytes = current_file_size.saturating_sub(position_after_utxos);

        info!(
            "    Reading remainder of file into memory: {:.1} MB from position {}",
            remaining_bytes as f64 / 1024.0 / 1024.0,
            position_after_utxos
        );

        // Read the entire remainder of the file into memory
        let mut remainder_buffer = Vec::with_capacity(remaining_bytes as usize);
        utxo_file.read_to_end(&mut remainder_buffer)?;

        info!(
            "    Successfully loaded {:.1} MB remainder buffer for parsing",
            remainder_buffer.len() as f64 / 1024.0 / 1024.0
        );

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

        info!(
            "    Successfully parsed governance state: {} proposals, {} votes",
            governance_state.proposals.len(),
            governance_state.votes.len()
        );

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
        let enacted_proposal_count = governance_state.enacted_actions.len();
        let expired_proposal_count = governance_state.expired_action_ids.len();

        // Subtract pending and enacted governance proposal deposits from us_deposited
        // The snapshot's us_deposited includes these, but they shouldn't be in our deposits pot
        // Enacted proposals will be refunded at epoch boundary, but snapshot is taken before that
        let governance_deposits = pending_proposal_deposits + enacted_proposal_deposits;
        let deposits = deposits.saturating_sub(governance_deposits);

        if governance_deposits > 0 {
            info!(
                "Governance proposal deposits: {} pending ({} ADA) + {} enacted ({} ADA) = {} ADA (subtracted from us_deposited)",
                governance_state.proposals.len(),
                pending_proposal_deposits / 1_000_000,
                enacted_proposal_count,
                enacted_proposal_deposits / 1_000_000,
                governance_deposits / 1_000_000
            );
        }

        info!(
            "Governance state: enacted={}, expired={} proposals",
            enacted_proposal_count, expired_proposal_count,
        );

        // Extract pool deposit from protocol parameters before consuming governance_state
        let stake_pool_deposit = match governance_state.protocol_params.pool_deposit {
            Some(deposit) => deposit,
            None => {
                return Err(anyhow::anyhow!(
                    "Stake pool deposit must exist in protocol params"
                ))
            }
        };

        // Emit governance state callback
        callbacks.on_governance_state(governance_state)?;

        // Epoch State / Ledger State / UTxO State / utxosStakeDistr
        remainder_decoder.skip()?;

        // Epoch State / Ledger State / UTxO State / utxosDonation
        // Treasury donations accumulate during epoch and are added to treasury at epoch boundary
        let donations: u64 = remainder_decoder.decode().unwrap_or(0);
        if donations > 0 {
            info!("Treasury donations: {} ADA", donations / 1_000_000);
        }

        // Parse mark/set/go snapshots (EpochState[2])
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

        // Build pots for snapshot conversion (these are the epoch N pots from the snapshot)
        let pots = Pots {
            reserves,
            treasury,
            deposits,
        };

        // Log the deltas from pulsing_rew_update (applied during bootstrap to adjust pots)
        info!(
            "Pulsing reward update deltas: delta_treasury={}, delta_reserves={}",
            pulsing_result.delta_treasury, pulsing_result.delta_reserves
        );

        let (bootstrap_snapshots, fees_prev_epoch) = match snapshots_result {
            Ok(raw_snapshots) => {
                info!("Successfully parsed mark/set/go snapshots!");
                let fees = raw_snapshots.fees;
                let processed = raw_snapshots.into_snapshots_container(
                    epoch,
                    &blocks_prev_map,
                    &blocks_curr_map,
                    pots.clone(),
                    network.clone(),
                );
                info!(
                    "Parsed snapshots: Mark {} SPOs, Set {} SPOs, Go {} SPOs",
                    processed.mark.spos.len(),
                    processed.set.spos.len(),
                    processed.go.spos.len()
                );
                callbacks.on_snapshots(processed.clone())?;
                (processed, fees)
            }
            Err(e) => {
                info!("    Failed to parse snapshots: {}", e);
                info!("    Using empty snapshots (pre-Shelley or parse error)...");
                (SnapshotsContainer::default(), 0)
            }
        };

        // Build pool registrations list for AccountsBootstrapMessage
        let pool_registrations: Vec<PoolRegistration> = pools.pools.values().cloned().collect();
        let retiring_pools: Vec<PoolId> = pools
            .retiring
            .iter()
            .filter(|(_, retiring_epoch)| **retiring_epoch == epoch)
            .map(|(pool_id, _)| *pool_id)
            .collect();

        info!(
            "Pools: {} registered, {} retiring, {} DReps",
            pool_registrations.len(),
            retiring_pools.len(),
            dreps.len()
        );

        // Convert DRepInfo to (credential, deposit) tuples
        let drep_deposits: Vec<(DRepCredential, u64)> =
            dreps.iter().map(|(cred, record)| (cred.clone(), record.deposit)).collect();

        // Calculate total DRep deposits
        let total_drep_deposits: u64 = drep_deposits.iter().map(|(_, d)| d).sum();
        let total_pool_deposits: u64 = (pool_registrations.len() as u64) * stake_pool_deposit;

        // Subtract DRep deposits from us_deposited
        // The snapshot's us_deposited includes DRep deposits, but they shouldn't be in our deposits pot
        let deposits = deposits.saturating_sub(total_drep_deposits);

        info!(
            "Deposit breakdown: total_deposits={} ADA (after subtracting {} ADA drep deposits), pool_deposits={} ADA ({} pools), drep_count={}",
            deposits / 1_000_000,
            total_drep_deposits / 1_000_000,
            total_pool_deposits / 1_000_000,
            pool_registrations.len(),
            drep_deposits.len()
        );

        // Merge UTXO values and pulsing reward update rewards into accounts
        // The pulsing_rew_update contains rewards calculated during the current epoch that need to be
        // added to DState rewards (accumulated rewards from previous epochs).
        let mut pulsing_rewards_total: u64 = 0;

        let mut accounts_with_utxo_values: Vec<AccountState> = accounts
            .into_iter()
            .map(|mut account| {
                if let Some(&utxo_value) = stake_utxo_values.get(&account.stake_address.credential)
                {
                    account.address_state.utxo_value = utxo_value;
                }
                if let Some(&pulsing_reward) =
                    pulsing_result.rewards.get(&account.stake_address.credential)
                {
                    account.address_state.rewards += pulsing_reward;
                    pulsing_rewards_total += pulsing_reward;
                }
                account
            })
            .collect();

        // Add accounts for stake addresses that have UTXOs but aren't registered in DState
        // These are addresses that received funds but were never registered for staking
        let registered_credentials: std::collections::HashSet<_> =
            accounts_with_utxo_values.iter().map(|a| a.stake_address.credential.clone()).collect();

        let unregistered_accounts: Vec<_> = stake_utxo_values
            .iter()
            .filter(|(credential, _)| !registered_credentials.contains(credential))
            .map(|(credential, &utxo_value)| AccountState {
                stake_address: StakeAddress::new(credential.clone(), network.clone()),
                address_state: StakeAddressState {
                    registered: false,
                    utxo_value,
                    rewards: 0,
                    delegated_spo: None,
                    delegated_drep: None,
                },
            })
            .collect();

        if !unregistered_accounts.is_empty() {
            info!(
                "Added {} unregistered stake addresses with UTXOs to bootstrap",
                unregistered_accounts.len()
            );
        }
        accounts_with_utxo_values.extend(unregistered_accounts);

        // Calculate summary statistics
        let total_utxo_value: u64 = stake_utxo_values.values().sum();
        let total_rewards: u64 =
            accounts_with_utxo_values.iter().map(|a| a.address_state.rewards).sum();
        let delegated_count = accounts_with_utxo_values
            .iter()
            .filter(|a| a.address_state.delegated_spo.is_some())
            .count();

        info!(
            "Accounts: {} total, {} delegated, {} ADA in UTXOs, {} ADA rewards ({} ADA from pulsing update)",
            accounts_with_utxo_values.len(),
            delegated_count,
            total_utxo_value / 1_000_000,
            total_rewards / 1_000_000,
            pulsing_rewards_total / 1_000_000
        );

        // Calculate deposit refunds for deregistered accounts with pending rewards
        // When rewards are paid to a deregistered account:
        // 1. The reward goes to treasury instead
        // 2. Their stake key deposit is refunded (reducing total deposits)
        let mut unclaimed_rewards: u64 = 0;
        let mut deregistered_with_rewards: u64 = 0;

        // Check pulsing rewards for deregistered accounts
        for (credential, &reward) in &pulsing_result.rewards {
            if !registered_credentials.contains(credential) {
                unclaimed_rewards += reward;
                deregistered_with_rewards += 1;
            }
        }

        // Also check instant rewards (MIRs) for deregistered accounts
        for (credential, &reward) in &instant_rewards_result.rewards {
            if !registered_credentials.contains(credential) {
                unclaimed_rewards += reward;
                // Only count unique deregistered accounts (avoid double counting)
                if !pulsing_result.rewards.contains_key(credential) {
                    deregistered_with_rewards += 1;
                }
            }
        }

        // Note: Stake key deposit refunds happen immediately when the deregistration tx is processed,
        // not at epoch boundary. The snapshot's us_deposited already reflects these refunds.
        // We only track deregistered_with_rewards for the unclaimed rewards -> treasury calculation.
        if deregistered_with_rewards > 0 {
            info!(
                "Deregistered accounts with rewards: {} accounts, {} ADA unclaimed rewards -> treasury",
                deregistered_with_rewards,
                unclaimed_rewards / 1_000_000
            );
        }

        // Calculate governance proposal deposit refunds for enacted proposals
        // We use enacted_proposal_deposits which was calculated earlier by summing each proposal's
        // actual deposit field. For expired proposals, we only have IDs (no deposit amounts),
        // so we cannot include them here. This is acceptable because expired proposal refunds
        // should be tracked when proposals are processed, not at epoch boundary.
        let gov_deposit_refunds = enacted_proposal_deposits;

        if enacted_proposal_count > 0 || expired_proposal_count > 0 {
            info!(
                "Governance deposit refunds: {} enacted ({} ADA), {} expired (not included - IDs only)",
                enacted_proposal_count,
                gov_deposit_refunds / 1_000_000,
                expired_proposal_count
            );
        }

        let total_deposit_refunds = gov_deposit_refunds;

        // Combine pot deltas from pulsing_rew_update and instant_rewards
        // Plus adjustments for deregistered accounts with pending rewards
        // Plus governance proposal deposit refunds
        // Plus treasury donations
        //
        // Use checked arithmetic to detect overflow
        let unclaimed_rewards_i64 =
            i64::try_from(unclaimed_rewards).expect("unclaimed_rewards exceeds i64::MAX");
        let donations_i64 = i64::try_from(donations).expect("donations exceeds i64::MAX");
        let total_deposit_refunds_i64 =
            i64::try_from(total_deposit_refunds).expect("total_deposit_refunds exceeds i64::MAX");

        let delta_treasury = pulsing_result
            .delta_treasury
            .checked_add(instant_rewards_result.delta_treasury)
            .and_then(|v| v.checked_add(unclaimed_rewards_i64))
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

        info!(
            "Combined pot deltas: delta_treasury={} (donations={}), delta_reserves={}, delta_deposits={} (gov={})",
            pot_deltas.delta_treasury, donations,
            pot_deltas.delta_reserves, pot_deltas.delta_deposits,
            gov_deposit_refunds
        );

        // Build the accounts bootstrap data
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
        let epoch_bootstrap =
            EpochBootstrapData::new(epoch, &blocks_previous_epoch, &blocks_current_epoch, total_fees_current);
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
            "Snapshot parsing complete: treasury {} ADA, reserves {} ADA, deposits {} ADA",
            treasury / 1_000_000,
            reserves / 1_000_000,
            deposits / 1_000_000
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
                            let buffer_usage = buffer.len();
                            info!(
                                "Streamed {} UTXOs, buffer: {} MB, max entry: {} bytes",
                                utxo_count,
                                buffer_usage / 1024 / 1024,
                                max_single_entry_size
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

        info!("Streaming results:");
        info!("  UTXOs processed: {}", utxo_count);
        info!(
            "  Total data streamed: {:.2} MB",
            total_bytes_processed as f64 / 1024.0 / 1024.0
        );
        info!(
            "  Peak buffer usage: {} MB",
            PARSE_BUFFER_SIZE / 1024 / 1024
        );
        info!("  Largest single entry: {} bytes", max_single_entry_size);

        // After successfully parsing all UTXOs, we need to consume the break token
        // that ends the indefinite-length UTXO map if present
        if !buffer.is_empty() {
            let mut decoder = Decoder::new(&buffer);
            match decoder.datatype() {
                Ok(Type::Break) => {
                    info!("    Found break token after UTXOs, consuming it (end of indefinite UTXO map)");
                    decoder.skip()?; // Consume the break that ends the UTXO map

                    // Update our tracking to account for the consumed break token
                    let break_bytes_consumed = decoder.position();
                    buffer.drain(0..break_bytes_consumed);
                }
                Ok(_) => {
                    // No break token, this is a definite-length map - continue normal parsing
                    info!("    No break token found, assuming definite-length UTXO map");
                }
                Err(e) => {
                    info!("    After UTXO parsing, datatype() check failed: {}", e);
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
                    info!("Processing indefinite-length blocks array");
                    let mut count = 0;
                    loop {
                        match decoder.datatype()? {
                            Type::Break => {
                                decoder.skip()?;
                                info!("Found array break after {} entries", count);
                                break;
                            }
                            entry_type => {
                                info!("  Block #{}: {:?}", count + 1, entry_type);
                                decoder.skip().context("Failed to skip block entry")?;
                                count += 1;
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
                        let value = decoder.u64().context("Failed to read block integer value")?;
                        info!("Block data is integer: {}", value);
                    }
                    Type::Bytes => {
                        let bytes = decoder.bytes().context("Failed to read block bytes")?;
                        info!("Block data is {} bytes", bytes.len());
                    }
                    Type::String => {
                        let text = decoder.str().context("Failed to read block text")?;
                        info!("Block data is text: '{}'", text);
                    }
                    Type::Null => {
                        decoder.skip()?;
                        info!("Block data is null");
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
                // delta_r1 is the reserves decrease, delta_t1 is the treasury increase
                // Both are positive values in RewardSnapshot
                info!(
                    "Pulsing reward snapshot: delta_r1={}, delta_t1={}, r={}",
                    snapshot.delta_r1, snapshot.delta_t1, snapshot.r
                );
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
                // In RewardUpdate: invert_dr and invert_df are stored inverted
                // We need to negate them to get actual deltas
                info!(
                    "Complete reward update: delta_treasury={}, delta_reserves(inverted)={}, delta_fees(inverted)={}",
                    update.delta_treasury, update.delta_reserves, update.delta_fees
                );
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

    /// Parse PState to extract stake pools
    /// PState = [pools_map, future_pools_map, retiring_map, deposits_map]
    pub fn parse_pstate(decoder: &mut Decoder, ctx: &mut SnapshotContext) -> Result<SPOState> {
        // Parse PState array
        let pstate_len = decoder
            .array()
            .context("Failed to parse PState array")?
            .ok_or_else(|| anyhow!("PState must be a definite-length array"))?;

        if pstate_len < 1 {
            return Err(anyhow!(
                "PState array too short: expected at least 1 element, got {pstate_len}"
            ));
        }

        // Parse pools map [0]: PoolId (Hash<28>) -> PoolParams
        // Note: Maps might be tagged with CBOR tag 258 (set)
        if matches!(decoder.datatype()?, Type::Tag) {
            decoder.tag()?; // skip tag if present
        }

        let mut pools = BTreeMap::new();
        match decoder.map()? {
            Some(pool_count) => {
                // Definite-length map
                for i in 0..pool_count {
                    let pool_id: PoolId =
                        decoder.decode().context(format!("Failed to decode pool id #{i}"))?;
                    let pool: SnapshotPoolRegistration = decoder
                        .decode_with(ctx)
                        .context(format!("Failed to decode pool for pool #{i}"))?;
                    pools.insert(pool_id, pool.0);
                }
            }
            None => {
                // Indefinite-length map
                let mut count = 0;
                loop {
                    match decoder.datatype()? {
                        Type::Break => {
                            decoder.skip()?;
                            break;
                        }
                        _ => {
                            let pool_id: PoolId = decoder
                                .decode()
                                .context(format!("Failed to decode pool id #{count}"))?;
                            let pool: SnapshotPoolRegistration = decoder
                                .decode_with(ctx)
                                .context(format!("Failed to decode pool for pool #{count}"))?;
                            pools.insert(pool_id, pool.0);
                            count += 1;
                        }
                    }
                }
            }
        }

        // Parse future pools map [1]: PoolId -> PoolParams
        if matches!(decoder.datatype()?, Type::Tag) {
            decoder.tag()?;
        }
        let updates: BTreeMap<PoolId, SnapshotPoolRegistration> = decoder.decode_with(ctx)?;
        let updates = updates.into_iter().map(|(id, pool)| (id, pool.0)).collect();

        // Parse retiring map [2]: PoolId -> Epoch
        if matches!(decoder.datatype()?, Type::Tag) {
            decoder.tag()?;
        }
        let retiring: BTreeMap<PoolId, Epoch> = decoder.decode()?;

        // Skip any remaining PState elements (like deposits)
        for i in 3..pstate_len {
            decoder.skip().context(format!("Failed to skip PState[{i}]"))?;
        }

        Ok(SPOState {
            pools,
            updates,
            retiring,
        })
    }

    /// Parse snapshots using hybrid approach with memory-based parsing
    /// Uses snapshot.rs functions to parse mark/set/go snapshots from buffer
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

        // Parse Mark, Set, Go snapshots
        let mark_snapshot =
            RawSnapshot::parse(decoder, ctx, "Mark").context("Failed to parse Mark snapshot")?;
        let set_snapshot =
            RawSnapshot::parse(decoder, ctx, "Set").context("Failed to parse Set snapshot")?;
        let go_snapshot =
            RawSnapshot::parse(decoder, ctx, "Go").context("Failed to parse Go snapshot")?;
        let fees = decoder.decode::<u64>().unwrap_or(0);

        Ok(RawSnapshotsContainer {
            mark: mark_snapshot,
            set: set_snapshot,
            go: go_snapshot,
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
        info!(
            "CollectingCallbacks: Received governance state with {} proposals",
            state.proposals.len()
        );
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
    fn on_snapshots(&mut self, snapshots: SnapshotsContainer) -> Result<()> {
        // For testing, we could store snapshots here if needed
        info!(
            "CollectingCallbacks: Received snapshots with {} mark SPOs, {} set SPOs, {} go SPOs",
            snapshots.mark.spos.len(),
            snapshots.set.spos.len(),
            snapshots.go.spos.len()
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{Address, NativeAssets, TxHash, UTXOValue, UTxOIdentifier, Value};

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
                    reference_script: None,
                },
            })
            .unwrap();

        assert_eq!(callbacks.utxos.len(), 1);
        assert_eq!(callbacks.utxos[0].value.value.lovelace, 5000000);
    }
}
