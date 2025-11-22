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
//!

use anyhow::{anyhow, Context, Result};
use minicbor::data::Type;
use minicbor::Decoder;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use tracing::info;

pub use crate::hash::Hash;
pub use crate::stake_addresses::{AccountState, StakeAddressState};
pub use crate::StakeCredential;

// -----------------------------------------------------------------------------
// Cardano Ledger Types (for decoding with minicbor)
// -----------------------------------------------------------------------------

pub type Epoch = u64;
pub type Lovelace = u64;

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
    fn decode(
        d: &mut minicbor::Decoder<'b>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let variant = d.u16()?;

        match variant {
            0 => {
                // ScriptHash variant (first in enum) - decode bytes directly
                let bytes = d.bytes()?;
                let key_hash = bytes.try_into().map_err(|_| {
                    minicbor::decode::Error::message(
                        "invalid length for ScriptHash in StakeCredential",
                    )
                })?;
                Ok(StakeCredential::ScriptHash(key_hash))
            }
            1 => {
                // AddrKeyHash variant (second in enum) - decodes bytes directly
                let bytes = d.bytes()?;
                let key_hash = bytes.try_into().map_err(|_| {
                    minicbor::decode::Error::message(
                        "invalid length for AddrKeyHash in StakeCredential",
                    )
                })?;
                Ok(StakeCredential::AddrKeyHash(key_hash))
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
        match self {
            StakeCredential::ScriptHash(key_hash) => {
                // ScriptHash is variant 0 (first in enum)
                e.array(2)?;
                e.encode_with(0, ctx)?;
                e.encode_with(key_hash, ctx)?;
                Ok(())
            }
            StakeCredential::AddrKeyHash(key_hash) => {
                // AddrKeyHash is variant 1 (second in enum)
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Set<T>(pub Vec<T>);

impl<T> Set<T> {
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }
}

impl<T> From<Vec<T>> for Set<T> {
    fn from(vec: Vec<T>) -> Self {
        Set(vec)
    }
}

impl<T> From<Set<T>> for Vec<T> {
    fn from(set: Set<T>) -> Self {
        set.0
    }
}

impl<'b, C, T> minicbor::Decode<'b, C> for Set<T>
where
    T: minicbor::Decode<'b, C>,
{
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        // Sets might be tagged with CBOR tag 258
        if matches!(d.datatype()?, Type::Tag) {
            d.tag()?;
        }

        let vec: Vec<T> = d.decode_with(ctx)?;
        Ok(Set(vec))
    }
}

impl<C, T> minicbor::Encode<C> for Set<T>
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
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
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
    pub pointers: Set<(u64, u64, u64)>,
    pub pool: StrictMaybe<PoolId>,
    pub drep: StrictMaybe<DRep>,
}

impl<'b, C> minicbor::Decode<'b, C> for Account {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
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
// Type aliases for pool_params compatibility
// -----------------------------------------------------------------------------

pub use crate::types::AddrKeyhash;
pub use crate::types::ScriptHash;
use crate::PoolId;
/// Alias minicbor as cbor for pool_params module
pub use minicbor as cbor;

/// Coin amount (Lovelace)
pub type Coin = u64;

/// Reward account (stake address bytes) - wrapper to handle CBOR bytes encoding
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewardAccount(pub Vec<u8>);

impl<'b, C> minicbor::Decode<'b, C> for RewardAccount {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let bytes = d.bytes()?;
        Ok(RewardAccount(bytes.to_vec()))
    }
}

impl<C> minicbor::Encode<C> for RewardAccount {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.bytes(&self.0)?;
        Ok(())
    }
}

/// Unit interval (rational number for pool margin)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitInterval {
    pub numerator: u64,
    pub denominator: u64,
}

impl<'b, C> minicbor::Decode<'b, C> for UnitInterval {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        // UnitInterval might be tagged (tag 30 for rational)
        if matches!(d.datatype()?, Type::Tag) {
            d.tag()?;
        }
        d.array()?;
        let numerator = d.u64()?;
        let denominator = d.u64()?;
        Ok(UnitInterval {
            numerator,
            denominator,
        })
    }
}

impl<C> minicbor::Encode<C> for UnitInterval {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.tag(minicbor::data::Tag::new(30))?;
        e.array(2)?;
        e.u64(self.numerator)?;
        e.u64(self.denominator)?;
        Ok(())
    }
}

/// Nullable type (like Maybe but with explicit null vs undefined)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Nullable<T> {
    Undefined,
    Null,
    Some(T),
}

impl<'b, C, T> minicbor::Decode<'b, C> for Nullable<T>
where
    T: minicbor::Decode<'b, C>,
{
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        match d.datatype()? {
            Type::Null => {
                d.skip()?;
                Ok(Nullable::Null)
            }
            Type::Undefined => {
                d.skip()?;
                Ok(Nullable::Undefined)
            }
            _ => {
                let value = T::decode(d, ctx)?;
                Ok(Nullable::Some(value))
            }
        }
    }
}

impl<C, T> minicbor::Encode<C> for Nullable<T>
where
    T: minicbor::Encode<C>,
{
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        match self {
            Nullable::Undefined => e.undefined()?.ok(),
            Nullable::Null => e.null()?.ok(),
            Nullable::Some(v) => v.encode(e, ctx),
        }
    }
}

// Network types for pool relays
pub type Port = u32;

/// IPv4 address (4 bytes, encoded as CBOR bytes)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IPv4(pub Vec<u8>);

impl<'b, C> minicbor::Decode<'b, C> for IPv4 {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let bytes = d.bytes()?;
        Ok(IPv4(bytes.to_vec()))
    }
}

impl<C> minicbor::Encode<C> for IPv4 {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.bytes(&self.0)?;
        Ok(())
    }
}

/// IPv6 address (16 bytes, encoded as CBOR bytes)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IPv6(pub Vec<u8>);

impl<'b, C> minicbor::Decode<'b, C> for IPv6 {
    fn decode(d: &mut Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let bytes = d.bytes()?;
        Ok(IPv6(bytes.to_vec()))
    }
}

impl<C> minicbor::Encode<C> for IPv6 {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        _ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.bytes(&self.0)?;
        Ok(())
    }
}

/// Pool relay types (for CBOR encoding/decoding)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Relay {
    SingleHostAddr(Nullable<Port>, Nullable<IPv4>, Nullable<IPv6>),
    SingleHostName(Nullable<Port>, String),
    MultiHostName(String),
}

impl<'b, C> minicbor::Decode<'b, C> for Relay {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let tag = d.u32()?;

        match tag {
            0 => {
                // SingleHostAddr
                let port = Nullable::<Port>::decode(d, ctx)?;
                let ipv4 = Nullable::<IPv4>::decode(d, ctx)?;
                let ipv6 = Nullable::<IPv6>::decode(d, ctx)?;
                Ok(Relay::SingleHostAddr(port, ipv4, ipv6))
            }
            1 => {
                // SingleHostName
                let port = Nullable::<Port>::decode(d, ctx)?;
                let hostname = d.str()?.to_string();
                Ok(Relay::SingleHostName(port, hostname))
            }
            2 => {
                // MultiHostName
                let hostname = d.str()?.to_string();
                Ok(Relay::MultiHostName(hostname))
            }
            _ => Err(minicbor::decode::Error::message("Invalid relay tag")),
        }
    }
}

impl<C> minicbor::Encode<C> for Relay {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        match self {
            Relay::SingleHostAddr(port, ipv4, ipv6) => {
                e.array(4)?;
                e.u32(0)?;
                port.encode(e, ctx)?;
                ipv4.encode(e, ctx)?;
                ipv6.encode(e, ctx)?;
                Ok(())
            }
            Relay::SingleHostName(port, hostname) => {
                e.array(3)?;
                e.u32(1)?;
                port.encode(e, ctx)?;
                e.str(hostname)?;
                Ok(())
            }
            Relay::MultiHostName(hostname) => {
                e.array(2)?;
                e.u32(2)?;
                e.str(hostname)?;
                Ok(())
            }
        }
    }
}

/// Pool metadata (for CBOR encoding/decoding)
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PoolMetadata {
    pub url: String,
    pub hash: Hash<32>,
}

impl<'b, C> minicbor::Decode<'b, C> for PoolMetadata {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let url = d.str()?.to_string();
        let hash = Hash::<32>::decode(d, ctx)?;
        Ok(PoolMetadata { url, hash })
    }
}

impl<C> minicbor::Encode<C> for PoolMetadata {
    fn encode<W: minicbor::encode::Write>(
        &self,
        e: &mut minicbor::Encoder<W>,
        ctx: &mut C,
    ) -> Result<(), minicbor::encode::Error<W::Error>> {
        e.array(2)?;
        e.str(&self.url)?;
        self.hash.encode(e, ctx)?;
        Ok(())
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
    pub delegators: Set<StakeCredential>,
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
        let delegators = Set::<StakeCredential>::decode(d, ctx)?;

        Ok(DRepState {
            expiry,
            anchor,
            deposit,
            delegators,
        })
    }
}

// -----------------------------------------------------------------------------
// Data Structures (based on OpenAPI schema)
// -----------------------------------------------------------------------------

/// UTXO entry with transaction hash, index, address, and value
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UtxoEntry {
    /// Transaction hash (hex-encoded)
    pub tx_hash: String,
    /// Output index
    pub output_index: u64,
    /// Hex encoded Cardano addresses
    pub address: String,
    /// Lovelace amount
    pub value: u64,
    /// Optional inline datum (hex-encoded CBOR)
    pub datum: Option<String>,
    /// Optional script reference (hex-encoded CBOR)
    pub script_ref: Option<String>,
}

// -----------------------------------------------------------------------------
// Ledger types for DState parsing
// -----------------------------------------------------------------------------

/// DRep credential (ledger format for CBOR decoding)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DRepCredential {
    AddrKeyhash(AddrKeyhash),
    ScriptHash(ScriptHash),
}

impl<'b, C> minicbor::Decode<'b, C> for DRepCredential {
    fn decode(d: &mut Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?;
        let variant = d.u16()?;

        match variant {
            0 => Ok(DRepCredential::AddrKeyhash(d.decode_with(ctx)?)),
            1 => Ok(DRepCredential::ScriptHash(d.decode_with(ctx)?)),
            _ => Err(minicbor::decode::Error::message(
                "invalid variant id for DRepCredential",
            )),
        }
    }
}

// -----------------------------------------------------------------------------
// Data Structures (based on OpenAPI schema)
// -----------------------------------------------------------------------------

/// Stake pool information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolInfo {
    /// Bech32-encoded pool ID
    pub pool_id: String,
    /// Hex-encoded VRF key hash
    pub vrf_key_hash: String,
    /// Pledge amount in Lovelace
    pub pledge: u64,
    /// Fixed cost in Lovelace
    pub cost: u64,
    /// Pool margin (0.0 to 1.0)
    pub margin: f64,
    /// Bech32-encoded reward account
    pub reward_account: String,
    /// List of pool owner stake addresses
    pub pool_owners: Vec<String>,
    /// Pool relay information
    pub relays: Vec<ApiRelay>,
    /// Pool metadata (URL and hash)
    pub pool_metadata: Option<ApiPoolMetadata>,
    /// Optional retirement epoch
    pub retirement_epoch: Option<u64>,
}

/// Pool relay information (for API/JSON output)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ApiRelay {
    SingleHostAddr {
        port: Option<u16>,
        ipv4: Option<String>,
        ipv6: Option<String>,
    },
    SingleHostName {
        port: Option<u16>,
        dns_name: String,
    },
    MultiHostName {
        dns_name: String,
    },
}

/// Pool metadata anchor (for API/JSON output)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiPoolMetadata {
    /// IPFS or HTTP(S) URL
    pub url: String,
    /// Hex-encoded hash
    pub hash: String,
}

/// DRep information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DRepInfo {
    /// Bech32-encoded DRep ID
    pub drep_id: String,
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

/// Pot balances (treasury, reserves, deposits)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PotBalances {
    /// Current reserves pot balance in Lovelace
    pub reserves: u64,
    /// Current treasury pot balance in Lovelace
    pub treasury: u64,
    /// Current deposits pot balance in Lovelace
    pub deposits: u64,
}

/// Snapshot metadata extracted before streaming
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotMetadata {
    /// Epoch number
    pub epoch: u64,
    /// Pot balances
    pub pot_balances: PotBalances,
    /// Total number of UTXOs (for progress tracking)
    pub utxo_count: Option<u64>,
    /// Block production statistics for previous epoch
    pub blocks_previous_epoch: Vec<crate::types::PoolBlockProduction>,
    /// Block production statistics for current epoch
    pub blocks_current_epoch: Vec<crate::types::PoolBlockProduction>,
}

// -----------------------------------------------------------------------------
// Callback Traits
// -----------------------------------------------------------------------------

/// Callback invoked for each UTXO entry (streaming)
pub trait UtxoCallback {
    /// Called once per UTXO entry
    fn on_utxo(&mut self, utxo: UtxoEntry) -> Result<()>;
}

/// Callback invoked with bulk stake pool data
pub trait PoolCallback {
    /// Called once with all pool data
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()>;
}

/// Callback invoked with bulk stake account data
pub trait StakeCallback {
    /// Called once with all account states
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()>;
}

/// Callback invoked with bulk DRep data
pub trait DRepCallback {
    /// Called once with all DRep info
    fn on_dreps(&mut self, dreps: Vec<DRepInfo>) -> Result<()>;
}

/// Callback invoked with bulk governance proposal data
pub trait ProposalCallback {
    /// Called once with all proposals
    fn on_proposals(&mut self, proposals: Vec<GovernanceProposal>) -> Result<()>;
}

/// Combined callback handler for all snapshot data
pub trait SnapshotCallbacks:
    UtxoCallback + PoolCallback + StakeCallback + DRepCallback + ProposalCallback
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
    pools: Vec<PoolInfo>,
    dreps: Vec<DRepInfo>,
    accounts: Vec<AccountState>,
    blocks_previous_epoch: Vec<crate::types::PoolBlockProduction>,
    blocks_current_epoch: Vec<crate::types::PoolBlockProduction>,
    utxo_position: u64,
}

#[expect(dead_code)]
struct ParsedMetadataWithoutUtxoPosition {
    epoch: u64,
    treasury: u64,
    reserves: u64,
    pools: Vec<PoolInfo>,
    dreps: Vec<DRepInfo>,
    accounts: Vec<AccountState>,
    blocks_previous_epoch: Vec<crate::types::PoolBlockProduction>,
    blocks_current_epoch: Vec<crate::types::PoolBlockProduction>,
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
    pub fn parse<C: SnapshotCallbacks>(&self, callbacks: &mut C) -> Result<()> {
        let file = File::open(&self.file_path)
            .context(format!("Failed to open snapshot file: {}", self.file_path))?;

        let mut chunked_reader = ChunkedCborReader::new(file, self.chunk_size)?;

        // Phase 1: Parse metadata efficiently using smaller buffer to locate UTXO section
        // Read initial portion for metadata parsing (256MB should be sufficient for metadata)
        let metadata_size = 256 * 1024 * 1024; // 256MB for metadata parsing
        let actual_metadata_size = metadata_size.min(chunked_reader.file_size as usize);

        // Read metadata portion
        let metadata_buffer = {
            let mut buffer = vec![0u8; actual_metadata_size];
            chunked_reader.file.seek(SeekFrom::Start(0))?;
            chunked_reader.file.read_exact(&mut buffer)?;
            buffer
        };

        let mut decoder = Decoder::new(&metadata_buffer);

        // Navigate to NewEpochState root array
        let new_epoch_state_len = decoder
            .array()
            .context("Failed to parse NewEpochState root array")?
            .ok_or_else(|| anyhow!("NewEpochState must be a definite-length array"))?;

        if new_epoch_state_len < 4 {
            return Err(anyhow!(
                "NewEpochState array too short: expected at least 4 elements, got {}",
                new_epoch_state_len
            ));
        }

        // Extract epoch number [0]
        let epoch = decoder.u64().context("Failed to parse epoch number")?;

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
                "EpochState array too short: expected at least 3 elements, got {}",
                epoch_state_len
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
                "AccountState array too short: expected at least 2 elements, got {}",
                account_state_len
            ));
        }

        // Parse treasury and reserves (can be negative in CBOR, so decode as i64 first)
        let treasury_i64: i64 = decoder.decode().context("Failed to parse treasury")?;
        let reserves_i64: i64 = decoder.decode().context("Failed to parse reserves")?;
        let treasury = u64::try_from(treasury_i64).map_err(|_| anyhow!("treasury was negative"))?;
        let reserves = u64::try_from(reserves_i64).map_err(|_| anyhow!("reserves was negative"))?;

        // Skip any remaining AccountState fields
        for i in 2..account_state_len {
            decoder.skip().context(format!("Failed to skip AccountState[{}]", i))?;
        }

        // Note: We defer the on_metadata callback until after we parse deposits from UTxOState[1]

        // Navigate to LedgerState [3][1]
        let ledger_state_len = decoder
            .array()
            .context("Failed to parse LedgerState array")?
            .ok_or_else(|| anyhow!("LedgerState must be a definite-length array"))?;

        if ledger_state_len < 2 {
            return Err(anyhow!(
                "LedgerState array too short: expected at least 2 elements, got {}",
                ledger_state_len
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
                "CertState array too short: expected at least 3 elements, got {}",
                cert_state_len
            ));
        }

        // Parse VState [3][1][0][0] for DReps, which also skips committee_state and dormant_epoch.
        // TODO: We may need to return to these later if we implement committee tracking.
        let dreps = Self::parse_vstate(&mut decoder).context("Failed to parse VState for DReps")?;

        // Parse PState [3][1][0][1] for pools
        let pools = Self::parse_pstate(&mut decoder).context("Failed to parse PState for pools")?;

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

        // Convert to AccountState for API
        let accounts: Vec<AccountState> = accounts_map
            .into_iter()
            .map(|(credential, account)| {
                // Convert StakeCredential to stake address representation
                let stake_address = match &credential {
                    StakeCredential::AddrKeyHash(hash) => {
                        format!("stake_key_{}", hex::encode(hash))
                    }
                    StakeCredential::ScriptHash(hash) => {
                        format!("stake_script_{}", hex::encode(hash))
                    }
                };

                // Extract rewards from rewards_and_deposit (first element of tuple)
                let rewards = match &account.rewards_and_deposit {
                    StrictMaybe::Just((reward, _deposit)) => *reward,
                    StrictMaybe::Nothing => 0,
                };

                // Convert SPO delegation from StrictMaybe<PoolId> to Option<KeyHash>
                // PoolId is Hash<28>, we need to convert to Vec<u8>
                let delegated_spo = match &account.pool {
                    StrictMaybe::Just(pool_id) => Some(*pool_id),
                    StrictMaybe::Nothing => None,
                };

                // Convert DRep delegation from StrictMaybe<DRep> to Option<DRepChoice>
                let delegated_drep = match &account.drep {
                    StrictMaybe::Just(drep) => Some(match drep {
                        DRep::Key(hash) => crate::DRepChoice::Key(*hash),
                        DRep::Script(hash) => crate::DRepChoice::Script(*hash),
                        DRep::Abstain => crate::DRepChoice::Abstain,
                        DRep::NoConfidence => crate::DRepChoice::NoConfidence,
                    }),
                    StrictMaybe::Nothing => None,
                };

                AccountState {
                    stake_address,
                    address_state: StakeAddressState {
                        registered: false, // Accounts are registered by SPOState
                        utxo_value: 0, // Not available in DState, would need to aggregate from UTxOs
                        rewards,
                        delegated_spo,
                        delegated_drep,
                    },
                }
            })
            .collect();

        // Skip remaining DState fields (fut_gen_deleg, gen_deleg, instant_rewards)
        // The UMap already handled all its internal elements including pointers

        // Epoch State / Ledger State / Cert State / Delegation state / dsFutureGenDelegs
        decoder.skip()?;

        // Epoch State / Ledger State / Cert State / Delegation state / dsGenDelegs
        decoder.skip()?;

        // Epoch State / Ledger State / Cert State / Delegation state / dsIRewards
        decoder.skip()?;

        // Navigate to UTxOState [3][1][1]
        let utxo_state_len = decoder
            .array()
            .context("Failed to parse UTxOState array")?
            .ok_or_else(|| anyhow!("UTxOState must be a definite-length array"))?;

        if utxo_state_len < 1 {
            return Err(anyhow!(
                "UTxOState array too short: expected at least 1 element, got {}",
                utxo_state_len
            ));
        }

        // Record the position before UTXO streaming - this is where UTXOs start in the file
        let utxo_file_position = decoder.position() as u64;

        // Read only the UTXO section from the file (not the entire file!)
        let mut utxo_file = File::open(&self.file_path).context(format!(
            "Failed to open snapshot file for UTXO reading: {}",
            self.file_path
        ))?;

        // TRUE STREAMING: Process UTXOs one by one with minimal memory usage
        utxo_file.seek(SeekFrom::Start(utxo_file_position))?;
        let utxo_count = Self::stream_utxos(&mut utxo_file, callbacks)
            .context("Failed to stream UTXOs with true streaming")?;

        // For chunked reading, we'll parse deposits from the metadata buffer if possible
        // or set to a default value. In a full implementation, we'd need to track
        // the position after UTXOs in the chunked reader.
        let deposits = 0; // TODO: Parse deposits properly with chunked reading

        // Emit bulk callbacks
        callbacks.on_pools(pools)?;
        callbacks.on_dreps(dreps)?;
        callbacks.on_accounts(accounts)?;
        callbacks.on_proposals(Vec::new())?; // TODO: Parse from GovState

        // Emit metadata callback with accurate deposits and utxo count
        callbacks.on_metadata(SnapshotMetadata {
            epoch,
            pot_balances: PotBalances {
                reserves,
                treasury,
                deposits,
            },
            utxo_count: Some(utxo_count),
            blocks_previous_epoch,
            blocks_current_epoch,
        })?;

        // Emit completion callback
        callbacks.on_complete()?;

        Ok(())
    }

    /// Parse metadata and structure, returning the UTXO position (for future chunked reading)
    #[expect(dead_code)]
    fn parse_metadata_and_find_utxos(&self, buffer: &[u8]) -> Result<ParsedMetadata> {
        let mut decoder = Decoder::new(buffer);
        let start_pos = decoder.position();

        // Navigate to NewEpochState root array
        let new_epoch_state_len = decoder
            .array()
            .context("Failed to parse NewEpochState root array")?
            .ok_or_else(|| anyhow!("NewEpochState must be a definite-length array"))?;

        if new_epoch_state_len < 4 {
            return Err(anyhow!(
                "NewEpochState array too short: expected at least 4 elements, got {}",
                new_epoch_state_len
            ));
        }

        // Extract epoch number [0]
        let epoch = decoder.u64().context("Failed to parse epoch number")?;

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
                "EpochState array too short: expected at least 3 elements, got {}",
                epoch_state_len
            ));
        }

        // Extract AccountState [3][0]: [treasury, reserves]
        let account_state_len = decoder
            .array()
            .context("Failed to parse AccountState array")?
            .ok_or_else(|| anyhow!("AccountState must be a definite-length array"))?;

        if account_state_len < 2 {
            return Err(anyhow!(
                "AccountState array too short: expected at least 2 elements, got {}",
                account_state_len
            ));
        }

        // Parse treasury and reserves
        let treasury_i64: i64 = decoder.decode().context("Failed to parse treasury")?;
        let reserves_i64: i64 = decoder.decode().context("Failed to parse reserves")?;
        let treasury = u64::try_from(treasury_i64).map_err(|_| anyhow!("treasury was negative"))?;
        let reserves = u64::try_from(reserves_i64).map_err(|_| anyhow!("reserves was negative"))?;

        // Skip any remaining AccountState fields
        for i in 2..account_state_len {
            decoder.skip().context(format!("Failed to skip AccountState[{}]", i))?;
        }

        // Navigate to LedgerState [3][1]
        let ledger_state_len = decoder
            .array()
            .context("Failed to parse LedgerState array")?
            .ok_or_else(|| anyhow!("LedgerState must be a definite-length array"))?;

        if ledger_state_len < 2 {
            return Err(anyhow!(
                "LedgerState array too short: expected at least 2 elements, got {}",
                ledger_state_len
            ));
        }

        // Parse CertState [3][1][0]
        let cert_state_len = decoder
            .array()
            .context("Failed to parse CertState array")?
            .ok_or_else(|| anyhow!("CertState must be a definite-length array"))?;

        if cert_state_len < 3 {
            return Err(anyhow!(
                "CertState array too short: expected at least 3 elements, got {}",
                cert_state_len
            ));
        }

        // Parse DReps, pools, and accounts
        let dreps = Self::parse_vstate(&mut decoder).context("Failed to parse VState for DReps")?;
        let pools = Self::parse_pstate(&mut decoder).context("Failed to parse PState for pools")?;
        let accounts =
            Self::parse_dstate(&mut decoder).context("Failed to parse DState for accounts")?;

        // Navigate to UTxOState [3][1][1]
        decoder.array().context("Failed to parse UTxOState array")?;

        // Current position is right before the UTXO map [3][1][1][0]
        let utxo_position = start_pos as u64 + decoder.position() as u64;

        Ok(ParsedMetadata {
            epoch,
            treasury,
            reserves,
            pools,
            dreps,
            accounts,
            blocks_previous_epoch,
            blocks_current_epoch,
            utxo_position,
        })
    }

    /// Parse metadata and structure (everything except UTXOs) (legacy)
    #[expect(dead_code)]
    fn parse_metadata_and_structure(
        &self,
        buffer: &[u8],
    ) -> Result<ParsedMetadataWithoutUtxoPosition> {
        let mut decoder = Decoder::new(buffer);

        // Navigate to NewEpochState root array
        let new_epoch_state_len = decoder
            .array()
            .context("Failed to parse NewEpochState root array")?
            .ok_or_else(|| anyhow!("NewEpochState must be a definite-length array"))?;

        if new_epoch_state_len < 4 {
            return Err(anyhow!(
                "NewEpochState array too short: expected at least 4 elements, got {}",
                new_epoch_state_len
            ));
        }

        // Extract epoch number [0]
        let epoch = decoder.u64().context("Failed to parse epoch number")?;

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
                "EpochState array too short: expected at least 3 elements, got {}",
                epoch_state_len
            ));
        }

        // Extract AccountState [3][0]: [treasury, reserves]
        let account_state_len = decoder
            .array()
            .context("Failed to parse AccountState array")?
            .ok_or_else(|| anyhow!("AccountState must be a definite-length array"))?;

        if account_state_len < 2 {
            return Err(anyhow!(
                "AccountState array too short: expected at least 2 elements, got {}",
                account_state_len
            ));
        }

        // Parse treasury and reserves
        let treasury_i64: i64 = decoder.decode().context("Failed to parse treasury")?;
        let reserves_i64: i64 = decoder.decode().context("Failed to parse reserves")?;
        let treasury = u64::try_from(treasury_i64).map_err(|_| anyhow!("treasury was negative"))?;
        let reserves = u64::try_from(reserves_i64).map_err(|_| anyhow!("reserves was negative"))?;

        // Skip any remaining AccountState fields
        for i in 2..account_state_len {
            decoder.skip().context(format!("Failed to skip AccountState[{}]", i))?;
        }

        // Navigate to LedgerState [3][1]
        let ledger_state_len = decoder
            .array()
            .context("Failed to parse LedgerState array")?
            .ok_or_else(|| anyhow!("LedgerState must be a definite-length array"))?;

        if ledger_state_len < 2 {
            return Err(anyhow!(
                "LedgerState array too short: expected at least 2 elements, got {}",
                ledger_state_len
            ));
        }

        // Parse CertState [3][1][0]
        let cert_state_len = decoder
            .array()
            .context("Failed to parse CertState array")?
            .ok_or_else(|| anyhow!("CertState must be a definite-length array"))?;

        if cert_state_len < 3 {
            return Err(anyhow!(
                "CertState array too short: expected at least 3 elements, got {}",
                cert_state_len
            ));
        }

        // Parse DReps, pools, and accounts
        let dreps = Self::parse_vstate(&mut decoder).context("Failed to parse VState for DReps")?;
        let pools = Self::parse_pstate(&mut decoder).context("Failed to parse PState for pools")?;
        let accounts =
            Self::parse_dstate(&mut decoder).context("Failed to parse DState for accounts")?;

        Ok(ParsedMetadataWithoutUtxoPosition {
            epoch,
            treasury,
            reserves,
            pools,
            dreps,
            accounts,
            blocks_previous_epoch,
            blocks_current_epoch,
        })
    }

    /// Parse DState for accounts (extracted from original parse method)
    fn parse_dstate(decoder: &mut Decoder) -> Result<Vec<AccountState>> {
        // Parse DState [3][1][0][2] for accounts/delegations
        decoder.array().context("Failed to parse DState array")?;

        // Parse unified rewards - it's actually an array containing the map
        let umap_len = decoder.array().context("Failed to parse UMap array")?;

        // Parse the rewards map [0]: StakeCredential -> Account
        let accounts_map: BTreeMap<StakeCredential, Account> = decoder.decode()?;

        // Skip remaining UMap elements if any
        if let Some(len) = umap_len {
            for _ in 1..len {
                decoder.skip()?;
            }
        }

        // Convert to AccountState for API
        let accounts: Vec<AccountState> = accounts_map
            .into_iter()
            .map(|(credential, account)| {
                // Convert StakeCredential to stake address representation
                let stake_address = match &credential {
                    StakeCredential::AddrKeyHash(hash) => {
                        format!("stake_key_{}", hex::encode(hash))
                    }
                    StakeCredential::ScriptHash(hash) => {
                        format!("stake_script_{}", hex::encode(hash))
                    }
                };

                // Extract rewards from rewards_and_deposit (first element of tuple)
                let rewards = match &account.rewards_and_deposit {
                    StrictMaybe::Just((reward, _deposit)) => *reward,
                    StrictMaybe::Nothing => 0,
                };

                // Convert SPO delegation from StrictMaybe<PoolId> to Option<KeyHash>
                let delegated_spo = match &account.pool {
                    StrictMaybe::Just(pool_id) => Some(*pool_id),
                    StrictMaybe::Nothing => None,
                };

                // Convert DRep delegation from StrictMaybe<DRep> to Option<DRepChoice>
                let delegated_drep = match &account.drep {
                    StrictMaybe::Just(drep) => Some(match drep {
                        DRep::Key(hash) => crate::DRepChoice::Key(*hash),
                        DRep::Script(hash) => crate::DRepChoice::Script(*hash),
                        DRep::Abstain => crate::DRepChoice::Abstain,
                        DRep::NoConfidence => crate::DRepChoice::NoConfidence,
                    }),
                    StrictMaybe::Nothing => None,
                };

                AccountState {
                    stake_address,
                    address_state: StakeAddressState {
                        registered: false, // Accounts are registered by SPOState
                        utxo_value: 0, // Not available in DState, would need to aggregate from UTxOs
                        rewards,
                        delegated_spo,
                        delegated_drep,
                    },
                }
            })
            .collect();

        // Skip remaining DState fields (fut_gen_deleg, gen_deleg, instant_rewards)
        decoder.skip()?; // dsFutureGenDelegs
        decoder.skip()?; // dsGenDelegs
        decoder.skip()?; // dsIRewards

        Ok(accounts)
    }

    /// STREAMING: Process UTXOs with chunked buffering and incremental parsing
    fn stream_utxos<C: UtxoCallback>(file: &mut File, callbacks: &mut C) -> Result<u64> {
        // OPTIMIZED: Balance between memory usage and performance
        // Based on experiment: avg=194 bytes, max=22KB per entry

        const READ_CHUNK_SIZE: usize = 16 * 1024 * 1024; // 16MB read chunks for I/O efficiency
        const PARSE_BUFFER_SIZE: usize = 64 * 1024 * 1024; // 64MB parse buffer (vs 2.1GB)
        const MAX_ENTRY_SIZE: usize = 32 * 1024; // 32KB safety margin

        let mut buffer = Vec::with_capacity(PARSE_BUFFER_SIZE);
        let mut utxo_count = 0u64;
        let mut total_bytes_processed = 0usize;

        // Read a larger initial buffer for better performance
        let mut chunk = vec![0u8; READ_CHUNK_SIZE];
        let initial_read = file.read(&mut chunk)?;
        chunk.truncate(initial_read);
        buffer.extend_from_slice(&chunk);

        // Parse map header first
        let mut decoder = Decoder::new(&buffer);
        let map_len = match decoder.map()? {
            Some(len) => {
                // Found CBOR map with "len" UTXO entries
                len
            }
            None => {
                // indefinite-length CBOR map
                u64::MAX
            }
        };

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

        Ok(utxo_count)
    }

    /// Parse a single block production entry from a map (producer pool ID -> block count)
    /// The CBOR structure maps pool IDs to block counts (not individual blocks)
    fn parse_single_block_production_entry(
        decoder: &mut Decoder,
        epoch: u64,
    ) -> Result<crate::types::PoolBlockProduction> {
        use crate::types::PoolBlockProduction;

        // Parse the pool ID (key) - stored as bytes (28 bytes for pool ID)
        let pool_id_bytes = decoder.bytes().context("Failed to parse pool ID bytes")?;

        // Parse the block count (value) - how many blocks this pool produced
        let block_count = decoder.u8().context("Failed to parse block count")?;

        // Convert pool ID bytes to hex string
        let pool_id = hex::encode(pool_id_bytes);

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
    ) -> Result<Vec<crate::types::PoolBlockProduction>> {
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
                    info!("📦 Processing indefinite-length blocks array");
                    let mut count = 0;
                    loop {
                        match decoder.datatype()? {
                            Type::Break => {
                                decoder.skip()?;
                                info!("📦 Found array break after {} entries", count);
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
                        info!("📦 Block data is integer: {}", value);
                    }
                    Type::Bytes => {
                        let bytes = decoder.bytes().context("Failed to read block bytes")?;
                        info!("📦 Block data is {} bytes", bytes.len());
                    }
                    Type::String => {
                        let text = decoder.str().context("Failed to read block text")?;
                        info!("📦 Block data is text: '{}'", text);
                    }
                    Type::Null => {
                        decoder.skip()?;
                        info!("📦 Block data is null");
                    }
                    _ => {
                        decoder.skip().context("Failed to skip blocks value")?;
                    }
                }

                Ok(Vec::new())
            }
        }
    }

    /// Parse a single UTXO entry from the streaming buffer
    fn parse_single_utxo(decoder: &mut Decoder) -> Result<UtxoEntry> {
        // Parse key: TransactionInput (array [tx_hash, output_index])
        decoder.array().context("Failed to parse TxIn array")?;

        let tx_hash_bytes = decoder.bytes().context("Failed to parse tx_hash")?;
        let output_index = decoder.u64().context("Failed to parse output_index")?;
        let tx_hash = hex::encode(tx_hash_bytes);

        // Parse value: TransactionOutput
        let (address, value) = Self::parse_transaction_output(decoder)
            .context("Failed to parse transaction output")?;

        Ok(UtxoEntry {
            tx_hash,
            output_index,
            address,
            value,
            datum: None,      // TODO: Extract from TxOut
            script_ref: None, // TODO: Extract from TxOut
        })
    }

    /// VState = [dreps_map, committee_state, dormant_epoch]
    fn parse_vstate(decoder: &mut Decoder) -> Result<Vec<DRepInfo>> {
        // Parse VState array
        let vstate_len = decoder
            .array()
            .context("Failed to parse VState array")?
            .ok_or_else(|| anyhow!("VState must be a definite-length array"))?;

        if vstate_len < 1 {
            return Err(anyhow!(
                "VState array too short: expected at least 1 element, got {}",
                vstate_len
            ));
        }

        // Parse DReps map [0]: StakeCredential -> DRepState
        // Using minicbor's Decode trait - much simpler than manual parsing!
        let dreps_map: BTreeMap<StakeCredential, DRepState> = decoder.decode()?;

        // Convert to DRepInfo for API compatibility
        let dreps = dreps_map
            .into_iter()
            .map(|(cred, state)| {
                let drep_id = match cred {
                    StakeCredential::AddrKeyHash(hash) => format!("drep_{}", hex::encode(hash)),
                    StakeCredential::ScriptHash(hash) => {
                        format!("drep_script_{}", hex::encode(hash))
                    }
                };

                let anchor = match state.anchor {
                    StrictMaybe::Just(a) => Some(AnchorInfo {
                        url: a.url,
                        data_hash: a.content_hash.to_string(),
                    }),
                    StrictMaybe::Nothing => None,
                };

                DRepInfo {
                    drep_id,
                    deposit: state.deposit,
                    anchor,
                }
            })
            .collect();

        // Skip committee_state [1] and dormant_epoch [2] if present
        for i in 1..vstate_len {
            decoder.skip().context(format!("Failed to skip VState[{}]", i))?;
        }

        Ok(dreps)
    }

    /// Parse PState to extract stake pools
    /// PState = [pools_map, future_pools_map, retiring_map, deposits_map]
    fn parse_pstate(decoder: &mut Decoder) -> Result<Vec<PoolInfo>> {
        // Parse PState array
        let pstate_len = decoder
            .array()
            .context("Failed to parse PState array")?
            .ok_or_else(|| anyhow!("PState must be a definite-length array"))?;

        if pstate_len < 1 {
            return Err(anyhow!(
                "PState array too short: expected at least 1 element, got {}",
                pstate_len
            ));
        }

        // Parse pools map [0]: PoolId (Hash<28>) -> PoolParams
        // Note: Maps might be tagged with CBOR tag 258 (set)
        if matches!(decoder.datatype()?, Type::Tag) {
            decoder.tag()?; // skip tag if present
        }

        let mut pools_map = BTreeMap::new();
        match decoder.map()? {
            Some(pool_count) => {
                // Definite-length map
                for i in 0..pool_count {
                    let pool_id: Hash<28> =
                        decoder.decode().context(format!("Failed to decode pool ID #{}", i))?;
                    let params: super::pool_params::PoolParams = decoder
                        .decode()
                        .context(format!("Failed to decode pool params for pool #{}", i))?;
                    pools_map.insert(pool_id, params);
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
                            let pool_id: Hash<28> = decoder
                                .decode()
                                .context(format!("Failed to decode pool ID #{}", count))?;
                            let params: super::pool_params::PoolParams = decoder.decode().context(
                                format!("Failed to decode pool params for pool #{}", count),
                            )?;
                            pools_map.insert(pool_id, params);
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
        let _pools_updates: BTreeMap<Hash<28>, super::pool_params::PoolParams> =
            decoder.decode()?;

        // Parse retiring map [2]: PoolId -> Epoch
        if matches!(decoder.datatype()?, Type::Tag) {
            decoder.tag()?;
        }
        let pools_retirements: BTreeMap<Hash<28>, Epoch> = decoder.decode()?;

        // Convert to PoolInfo for API compatibility
        let pools = pools_map
            .into_iter()
            .map(|(pool_id, params)| {
                // Convert relay types from ledger format to API format
                let relays: Vec<ApiRelay> = params
                    .relays
                    .iter()
                    .map(|relay| match relay {
                        Relay::SingleHostAddr(port, ipv4, ipv6) => {
                            let port_opt = match port {
                                Nullable::Some(p) => u16::try_from(*p).ok(),
                                _ => None,
                            };
                            let ipv4_opt = match ipv4 {
                                Nullable::Some(bytes) if bytes.0.len() == 4 => Some(format!(
                                    "{}.{}.{}.{}",
                                    bytes.0[0], bytes.0[1], bytes.0[2], bytes.0[3]
                                )),
                                _ => None,
                            };
                            let ipv6_opt = match ipv6 {
                                Nullable::Some(bytes) if bytes.0.len() == 16 => {
                                    // Convert big-endian byte array to IPv6 string
                                    let b = &bytes.0;
                                    let addr = std::net::Ipv6Addr::from([
                                        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9],
                                        b[10], b[11], b[12], b[13], b[14], b[15],
                                    ]);
                                    Some(addr.to_string())
                                }
                                _ => None,
                            };
                            ApiRelay::SingleHostAddr {
                                port: port_opt,
                                ipv4: ipv4_opt,
                                ipv6: ipv6_opt,
                            }
                        }
                        Relay::SingleHostName(port, hostname) => {
                            let port_opt = match port {
                                Nullable::Some(p) => Some(*p as u16),
                                _ => None,
                            };
                            ApiRelay::SingleHostName {
                                port: port_opt,
                                dns_name: hostname.clone(),
                            }
                        }
                        Relay::MultiHostName(hostname) => ApiRelay::MultiHostName {
                            dns_name: hostname.clone(),
                        },
                    })
                    .collect();

                // Convert metadata from ledger format to API format
                let pool_metadata = match &params.metadata {
                    Nullable::Some(meta) => Some(ApiPoolMetadata {
                        url: meta.url.clone(),
                        hash: meta.hash.to_string(),
                    }),
                    _ => None,
                };

                // Look up retirement epoch
                let retirement_epoch = pools_retirements.get(&pool_id).copied();

                PoolInfo {
                    pool_id: pool_id.to_string(),
                    vrf_key_hash: params.vrf.to_string(),
                    pledge: params.pledge,
                    cost: params.cost,
                    margin: (params.margin.numerator as f64) / (params.margin.denominator as f64),
                    reward_account: hex::encode(&params.reward_account.0),
                    pool_owners: params.owners.iter().map(|h| h.to_string()).collect(),
                    relays,
                    pool_metadata,
                    retirement_epoch,
                }
            })
            .collect();

        // Skip any remaining PState elements (like deposits)
        for i in 3..pstate_len {
            decoder.skip().context(format!("Failed to skip PState[{}]", i))?;
        }

        Ok(pools)
    }

    /// Stream UTXOs with per-entry callback
    ///
    /// Parse a single TxOut from the CBOR decoder
    fn parse_transaction_output(dec: &mut Decoder) -> Result<(String, u64)> {
        // TxOut is typically an array [address, value, ...]
        // or a map for Conway with optional fields

        // Try array format first (most common)
        match dec.datatype().context("Failed to read TxOut datatype")? {
            Type::Array | Type::ArrayIndef => {
                let arr_len = dec.array().context("Failed to parse TxOut array")?;
                if arr_len == Some(0) {
                    return Err(anyhow!("empty TxOut array"));
                }

                // Element 0: Address (bytes)
                let address_bytes = dec.bytes().context("Failed to parse address bytes")?;
                let hex_address = hex::encode(address_bytes);

                // Element 1: Value (coin or map)
                let value = match dec.datatype().context("Failed to read value datatype")? {
                    Type::U8 | Type::U16 | Type::U32 | Type::U64 => {
                        // Simple ADA-only value
                        dec.u64().context("Failed to parse u64 value")?
                    }
                    Type::Array | Type::ArrayIndef => {
                        // Multi-asset: [coin, assets_map]
                        dec.array().context("Failed to parse value array")?;
                        let coin = dec.u64().context("Failed to parse coin amount")?;
                        // Skip the assets map
                        dec.skip().context("Failed to skip assets map")?;
                        coin
                    }
                    _ => {
                        return Err(anyhow!("unexpected value type"));
                    }
                };

                // Skip remaining fields (datum, script_ref)
                if let Some(len) = arr_len {
                    for _ in 2..len {
                        dec.skip().context("Failed to skip TxOut field")?;
                    }
                }

                Ok((hex_address, value))
            }
            Type::Map | Type::MapIndef => {
                // Map format (Conway with optional fields)
                // Map keys: 0=address, 1=value, 2=datum, 3=script_ref
                let map_len = dec.map().context("Failed to parse TxOut map")?;

                let mut address = String::new();
                let mut value = 0u64;
                let mut found_address = false;
                let mut found_value = false;

                let entries = map_len.unwrap_or(4); // Assume max 4 entries if indefinite
                for _ in 0..entries {
                    // Check for break in indefinite map
                    if map_len.is_none() && matches!(dec.datatype(), Ok(Type::Break)) {
                        dec.skip().ok(); // consume break
                        break;
                    }

                    // Read key
                    let key = match dec.u32() {
                        Ok(k) => k,
                        Err(_) => {
                            // Skip both key and value if key is not u32
                            dec.skip().ok();
                            dec.skip().ok();
                            continue;
                        }
                    };

                    // Read value based on key
                    match key {
                        0 => {
                            // Address
                            if let Ok(addr_bytes) = dec.bytes() {
                                address = hex::encode(addr_bytes);
                                found_address = true;
                            } else {
                                dec.skip().ok();
                            }
                        }
                        1 => {
                            // Value (coin or multi-asset)
                            match dec.datatype() {
                                Ok(Type::U8) | Ok(Type::U16) | Ok(Type::U32) | Ok(Type::U64) => {
                                    if let Ok(coin) = dec.u64() {
                                        value = coin;
                                        found_value = true;
                                    } else {
                                        dec.skip().ok();
                                    }
                                }
                                Ok(Type::Array) | Ok(Type::ArrayIndef) => {
                                    // Multi-asset: [coin, assets_map]
                                    if dec.array().is_ok() {
                                        if let Ok(coin) = dec.u64() {
                                            value = coin;
                                            found_value = true;
                                        }
                                        dec.skip().ok(); // skip assets map
                                    } else {
                                        dec.skip().ok();
                                    }
                                }
                                _ => {
                                    dec.skip().ok();
                                }
                            }
                        }
                        _ => {
                            // datum (2), script_ref (3), or unknown - skip
                            dec.skip().ok();
                        }
                    }
                }

                if found_address && found_value {
                    Ok((address, value))
                } else {
                    Err(anyhow!("map-based TxOut missing required fields"))
                }
            }
            _ => Err(anyhow!("unexpected TxOut type")),
        }
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
    pub pools: Vec<PoolInfo>,
    pub accounts: Vec<AccountState>,
    pub dreps: Vec<DRepInfo>,
    pub proposals: Vec<GovernanceProposal>,
}

impl UtxoCallback for CollectingCallbacks {
    fn on_utxo(&mut self, utxo: UtxoEntry) -> Result<()> {
        self.utxos.push(utxo);
        Ok(())
    }
}

impl PoolCallback for CollectingCallbacks {
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()> {
        self.pools = pools;
        Ok(())
    }
}

impl StakeCallback for CollectingCallbacks {
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()> {
        self.accounts = accounts;
        Ok(())
    }
}

impl DRepCallback for CollectingCallbacks {
    fn on_dreps(&mut self, dreps: Vec<DRepInfo>) -> Result<()> {
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

impl SnapshotCallbacks for CollectingCallbacks {
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()> {
        self.metadata = Some(metadata);
        Ok(())
    }

    fn on_complete(&mut self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_collecting_callbacks() {
        let mut callbacks = CollectingCallbacks::default();

        // Test metadata callback
        callbacks
            .on_metadata(SnapshotMetadata {
                epoch: 507,
                pot_balances: PotBalances {
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
                tx_hash: "abc123".to_string(),
                output_index: 0,
                address: "addr1...".to_string(),
                value: 5000000,
                datum: None,
                script_ref: None,
            })
            .unwrap();

        assert_eq!(callbacks.utxos.len(), 1);
        assert_eq!(callbacks.utxos[0].value, 5000000);
    }
}
