//! Core type definitions for Acropolis
// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]

use crate::certificate::TxCertificateIdentifier;
use crate::crypto::keyhash_224;
use crate::drep::{Anchor, DRepVotingThresholds};
use crate::script::Datum;
use crate::UTxOIdentifier;
// Re-export certificate types for backward compatibility
pub use crate::certificate::{
    AuthCommitteeHot, CommitteeCredential, Deregistration, GenesisKeyDelegation,
    InstantaneousRewardSource, InstantaneousRewardTarget, MoveInstantaneousReward,
    PoolRegistration, PoolRetirement, Registration, ResignCommitteeCold, StakeAndVoteDelegation,
    StakeDelegation, StakeRegistrationAndDelegation, StakeRegistrationAndStakeAndVoteDelegation,
    StakeRegistrationAndVoteDelegation, TxCertificate, TxCertificateWithPos, VoteDelegation,
};
use crate::hash::Hash;
use crate::serialization::Bech32Conversion;
use crate::{
    address::{Address, ShelleyAddress, StakeAddress},
    declare_hash_type, declare_hash_type_with_bech32, protocol_params,
    rational_number::RationalNumber,
};
use anyhow::{anyhow, bail, Context, Error, Result};
use bech32::{Bech32, Hrp};
use bitmask_enum::bitmask;
use chrono::{DateTime, NaiveDateTime, Utc};
use hex::decode;
use regex::Regex;
use serde::de::Error as SerdeError;
use serde::{Deserialize, Deserializer, Serialize};
use serde_with::{hex::Hex, serde_as};
use std::collections::BTreeMap;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::ops::Add;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fmt,
    fmt::{Display, Formatter},
    ops::{AddAssign, Neg},
    str::FromStr,
};
use tracing::error;

/// Network identifier
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Ord,
    PartialOrd,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub enum NetworkId {
    /// Main
    #[n(0)]
    #[default]
    Mainnet,

    /// Test
    #[n(1)]
    Testnet,
}

impl From<String> for NetworkId {
    fn from(s: String) -> Self {
        match s.as_str() {
            "testnet" => NetworkId::Testnet,
            "mainnet" => NetworkId::Mainnet,
            _ => NetworkId::Mainnet,
        }
    }
}

impl Display for NetworkId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            match self {
                NetworkId::Mainnet => "mainnet",
                NetworkId::Testnet => "testnet",
            }
        )
    }
}

/// Protocol era
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum Era {
    #[default]
    Byron,
    Shelley,
    Allegra,
    Mary,
    Alonzo,
    Babbage,
    Conway,
}

impl From<Era> for u8 {
    fn from(e: Era) -> u8 {
        match e {
            Era::Byron => 0,
            Era::Shelley => 1,
            Era::Allegra => 2,
            Era::Mary => 3,
            Era::Alonzo => 4,
            Era::Babbage => 5,
            Era::Conway => 6,
        }
    }
}

impl TryFrom<u8> for Era {
    type Error = anyhow::Error;
    fn try_from(v: u8) -> Result<Era, Error> {
        match v {
            0 => Ok(Era::Byron),
            1 => Ok(Era::Shelley),
            2 => Ok(Era::Allegra),
            3 => Ok(Era::Mary),
            4 => Ok(Era::Alonzo),
            5 => Ok(Era::Babbage),
            6 => Ok(Era::Conway),
            n => bail!("Impossible era {n}"),
        }
    }
}

impl Display for Era {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

/// Block production statistics for a stake pool in a specific epoch
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PoolBlockProduction {
    /// Pool ID that produced the blocks
    pub pool_id: PoolId,

    /// Number of blocks produced by this pool in the epoch
    pub block_count: u8,

    /// Epoch number
    pub epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct EpochBootstrapData {
    /// Current epoch number
    pub epoch: u64,
    /// Pool ID (hex) → block count
    pub spo_blocks_previous: HashMap<PoolId, u64>,
    /// Pool ID (hex) → block count
    pub spo_blocks_current: HashMap<PoolId, u64>,
    /// Sum of current epoch blocks
    pub total_blocks_current: u64,
    /// Sum of previous epoch blocks
    pub total_blocks_previous: u64,
    /// Total fees accumulated in the epoch
    pub total_fees_current: u64,
}

impl EpochBootstrapData {
    pub fn new(
        epoch: u64,
        blocks_previous_epoch: &[crate::types::PoolBlockProduction],
        blocks_current_epoch: &[crate::types::PoolBlockProduction],
        total_fees_current: u64,
    ) -> Self {
        let blocks_previous: HashMap<PoolId, u64> =
            blocks_previous_epoch.iter().map(|p| (p.pool_id, p.block_count as u64)).collect();

        let blocks_current: HashMap<PoolId, u64> =
            blocks_current_epoch.iter().map(|p| (p.pool_id, p.block_count as u64)).collect();

        let total_previous = blocks_previous.values().sum();
        let total_current = blocks_current.values().sum();

        Self {
            epoch,
            spo_blocks_previous: blocks_previous,
            spo_blocks_current: blocks_current,
            total_blocks_current: total_current,
            total_blocks_previous: total_previous,
            total_fees_current,
        }
    }
}

/// Block status
#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BlockStatus {
    Bootstrap,  // Pseudo-block from bootstrap data
    Immutable,  // Now immutable (more than 'k' blocks ago)
    Volatile,   // Volatile, in sequence
    RolledBack, // Volatile, restarted after rollback
}

/// Block status
#[bitmask(u8)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum BlockIntent {
    Validate = 0b00000001, // Just validate the block
    Apply = 0b00000010,    // Apply the block
    ValidateAndApply = BlockIntent::Validate.bits | BlockIntent::Apply.bits, // Validate and apply block
}

impl BlockIntent {
    pub fn do_validation(&self) -> bool {
        (*self & BlockIntent::Validate) == BlockIntent::Validate
    }
}

/// Block info, shared across multiple messages
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockInfo {
    /// Block status
    pub status: BlockStatus,

    /// Block intent
    pub intent: BlockIntent,

    /// Slot number
    pub slot: u64,

    /// Block number
    pub number: u64,

    /// Block hash
    pub hash: BlockHash,

    /// Epoch number
    pub epoch: u64,

    /// Epoch slot number
    #[serde(default)]
    pub epoch_slot: u64,

    /// Does this block start a new epoch?
    pub new_epoch: bool,

    /// Does this block start a new era?
    #[serde(default)]
    pub is_new_era: bool,

    /// Which slot was the tip at when we received this block?
    #[serde(default)]
    pub tip_slot: Option<u64>,

    /// UNIX timestamp
    #[serde(default)]
    pub timestamp: u64,

    /// Protocol era
    pub era: Era,
}

impl Ord for BlockInfo {
    fn cmp(&self, other: &Self) -> Ordering {
        self.number.cmp(&other.number)
    }
}

impl PartialOrd for BlockInfo {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl BlockInfo {
    pub fn with_intent(&self, intent: BlockIntent) -> BlockInfo {
        let mut copy = self.clone();
        copy.intent = intent;
        copy
    }

    pub fn is_at_tip(&self) -> bool {
        // The slot of a newly-reported block can be later than the slot of the tip.
        // This is because the tip is the most recent slot with a _validated_ block,
        // and we can receive and propagate blocks which have not yet been validated.
        self.tip_slot.is_some_and(|s| s <= self.slot)
    }

    pub fn to_point(&self) -> Point {
        Point::Specific {
            hash: self.hash,
            slot: self.slot,
        }
    }

    pub fn to_naive_datetime(&self) -> NaiveDateTime {
        DateTime::<Utc>::from_timestamp(self.timestamp as i64, 0)
            .expect("invalid UNIX timestamp")
            .naive_utc()
    }
}

// For stake address registration/deregistration (handles deposits/refunds)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StakeRegistrationOutcome {
    Registered(Lovelace),   // New registration → deposit taken
    Deregistered(Lovelace), // Valid deregistration → refund given
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeRegistrationUpdate {
    pub cert_identifier: TxCertificateIdentifier,
    pub outcome: StakeRegistrationOutcome,
}

impl StakeRegistrationOutcome {
    pub fn deposit(&self) -> Lovelace {
        match self {
            StakeRegistrationOutcome::Registered(deposit) => *deposit,
            _ => 0,
        }
    }

    pub fn refund(&self) -> Lovelace {
        match self {
            StakeRegistrationOutcome::Deregistered(refund) => *refund,
            _ => 0,
        }
    }
}

// For pool registration/retirement (handles pool deposits)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum PoolRegistrationOutcome {
    Registered(Lovelace), // New pool → deposit taken
    Updated,              // Existing pool update → no deposit
    RetirementQueued,     // Retirement queued
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolRegistrationUpdate {
    pub cert_identifier: TxCertificateIdentifier,
    pub outcome: PoolRegistrationOutcome,
}

impl PoolRegistrationOutcome {
    pub fn deposit(&self) -> Lovelace {
        match self {
            PoolRegistrationOutcome::Registered(deposit) => *deposit,
            _ => 0,
        }
    }
}

// For DRep registration/deregistration (handles deposits/refunds)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum DRepRegistrationOutcome {
    Registered(Lovelace),   // New registration → deposit taken
    Deregistered(Lovelace), // Valid deregistration → refund given
    Updated,                // Existing update → no deposit
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepRegistrationUpdate {
    pub cert_identifier: TxCertificateIdentifier,
    pub outcome: DRepRegistrationOutcome,
}

impl DRepRegistrationOutcome {
    pub fn deposit(&self) -> Lovelace {
        match self {
            DRepRegistrationOutcome::Registered(deposit) => *deposit,
            _ => 0,
        }
    }

    pub fn refund(&self) -> Lovelace {
        match self {
            DRepRegistrationOutcome::Deregistered(refund) => *refund,
            _ => 0,
        }
    }
}

/// Individual address balance change
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AddressDelta {
    // Address involved in delta
    pub address: Address,

    // Transaction in which delta occured
    pub tx_identifier: TxIdentifier,

    // Address impacted UTxOs
    pub spent_utxos: Vec<UTxOIdentifier>,
    pub created_utxos: Vec<UTxOIdentifier>,

    // Sums of spent and created UTxOs
    pub sent: Value,
    pub received: Value,
}

/// Extended spent UTxO details for address delta messages
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SpentUTxOExtended {
    /// UTxO identifier being spent
    pub utxo: UTxOIdentifier,

    /// Hash of the transaction spending this UTxO
    pub spent_by: TxHash,
}

/// Extended created UTxO details for address delta messages
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CreatedUTxOExtended {
    /// UTxO identifier being created
    pub utxo: UTxOIdentifier,

    /// Full value of the created UTxO
    pub value: ValueMap,

    /// Datum attached to the created UTxO, if present
    pub datum: Option<Datum>,
}

/// Extended per-address balance change with UTxO-level details
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtendedAddressDelta {
    /// Address involved in delta
    pub address: Address,

    /// Transaction in which delta occurred
    pub tx_identifier: TxIdentifier,

    /// Address impacted spent and created UTxOs
    pub spent_utxos: Vec<SpentUTxOExtended>,
    pub created_utxos: Vec<CreatedUTxOExtended>,

    /// Sums of spent and created UTxOs
    pub sent: ValueMap,
    pub received: ValueMap,
}

/// Stake balance change
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeAddressDelta {
    /// Stake address
    pub stake_address: StakeAddress,

    /// Shelley addresses contributing to the delta
    pub addresses: Vec<ShelleyAddress>,

    /// The number of transactions contributing to the delta
    pub tx_count: u32,

    /// Balance change
    pub delta: i64,
}

/// Stake Address Reward change
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeRewardDelta {
    pub stake_address: StakeAddress,
    pub delta: u64,
    pub reward_type: RewardType,
    pub pool: PoolId,
}

/// Type of reward being given
#[derive(
    Debug,
    Clone,
    PartialEq,
    minicbor::Encode,
    minicbor::Decode,
    serde::Serialize,
    serde::Deserialize,
)]
pub enum RewardType {
    #[n(0)]
    Leader,
    #[n(1)]
    Member,
    #[n(2)]
    PoolRefund,
    #[n(3)]
    ProposalRefund,
}

impl fmt::Display for RewardType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RewardType::Leader => write!(f, "leader"),
            RewardType::Member => write!(f, "member"),
            RewardType::PoolRefund => write!(f, "pool_deposit_refund"),
            RewardType::ProposalRefund => write!(f, "proposal_refund"),
        }
    }
}

pub type PolicyId = Hash<28>;
pub type NativeAssets = Vec<(PolicyId, Vec<NativeAsset>)>;
pub type NativeAssetsDelta = Vec<(PolicyId, Vec<NativeAssetDelta>)>;
pub type NativeAssetsMap = HashMap<PolicyId, HashMap<AssetName, u64>>;
pub type NativeAssetsDeltaMap = HashMap<PolicyId, HashMap<AssetName, i64>>;

#[derive(
    Default,
    Debug,
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    serde::Serialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct AssetName {
    #[n(0)]
    len: u8,
    #[n(1)]
    bytes: [u8; 32],
}

impl AssetName {
    pub fn new(data: &[u8]) -> Option<Self> {
        if data.len() > 32 {
            return None;
        }
        let mut bytes = [0u8; 32];
        bytes[..data.len()].copy_from_slice(data);
        Some(Self {
            len: data.len() as u8,
            bytes,
        })
    }

    pub fn len(&self) -> usize {
        self.len as usize
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..self.len as usize]
    }
}

impl<'de> Deserialize<'de> for AssetName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        AssetName::new(s.as_bytes())
            .ok_or_else(|| SerdeError::custom("AssetName too long (max 32 bytes)"))
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct NativeAsset {
    #[n(0)]
    pub name: AssetName,
    #[n(1)]
    pub amount: u64,
}

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, minicbor::Encode, minicbor::Decode,
)]
pub struct NativeAssetDelta {
    #[n(0)]
    pub name: AssetName,
    #[n(1)]
    pub amount: i64,
}

/// Value (lovelace + multiasset)
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct Value {
    pub lovelace: u64,
    pub assets: NativeAssets,
}

impl Value {
    pub fn new(lovelace: u64, assets: NativeAssets) -> Self {
        Self { lovelace, assets }
    }

    pub fn coin(&self) -> u64 {
        self.lovelace
    }

    pub fn sum_lovelace<'a>(iter: impl Iterator<Item = &'a Value>) -> u64 {
        iter.map(|v| v.lovelace).sum()
    }

    pub fn token_amount(&self, policy_id: &PolicyId, asset_name: &AssetName) -> u64 {
        for (pid, assets) in &self.assets {
            if pid == policy_id {
                for asset in assets {
                    if &asset.name == asset_name {
                        return asset.amount;
                    }
                }
                return 0;
            }
        }
        0
    }
}

impl AddAssign<&Value> for Value {
    fn add_assign(&mut self, other: &Value) {
        self.lovelace += other.lovelace;

        for (policy_id, other_assets) in &other.assets {
            if let Some((_, existing_assets)) =
                self.assets.iter_mut().find(|(pid, _)| pid == policy_id)
            {
                for other_asset in other_assets {
                    if let Some(existing) =
                        existing_assets.iter_mut().find(|a| a.name == other_asset.name)
                    {
                        existing.amount += other_asset.amount;
                    } else {
                        existing_assets.push(other_asset.clone());
                    }
                }
            } else {
                self.assets.push((*policy_id, other_assets.clone()));
            }
        }
    }
}

impl Add for Value {
    type Output = Self;

    fn add(self, other: Self) -> Self::Output {
        let mut result = self.clone();
        result += &other;
        result
    }
}

/// Hashmap representation of Value (lovelace + multiasset)
#[derive(
    Debug,
    Default,
    Clone,
    PartialEq,
    Eq,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct ValueMap {
    #[n(0)]
    pub lovelace: u64,
    #[n(1)]
    pub assets: NativeAssetsMap,
}

impl AddAssign for ValueMap {
    fn add_assign(&mut self, other: Self) {
        self.lovelace += other.lovelace;

        for (policy, assets) in other.assets {
            let entry = self.assets.entry(policy).or_default();
            for (asset_name, amount) in assets {
                *entry.entry(asset_name).or_default() += amount;
            }
        }
    }
}

impl ValueMap {
    pub fn add_value(&mut self, other: &Value) {
        // Handle lovelace
        self.lovelace = self.lovelace.saturating_add(other.lovelace);

        // Handle multi-assets
        for (policy, assets) in &other.assets {
            let policy_entry = self.assets.entry(*policy).or_default();
            for asset in assets {
                *policy_entry.entry(asset.name).or_default() = policy_entry
                    .get(&asset.name)
                    .copied()
                    .unwrap_or(0)
                    .saturating_add(asset.amount);
            }
        }
    }

    pub fn remove_zero_amounts(&mut self) {
        self.assets.retain(|_, assets| {
            assets.retain(|_, amount| *amount != 0);
            !assets.is_empty()
        });
    }
}

impl From<&Value> for ValueMap {
    fn from(value: &Value) -> Self {
        let mut map = Self::default();
        map.add_value(value);
        map
    }
}

impl From<Value> for ValueMap {
    fn from(value: Value) -> Self {
        Self::from(&value)
    }
}

impl From<ValueMap> for Value {
    fn from(map: ValueMap) -> Self {
        Self {
            lovelace: map.lovelace,
            assets: map
                .assets
                .into_iter()
                .map(|(policy, assets)| {
                    (
                        policy,
                        assets
                            .into_iter()
                            .map(|(name, amount)| NativeAsset { name, amount })
                            .collect(),
                    )
                })
                .collect(),
        }
    }
}

impl From<ValueMap> for ValueDelta {
    fn from(map: ValueMap) -> Self {
        Self {
            lovelace: map.lovelace as i64,
            assets: map
                .assets
                .into_iter()
                .map(|(policy, assets)| {
                    (
                        policy,
                        assets
                            .into_iter()
                            .map(|(name, amount)| NativeAssetDelta {
                                name,
                                amount: amount as i64,
                            })
                            .collect(),
                    )
                })
                .collect(),
        }
    }
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ValueDelta {
    pub lovelace: i64,
    pub assets: NativeAssetsDelta,
}

#[derive(
    Debug, Default, Clone, serde::Serialize, serde::Deserialize, minicbor::Encode, minicbor::Decode,
)]
pub struct AddressTotalsMap {
    #[n(0)]
    pub lovelace: i64,
    #[n(1)]
    pub assets: NativeAssetsMap,
}

impl ValueDelta {
    pub fn new(lovelace: i64, assets: NativeAssetsDelta) -> Self {
        Self { lovelace, assets }
    }
}

impl From<&Value> for ValueDelta {
    fn from(v: &Value) -> Self {
        ValueDelta {
            lovelace: v.lovelace as i64,
            assets: v
                .assets
                .iter()
                .map(|(pid, nas)| {
                    let nas_delta = nas
                        .iter()
                        .map(|na| NativeAssetDelta {
                            name: na.name,
                            amount: na.amount as i64,
                        })
                        .collect();
                    (*pid, nas_delta)
                })
                .collect(),
        }
    }
}

impl Neg for ValueDelta {
    type Output = Self;

    fn neg(mut self) -> Self::Output {
        self.lovelace = -self.lovelace;
        for (_, nas) in &mut self.assets {
            for na in nas {
                na.amount = -na.amount;
            }
        }
        self
    }
}

/// Key hash
pub type KeyHash = Hash<28>;

/// Script hash
pub type ScriptHash = KeyHash;

/// Address key hash
pub type AddrKeyhash = KeyHash;

/// Genesis key hash
pub type GenesisKeyhash = Hash<28>;

declare_hash_type!(BlockHash, 32);
declare_hash_type!(TxHash, 32);
declare_hash_type_with_bech32!(VrfKeyHash, 32, "vrf_vk");
declare_hash_type_with_bech32!(PoolId, 28, "pool");

declare_hash_type_with_bech32!(ConstitutionalCommitteeKeyHash, 28, "cc_hot");
declare_hash_type_with_bech32!(ConstitutionalCommitteeScriptHash, 28, "cc_hot_script");
declare_hash_type_with_bech32!(DRepKeyHash, 28, "drep");
declare_hash_type_with_bech32!(DRepScriptHash, 28, "drep_script");

/// Data hash used for metadata, anchors (blake2b 256)
pub type DataHash = Hash<32>;

/// Compact transaction identifier (block_number, tx_index).
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
    PartialOrd,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct TxIdentifier(#[n(0)] [u8; 6]);

impl TxIdentifier {
    pub fn new(block_number: u32, tx_index: u16) -> Self {
        let mut buf = [0u8; 6];
        buf[..4].copy_from_slice(&block_number.to_be_bytes());
        buf[4..6].copy_from_slice(&tx_index.to_be_bytes());
        Self(buf)
    }

    pub fn block_number(&self) -> u32 {
        u32::from_be_bytes(self.0[..4].try_into().unwrap())
    }

    pub fn tx_index(&self) -> u16 {
        u16::from_be_bytes(self.0[4..6].try_into().unwrap())
    }

    pub fn from_bytes(bytes: [u8; 6]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 6] {
        &self.0
    }
}

impl Display for TxIdentifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.block_number(), self.tx_index())
    }
}

pub type VKey = Hash<32>;
pub type Signature = Hash<64>;

/// VKey Witness
#[derive(Debug, Clone, Hash, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct VKeyWitness {
    pub vkey: VKey,
    pub signature: Signature,
}

impl VKeyWitness {
    pub fn new(vkey: VKey, signature: Signature) -> Self {
        Self { vkey, signature }
    }

    pub fn key_hash(&self) -> KeyHash {
        keyhash_224(self.vkey.as_ref())
    }
}

impl Display for VKeyWitness {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "vkey={}, signature={}", self.vkey, self.signature)
    }
}

/// Slot
pub type Slot = u64;

/// Block Number
pub type BlockNumber = u64;

/// Epoch
pub type Epoch = u64;

/// Point on the chain
#[derive(
    Debug,
    Default,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    Eq,
    PartialEq,
    minicbor::Decode,
    minicbor::Encode,
)]
pub enum Point {
    #[default]
    #[n(0)]
    Origin,
    #[n(1)]
    Specific {
        #[n(0)]
        hash: BlockHash,
        #[n(1)]
        slot: Slot,
    },
}

impl Point {
    pub fn slot(&self) -> Slot {
        match self {
            Self::Origin => 0,
            Self::Specific { slot, .. } => *slot,
        }
    }

    pub fn hash(&self) -> Option<&BlockHash> {
        match self {
            Self::Origin => None,
            Self::Specific { hash, .. } => Some(hash),
        }
    }
}

impl Display for Point {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Origin => write!(f, "origin"),
            Self::Specific { hash, slot } => write!(f, "{slot}.{hash}"),
        }
    }
}

impl FromStr for Point {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "origin" {
            return Ok(Self::Origin);
        }
        let Some((slot_str, hash_str)) = s.split_once(".") else {
            bail!("invalid point: missing \".\"");
        };
        let slot = slot_str.parse().context("invalid slot")?;
        let hash = hash_str.parse().context("invalid hash")?;
        Ok(Self::Specific { hash, slot })
    }
}

/// Amount of Ada, in Lovelace
pub type Lovelace = u64;
pub type LovelaceDelta = i64;

/// Global 'pot' account state (treasury, reserves, deposits)
#[derive(Debug, Default, PartialEq, Clone, serde::Serialize, serde::Deserialize)]
pub struct Pots {
    /// Unallocated reserves
    pub reserves: Lovelace,

    /// Treasury
    pub treasury: Lovelace,

    /// Deposits
    pub deposits: Lovelace,
}

/// Registration change kind for stake addresses
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum RegistrationChangeKind {
    Registered,
    Deregistered,
}

/// Registration change on a stake address during an epoch
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RegistrationChange {
    /// Stake address
    pub address: StakeAddress,

    /// Change type
    pub kind: RegistrationChangeKind,

    /// Epoch slot when this change occurred (for Shelley-era filtering)
    #[serde(default)]
    pub epoch_slot: u64,
}

/// Rational number = numerator / denominator
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct Ratio {
    pub numerator: u64,
    pub denominator: u64,
}

impl Ratio {
    /// Returns the ratio as f64 (safe for large values)
    pub fn to_f64(&self) -> f64 {
        if self.denominator == 0 {
            0.0
        } else {
            (self.numerator as f64) / (self.denominator as f64)
        }
    }

    /// Returns the ratio as f32 (less precision)
    pub fn to_f32(&self) -> f32 {
        if self.denominator == 0 {
            0.0
        } else {
            (self.numerator as f32) / (self.denominator as f32)
        }
    }
}

/// Withdrawal
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Withdrawal {
    /// Stake address to withdraw from
    pub address: StakeAddress,

    /// Value to withdraw
    pub value: Lovelace,

    // Identifier of withdrawal tx
    pub tx_identifier: TxIdentifier,
}

impl Withdrawal {
    pub fn get_withdrawal_vkey_author(&self) -> Option<KeyHash> {
        self.address.credential.get_addr_key_hash()
    }

    pub fn get_withdrawal_script_author(&self) -> Option<ScriptHash> {
        self.address.credential.get_script_hash()
    }
}
/// Treasury pot account
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Pot {
    Reserves,
    Treasury,
    Deposits,
}

impl fmt::Display for Pot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Pot::Reserves => write!(f, "reserves"),
            Pot::Treasury => write!(f, "treasury"),
            Pot::Deposits => write!(f, "deposits"),
        }
    }
}

#[serde_as]
#[derive(
    Debug, Clone, Ord, Eq, PartialEq, PartialOrd, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Credential {
    /// Script hash. NOTE: Order matters when parsing Haskell Node Snapshot data.
    ScriptHash(#[serde_as(as = "Hex")] ScriptHash),

    /// Address key hash
    AddrKeyHash(#[serde_as(as = "Hex")] AddrKeyhash),
}

impl Credential {
    fn hex_string_to_hash(hex_str: &str) -> Result<KeyHash> {
        let key_hash = decode(hex_str.to_owned().into_bytes())?;
        if key_hash.len() != 28 {
            Err(anyhow!(
                "Invalid hash length for {hex_str:?}, expected 28 bytes"
            ))
        } else {
            key_hash.as_slice().try_into().map_err(|e| anyhow!("Failed to convert to KeyHash {e}"))
        }
    }

    pub fn from_json_string(credential: &str) -> Result<Self> {
        if let Some(hash) = credential.strip_prefix("scriptHash-") {
            Ok(Credential::ScriptHash(Self::hex_string_to_hash(hash)?))
        } else if let Some(hash) = credential.strip_prefix("keyHash-") {
            Ok(Credential::AddrKeyHash(Self::hex_string_to_hash(hash)?))
        } else {
            Err(anyhow!(
                "Incorrect credential {credential}, expected scriptHash- or keyHash- prefix"
            ))
        }
    }

    pub fn to_json_string(&self) -> String {
        match self {
            Self::ScriptHash(hash) => format!("scriptHash-{hash}"),
            Self::AddrKeyHash(hash) => format!("keyHash-{hash}"),
        }
    }

    pub fn get_hash(&self) -> KeyHash {
        *match self {
            Self::AddrKeyHash(hash) => hash,
            Self::ScriptHash(hash) => hash,
        }
    }

    pub fn get_script_hash(&self) -> Option<ScriptHash> {
        match self {
            Self::ScriptHash(hash) => Some(*hash),
            _ => None,
        }
    }

    pub fn get_addr_key_hash(&self) -> Option<KeyHash> {
        match self {
            Self::AddrKeyHash(hash) => Some(*hash),
            _ => None,
        }
    }

    pub fn from_drep_bech32(bech32_str: &str) -> Result<Self, Error> {
        let (hrp, data) = bech32::decode(bech32_str)?;
        if data.len() != 28 {
            return Err(anyhow!(
                "Invalid payload length for DRep Bech32, expected 28 bytes, got {}",
                data.len()
            ));
        }

        let hash = data.try_into().expect("failed to convert to fixed-size array");

        match hrp.as_str() {
            "drep" => Ok(Credential::AddrKeyHash(hash)),
            "drep_script" => Ok(Credential::ScriptHash(hash)),
            _ => Err(anyhow!(
                "Invalid HRP for DRep Bech32, expected 'drep' or 'drep_script', got '{hrp}'"
            )),
        }
    }

    pub fn to_drep_bech32(&self) -> Result<String, anyhow::Error> {
        let hrp = Hrp::parse(match self {
            Credential::AddrKeyHash(_) => "drep",
            Credential::ScriptHash(_) => "drep_script",
        })
        .map_err(|e| anyhow!("Bech32 HRP parse error: {e}"))?;

        let data = self.get_hash();

        bech32::encode::<Bech32>(hrp, data.as_slice())
            .map_err(|e| anyhow!("Bech32 encoding error: {e}"))
    }

    pub fn to_stake_bech32(&self) -> Result<String, anyhow::Error> {
        let hash = self.get_hash();

        if hash.len() != 28 {
            return Err(anyhow!("Credential hash must be 28 bytes"));
        }

        let header = match self {
            Credential::AddrKeyHash(_) => 0b1110_0001,
            Credential::ScriptHash(_) => 0b1111_0001,
        };

        let mut address_bytes = [0u8; 29];
        address_bytes[0] = header;
        address_bytes[1..].copy_from_slice(hash.as_ref());

        let hrp = Hrp::parse("stake").map_err(|e| anyhow!("HRP parse error: {e}"))?;
        bech32::encode::<Bech32>(hrp, &address_bytes)
            .map_err(|e| anyhow!("Bech32 encoding error: {e}"))
    }
}

pub type StakeCredential = Credential;

impl StakeCredential {
    pub fn to_string(&self) -> Result<String> {
        let (hrp, data) = match &self {
            Self::AddrKeyHash(data) => (Hrp::parse("stake_vkh")?, data.as_slice()),
            Self::ScriptHash(data) => (Hrp::parse("script")?, data.as_slice()),
        };

        Ok(bech32::encode::<Bech32>(hrp, data)?)
    }
}

/// Relay single host address
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct SingleHostAddr {
    /// Optional port number
    pub port: Option<u16>,

    /// Optional IPv4 address
    pub ipv4: Option<Ipv4Addr>,

    /// Optional IPv6 address
    pub ipv6: Option<Ipv6Addr>,
}

/// Relay hostname
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct SingleHostName {
    /// Optional port number
    pub port: Option<u16>,

    /// DNS name (A or AAAA record)
    pub dns_name: String,
}

/// Relay multihost (SRV)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct MultiHostName {
    /// DNS name (SRC record)
    pub dns_name: String,
}

/// Pool Relay
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub enum Relay {
    SingleHostAddr(SingleHostAddr),
    SingleHostName(SingleHostName),
    MultiHostName(MultiHostName),
}

/// Pool metadata
#[serde_as]
#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
    Eq,
    PartialEq,
)]
pub struct PoolMetadata {
    /// Metadata URL
    #[n(0)]
    pub url: String,

    /// Metadata hash
    #[serde_as(as = "Hex")]
    #[n(1)]
    pub hash: DataHash,
}

/// Pool Update Action
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PoolUpdateAction {
    Registered,
    Deregistered,
}

/// Pool Update Event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolUpdateEvent {
    pub tx_identifier: TxIdentifier,
    pub cert_index: u64,
    pub action: PoolUpdateAction,
}

impl PoolUpdateEvent {
    pub fn register_event(tx_identifier: TxIdentifier, cert_index: u64) -> Self {
        Self {
            tx_identifier,
            cert_index,
            action: PoolUpdateAction::Registered,
        }
    }

    pub fn retire_event(tx_identifier: TxIdentifier, cert_index: u64) -> Self {
        Self {
            tx_identifier,
            cert_index,
            action: PoolUpdateAction::Deregistered,
        }
    }
}

/// Pool Live Stake Info
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolLiveStakeInfo {
    pub live_stake: u64,
    pub live_delegators: u64,
    pub total_live_stakes: u64,
}

/// Pool Epoch History Data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolEpochState {
    pub epoch: u64,
    pub blocks_minted: u64,
    pub active_stake: u64,
    pub active_size: RationalNumber,
    pub delegators_count: u64,
    pub pool_reward: u64,
    pub spo_reward: u64,
}

/// SPO total delegation data (for SPDD)
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct DelegatedStake {
    /// Active stake - UTXO values and rewards
    pub active: Lovelace,

    /// Active delegators count - delegators making active stakes (used for pool history)
    pub active_delegators_count: u64,
}

/// SPO rewards data (for SPORewardsMessage)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SPORewards {
    /// Total rewards before distribution
    pub total_rewards: Lovelace,

    /// Pool operator's rewards
    pub operator_rewards: Lovelace,
}

pub use crate::drep::DRepCredential;

/// Governance actions data structures

#[derive(
    Default,
    serde::Serialize,
    serde::Deserialize,
    Debug,
    PartialEq,
    Eq,
    Clone,
    Copy,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct ExUnits {
    #[n(0)]
    pub mem: u64,
    #[n(1)]
    pub steps: u64,
}

#[derive(serde::Serialize, Default, serde::Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct ExUnitPrices {
    pub mem_price: RationalNumber,
    pub step_price: RationalNumber,
}

impl<'a, C> minicbor::Decode<'a, C> for ExUnitPrices {
    fn decode(
        d: &mut minicbor::Decoder<'a>,
        _ctx: &mut C,
    ) -> Result<Self, minicbor::decode::Error> {
        // Decode mem_price as [numerator, denominator] array
        d.array()?;
        let mem_num: u64 = d.decode()?;
        let mem_den: u64 = d.decode()?;
        let mem_price = RationalNumber::from(mem_num, mem_den);

        // Decode step_price as [numerator, denominator] array
        d.array()?;
        let step_num: u64 = d.decode()?;
        let step_den: u64 = d.decode()?;
        let step_price = RationalNumber::from(step_num, step_den);

        Ok(ExUnitPrices {
            mem_price,
            step_price,
        })
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct GovActionId {
    pub transaction_id: TxHash,
    pub action_index: u8,
}

impl GovActionId {
    pub fn to_bech32(&self) -> Result<String, anyhow::Error> {
        let mut buf = self.transaction_id.to_vec();
        buf.push(self.action_index);

        let gov_action_hrp = Hrp::parse("gov_action")?;
        let encoded = bech32::encode::<Bech32>(gov_action_hrp, &buf)
            .map_err(|e| anyhow!("Bech32 encoding error: {e}"))?;
        Ok(encoded)
    }

    pub fn from_bech32(bech32_str: &str) -> Result<Self, anyhow::Error> {
        let (hrp, data) = bech32::decode(bech32_str)?;

        if hrp != Hrp::parse("gov_action")? {
            return Err(anyhow!("Invalid HRP, expected 'gov_action', got: {hrp}"));
        }

        if data.len() < 33 {
            return Err(anyhow!("Invalid Bech32 governance action"));
        }

        let transaction_id: TxHash = match data[..32].try_into() {
            Ok(arr) => arr,
            Err(_) => return Err(anyhow!("Transaction ID must be 32 bytes")),
        };
        let action_index = data[32];

        Ok(GovActionId {
            transaction_id,
            action_index,
        })
    }

    pub fn set_action_index(&mut self, action_index: usize) -> Result<&Self, anyhow::Error> {
        if action_index >= 256 {
            return Err(anyhow!("Action_index {action_index} >= 256"));
        }

        self.action_index = action_index as u8;
        Ok(self)
    }
}

impl Display for GovActionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self.to_bech32() {
            Ok(s) => write!(f, "{s}"),
            Err(e) => {
                tracing::error!("GovActionId to_bech32 failed: {:?}", e);
                write!(f, "<invalid-govactionid>")
            }
        }
    }
}

#[derive(Default, Serialize, Deserialize, Debug, PartialEq, Eq, Clone, minicbor::Decode)]
pub struct CostModel(#[n(0)] Vec<i64>);

impl CostModel {
    pub fn new(m: Vec<i64>) -> Self {
        CostModel(m)
    }

    pub fn as_vec(&self) -> &Vec<i64> {
        &self.0
    }
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct CostModels {
    pub plutus_v1: Option<CostModel>,
    pub plutus_v2: Option<CostModel>,
    pub plutus_v3: Option<CostModel>,
}

#[derive(
    Default, serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone, minicbor::Decode,
)]
pub struct PoolVotingThresholds {
    #[n(0)]
    pub motion_no_confidence: RationalNumber,
    #[n(1)]
    pub committee_normal: RationalNumber,
    #[n(2)]
    pub committee_no_confidence: RationalNumber,
    #[n(3)]
    pub hard_fork_initiation: RationalNumber,
    #[n(4)]
    pub security_voting_threshold: RationalNumber,
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SoftForkRule {
    pub init_thd: u64,
    pub min_thd: u64,
    pub thd_decrement: u64,
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TxFeePolicy {
    pub multiplier: u64,
    pub summand: u64,
}

#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BlockVersionData {
    pub script_version: u16,
    pub heavy_del_thd: u64,
    pub max_block_size: u64,
    pub max_header_size: u64,
    pub max_proposal_size: u64,
    pub max_tx_size: u64,
    pub mpc_thd: u64,
    pub slot_duration: u64,

    pub softfork_rule: SoftForkRule,
    pub tx_fee_policy: TxFeePolicy,

    pub unlock_stake_epoch: u64,
    pub update_implicit: u64,
    pub update_proposal_thd: u64,
    pub update_vote_thd: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HeavyDelegate {
    pub cert: Vec<u8>,
    pub delegate_pk: Vec<u8>,
    pub issuer_pk: Vec<u8>,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GenesisDelegate {
    #[serde_as(as = "Hex")]
    pub delegate: Hash<28>,
    #[serde_as(as = "Hex")]
    pub vrf: VrfKeyHash,
}

#[serde_as]
#[derive(Default, Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct GenesisDelegates(
    #[serde_as(as = "BTreeMap<Hex, _>")] pub BTreeMap<GenesisKeyhash, GenesisDelegate>,
);

impl TryFrom<Vec<(&str, (&str, &str))>> for GenesisDelegates {
    type Error = anyhow::Error;
    fn try_from(entries: Vec<(&str, (&str, &str))>) -> Result<Self, Self::Error> {
        Ok(GenesisDelegates(
            entries
                .into_iter()
                .map(|(genesis_key_str, (delegate_str, vrf_str))| {
                    let genesis_key = GenesisKeyhash::from_str(genesis_key_str)
                        .map_err(|e| anyhow::anyhow!("Invalid genesis key hash: {e}"))?;
                    let delegate = Hash::<28>::from_str(delegate_str)
                        .map_err(|e| anyhow::anyhow!("Invalid genesis delegate: {e}"))?;
                    let vrf = VrfKeyHash::from_str(vrf_str)
                        .map_err(|e| anyhow::anyhow!("Invalid genesis VRF: {e}"))?;
                    Ok((genesis_key, GenesisDelegate { delegate, vrf }))
                })
                .collect::<Result<_, Self::Error>>()?,
        ))
    }
}

impl AsRef<BTreeMap<GenesisKeyhash, GenesisDelegate>> for GenesisDelegates {
    fn as_ref(&self) -> &BTreeMap<GenesisKeyhash, GenesisDelegate> {
        &self.0
    }
}

impl From<HashMap<PoolId, GenesisDelegate>> for GenesisDelegates {
    fn from(map: HashMap<PoolId, GenesisDelegate>) -> Self {
        GenesisDelegates(map.into_iter().map(|(k, v)| (*k, v)).collect())
    }
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProtocolConsts {
    pub k: usize,
    pub protocol_magic: MagicNumber,
    pub vss_max_ttl: Option<u32>,
    pub vss_min_ttl: Option<u32>,
}

#[derive(Debug, Default, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MagicNumber(u32);

impl MagicNumber {
    pub fn new(value: u32) -> Self {
        Self(value)
    }

    pub fn to_network_name(&self) -> &str {
        match self.0 {
            764824073 => "mainnet",
            1 => "preprod",
            2 => "preview",
            4 => "sanchonet",
            _ => "unknown",
        }
    }
}

impl From<MagicNumber> for u32 {
    fn from(m: MagicNumber) -> Self {
        m.0
    }
}

#[bitmask(u8)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum ProtocolParamType {
    NetworkGroup,
    EconomicGroup,
    TechnicalGroup,
    GovernanceGroup,
    SecurityProperty,
}

#[derive(Debug, Default, Clone)]
pub struct RewardParams {
    pub expansion_rate: RationalNumber,
    pub treasury_growth_rate: RationalNumber,
    pub desired_number_of_stake_pools: u64,
    pub pool_pledge_influence: RationalNumber,
    pub min_pool_cost: u64,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProtocolParamUpdate {
    /// The following are the fields from Conway ProtocolParamUpdate structure
    /// AKA txFeePerByte, tx_fee_per_byte (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub minfee_a: Option<u64>,

    /// AKA txFeeFixed, tx_fee_fixed (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub minfee_b: Option<u64>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_block_body_size: Option<u64>,

    /// AKA max_tx_size (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_transaction_size: Option<u64>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_block_header_size: Option<u64>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub key_deposit: Option<Lovelace>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub pool_deposit: Option<Lovelace>,

    /// AKA poolRetireMaxEpoch, eMax (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub maximum_epoch: Option<u64>,

    /// AKA stakePoolTargetNum, nOpt (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub desired_number_of_stake_pools: Option<u64>,

    /// AKA a0 (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub pool_pledge_influence: Option<RationalNumber>,

    /// AKA rho, monetary_expansion (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub expansion_rate: Option<RationalNumber>,

    /// AKA tau, treasury_cut (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub treasury_growth_rate: Option<RationalNumber>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub min_pool_cost: Option<Lovelace>,

    /// Cost per 8-byte word (Alonzo) - DEPRECATED after Babbage
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub lovelace_per_utxo_word: Option<Lovelace>,

    /// AKA plutus_v1_cost_model (Shelley), plutus_v2_cost_model (Babbage)
    /// plutus_v3_cost_model (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub cost_models_for_script_languages: Option<CostModels>,

    /// AKA execution_prices (Alonzo)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub execution_costs: Option<ExUnitPrices>,

    /// (Alonzo)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_tx_ex_units: Option<ExUnits>,

    /// (Alonzo)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_block_ex_units: Option<ExUnits>,

    /// (Alonzo)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_value_size: Option<u64>,

    /// (Alonzo)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub collateral_percentage: Option<u64>,

    /// (Alonzo)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub max_collateral_inputs: Option<u64>,

    // Cost per byte (Babbage)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub coins_per_utxo_byte: Option<Lovelace>,

    /// (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub pool_voting_thresholds: Option<PoolVotingThresholds>,

    /// (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub drep_voting_thresholds: Option<DRepVotingThresholds>,

    /// (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub min_committee_size: Option<u64>,

    /// AKA committee_max_term_limit (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub committee_term_limit: Option<u64>,

    /// AKA gov_action_lifetime (Cownay)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub governance_action_validity_period: Option<u64>,

    /// AKA gov_action_deposit (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub governance_action_deposit: Option<Lovelace>,

    /// AKA d_rep_deposit (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub drep_deposit: Option<Lovelace>,

    /// AKA drep_inactivity (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub drep_inactivity_period: Option<u64>,

    /// AKA min_fee_ref_script_cost_per_byte (Conway)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub minfee_refscript_cost_per_byte: Option<RationalNumber>,

    /// The following are the fields from Alonzo-compatible ProtocolParamUpdate
    /// structure, not present in Conway.
    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub decentralisation_constant: Option<RationalNumber>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub extra_enthropy: Option<protocol_params::Nonce>,

    /// (Shelley)
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(default)]
    pub protocol_version: Option<protocol_params::ProtocolVersion>,
}

#[derive(Serialize, PartialEq, Deserialize, Debug, Clone)]
pub struct AlonzoBabbageUpdateProposal {
    pub proposals: Vec<(GenesisKeyhash, Box<ProtocolParamUpdate>)>,
    pub enactment_epoch: u64,
}

impl AlonzoBabbageUpdateProposal {
    pub fn get_governance_vkey_authors(
        &self,
        genesis_delegs: &GenesisDelegates,
    ) -> HashSet<KeyHash> {
        let mut vkey_hashes = HashSet::new();
        for (genesis_key_hash, _) in self.proposals.iter() {
            let found_genesis: Option<&GenesisDelegate> =
                genesis_delegs.as_ref().get(genesis_key_hash);
            if let Some(genesis) = found_genesis {
                vkey_hashes.insert(genesis.delegate);
            } else {
                error!("Genesis delegate not found: {genesis_key_hash}");
            }
        }

        vkey_hashes
    }
}

#[derive(Default, Serialize, PartialEq, Eq, Deserialize, Debug, Clone)]
pub struct Constitution {
    pub anchor: Anchor,
    pub guardrail_script: Option<ScriptHash>,
}

impl<'b, C> minicbor::Decode<'b, C> for Constitution {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.array()?; // Constitution array

        // In snapshot format, Anchor fields are flattened (not wrapped in array)
        // Try to detect: if next element is bytes/string, it's flattened
        // If next element is array, it's wrapped
        let is_flattened = matches!(
            d.datatype()?,
            minicbor::data::Type::Bytes | minicbor::data::Type::String
        );

        let anchor = if is_flattened {
            // Flattened format: [url, data_hash, guardrail_script]
            let url = match d.datatype()? {
                minicbor::data::Type::Bytes => {
                    let url_bytes = d.bytes()?;
                    String::from_utf8_lossy(url_bytes).to_string()
                }
                minicbor::data::Type::String => d.str()?.to_string(),
                _ => {
                    return Err(minicbor::decode::Error::message(
                        "Expected bytes or string for Anchor URL",
                    ))
                }
            };
            let data_hash: Vec<u8> = d.bytes()?.to_vec();
            Anchor { url, data_hash }
        } else {
            // Wrapped format: [[url, data_hash], guardrail_script]
            d.decode_with(ctx)?
        };

        let guardrail_script: Option<ScriptHash> = d.decode_with(ctx)?;
        Ok(Self {
            anchor,
            guardrail_script,
        })
    }
}

#[serde_as]
#[derive(Default, Serialize, PartialEq, Debug, Deserialize, Clone)]
pub struct Committee {
    #[serde_as(as = "Vec<(_, _)>")]
    pub members: HashMap<CommitteeCredential, u64>,
    pub threshold: RationalNumber,
}

impl Committee {
    pub fn is_empty(&self) -> bool {
        self.members.is_empty()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ParameterChangeAction {
    pub previous_action_id: Option<GovActionId>,
    pub protocol_param_update: Box<ProtocolParamUpdate>,
    pub script_hash: Option<ScriptHash>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HardForkInitiationAction {
    pub previous_action_id: Option<GovActionId>,
    pub protocol_version: protocol_params::ProtocolVersion,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TreasuryWithdrawalsAction {
    #[serde_as(as = "Vec<(_, _)>")]
    pub rewards: HashMap<Vec<u8>, Lovelace>,
    pub script_hash: Option<ScriptHash>,
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CommitteeChange {
    pub removed_committee_members: HashSet<CommitteeCredential>,
    #[serde_as(as = "Vec<(_, _)>")]
    pub new_committee_members: HashMap<CommitteeCredential, u64>,
    pub terms: RationalNumber,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct UpdateCommitteeAction {
    pub previous_action_id: Option<GovActionId>,
    pub data: CommitteeChange,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NewConstitutionAction {
    pub previous_action_id: Option<GovActionId>,
    pub new_constitution: Constitution,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GovernanceAction {
    ParameterChange(ParameterChangeAction),
    HardForkInitiation(HardForkInitiationAction),
    TreasuryWithdrawals(TreasuryWithdrawalsAction),
    NoConfidence(Option<GovActionId>),
    UpdateCommittee(UpdateCommitteeAction),
    NewConstitution(NewConstitutionAction),
    Information,
}

impl GovernanceAction {
    pub fn get_previous_action_id(&self) -> Option<GovActionId> {
        match &self {
            Self::ParameterChange(ParameterChangeAction {
                previous_action_id: prev,
                ..
            }) => prev.clone(),
            Self::HardForkInitiation(HardForkInitiationAction {
                previous_action_id: prev,
                ..
            }) => prev.clone(),
            Self::TreasuryWithdrawals(_) => None,
            Self::NoConfidence(prev) => prev.clone(),
            Self::UpdateCommittee(UpdateCommitteeAction {
                previous_action_id: prev,
                ..
            }) => prev.clone(),
            Self::NewConstitution(NewConstitutionAction {
                previous_action_id: prev,
                ..
            }) => prev.clone(),
            Self::Information => None,
        }
    }

    pub fn get_action_name(&self) -> &str {
        match &self {
            GovernanceAction::ParameterChange(_) => "ParameterChange",
            GovernanceAction::HardForkInitiation(_) => "HardForkInitiation",
            GovernanceAction::TreasuryWithdrawals(_) => "TreasuryWithdrawals",
            GovernanceAction::NoConfidence(_) => "NoConfidence",
            GovernanceAction::UpdateCommittee(_) => "UpdateCommittee",
            GovernanceAction::NewConstitution(_) => "NewConstitution",
            GovernanceAction::Information => "Information",
        }
    }

    pub fn get_action_script_hash(&self) -> Option<ScriptHash> {
        match self {
            GovernanceAction::ParameterChange(action) => action.script_hash,
            GovernanceAction::TreasuryWithdrawals(action) => action.script_hash,
            _ => None,
        }
    }
}

#[derive(
    serde::Serialize, serde::Deserialize, Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Hash,
)]
pub enum Voter {
    ConstitutionalCommitteeKey(ConstitutionalCommitteeKeyHash),
    ConstitutionalCommitteeScript(ConstitutionalCommitteeScriptHash),
    DRepKey(DRepKeyHash),
    DRepScript(DRepScriptHash),
    StakePoolKey(PoolId),
}

impl Voter {
    pub fn to_bech32(&self) -> Result<String, Error> {
        match self {
            Voter::ConstitutionalCommitteeKey(h) => h.to_bech32(),
            Voter::ConstitutionalCommitteeScript(s) => s.to_bech32(),
            Voter::DRepKey(k) => k.to_bech32(),
            Voter::DRepScript(s) => s.to_bech32(),
            Voter::StakePoolKey(k) => k.to_bech32(),
        }
    }

    pub fn get_voter_script_hash(&self) -> Option<ScriptHash> {
        match self {
            Voter::ConstitutionalCommitteeScript(s) => Some(s.into_inner()),
            Voter::DRepScript(s) => Some(s.into_inner()),
            _ => None,
        }
    }

    pub fn get_voter_key_hash(&self) -> Option<KeyHash> {
        match self {
            Voter::ConstitutionalCommitteeKey(h) => Some(h.into_inner()),
            Voter::DRepKey(k) => Some(k.into_inner()),
            Voter::StakePoolKey(k) => Some(k.into_inner()),
            _ => None,
        }
    }
}

impl Display for Voter {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self.to_bech32() {
            Ok(addr) => write!(f, "{addr}"),
            Err(e) => write!(f, "<invalid voter: {e}>"),
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone)]
pub enum Vote {
    No,
    Yes,
    Abstain,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VotingProcedure {
    pub vote: Vote,
    pub anchor: Option<Anchor>,
    pub vote_index: u32,
}

#[serde_as]
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SingleVoterVotes {
    #[serde_as(as = "Vec<(_, _)>")]
    pub voting_procedures: HashMap<GovActionId, VotingProcedure>,
}

#[serde_as]
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VotingProcedures {
    #[serde_as(as = "Vec<(_, _)>")]
    pub votes: HashMap<Voter, SingleVoterVotes>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VoteCount {
    pub yes: u64,
    pub no: u64,
    pub abstain: u64,
}

impl VoteCount {
    pub fn zero() -> Self {
        Self {
            yes: 0,
            no: 0,
            abstain: 0,
        }
    }

    pub fn total(&self) -> u64 {
        self.yes + self.no + self.abstain
    }
}

impl Display for VoteCount {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "y{}/n{}/a{}", self.yes, self.no, self.abstain)
    }
}

impl FromStr for VoteCount {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let re = Regex::new(r"y(\d+)/n(\d+)/a(\d+)$").unwrap();
        let caps = re.captures(s).ok_or_else(|| anyhow!("Invalid VoteCount string: '{s}'"))?;

        let yes = u64::from_str(&caps[1])?;
        let no = u64::from_str(&caps[2])?;
        let abstain = u64::from_str(&caps[3])?;

        Ok(VoteCount { yes, no, abstain })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VoteResult<E: FromStr + Display> {
    pub committee: E,
    pub drep: E,
    pub pool: E,
}

impl<E: FromStr + Display> VoteResult<E> {
    pub fn new(committee: E, drep: E, pool: E) -> Self {
        Self {
            committee,
            drep,
            pool,
        }
    }
}

impl<E: FromStr + Display> Display for VoteResult<E> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "c{}:d{}:s{}", self.committee, self.drep, self.pool)
    }
}

impl<E: FromStr + Display> FromStr for VoteResult<E> {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        // Regex for capturing each section
        let Ok(re) = Regex::new(r"^c([^:]+):d([^:]+):s([^:]+)$") else {
            bail!("Cannot parse redex");
        };
        let caps = re.captures(s).ok_or_else(|| anyhow!("Invalid VoteResult string: '{s}'"))?;

        let Ok(committee) = E::from_str(&caps[1]) else {
            bail!("Incorrect committee value {}", &caps[1]);
        };
        let Ok(drep) = E::from_str(&caps[2]) else {
            bail!("Incorrect DRep value {}", &caps[2]);
        };
        let Ok(pool) = E::from_str(&caps[3]) else {
            bail!("Incorrect SPO value {}", &caps[3]);
        };

        Ok(VoteResult {
            committee,
            drep,
            pool,
        })
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VotingOutcome {
    pub procedure: ProposalProcedure,
    pub votes_cast: VoteResult<VoteCount>,
    pub votes_threshold: VoteResult<RationalNumber>,
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProposalProcedure {
    pub deposit: Lovelace,
    pub reward_account: StakeAddress,
    pub gov_action_id: GovActionId,
    pub gov_action: GovernanceAction,
    pub anchor: Anchor,
}

impl ProposalProcedure {
    pub fn get_proposal_script_hash(&self) -> Option<ScriptHash> {
        self.gov_action.get_action_script_hash()
    }
}

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommitteeUpdateEnactment {
    #[serde_as(as = "Vec<(_, _)>")]
    pub members_change: HashMap<CommitteeCredential, Option<u64>>,
    pub terms: RationalNumber,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum EnactStateElem {
    Params(Box<ProtocolParamUpdate>),
    Constitution(Constitution),
    Committee(CommitteeChange),
    ProtVer(protocol_params::ProtocolVersion),
    NoConfidence,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum GovernanceOutcomeVariant {
    EnactStateElem(EnactStateElem),
    TreasuryWithdrawal(TreasuryWithdrawalsAction),
    NoAction,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlonzoBabbageVotingOutcome {
    pub voting: Vec<GenesisKeyhash>,
    pub votes_threshold: u32,
    pub accepted: bool,
    pub parameter_update: Box<ProtocolParamUpdate>,
}

/// The structure has info about outcome of a single governance action.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GovernanceOutcome {
    /// Information about voting results: what was the issue,
    /// how many votes cast, was it accepted or not
    pub voting: VotingOutcome,

    /// Enact state/Withdrawal, accepted after voting. If the voting failed,
    /// or if the proposal does not suppose formal action, this field is
    /// `NoFormalAction`
    pub action_to_perform: GovernanceOutcomeVariant,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetInfoRecord {
    pub initial_mint_tx: TxIdentifier,
    pub mint_or_burn_count: u64,
    pub metadata: AssetMetadata,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetMetadata {
    pub cip25_metadata: Option<Vec<u8>>,
    pub cip25_version: Option<AssetMetadataStandard>,
    pub cip68_metadata: Option<Vec<u8>>,
    pub cip68_version: Option<AssetMetadataStandard>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct AssetMintRecord {
    pub tx: TxIdentifier,
    pub amount: u64,
    pub burn: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum AssetMetadataStandard {
    CIP25v1,
    CIP25v2,
    CIP68v1,
    CIP68v2,
    CIP68v3,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PolicyAsset {
    pub policy: PolicyId,
    pub name: AssetName,
    pub quantity: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetAddressEntry {
    pub address: ShelleyAddress,
    pub quantity: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxTotals {
    pub sent: Value,
    pub received: Value,
}

#[derive(
    Debug, Default, Clone, serde::Serialize, serde::Deserialize, minicbor::Encode, minicbor::Decode,
)]
pub struct AddressTotals {
    #[n(0)]
    pub sent: ValueMap,
    #[n(1)]
    pub received: ValueMap,
    #[n(2)]
    pub tx_count: u64,
}

impl AddAssign for AddressTotals {
    fn add_assign(&mut self, other: Self) {
        self.sent += other.sent;
        self.received += other.received;
        self.tx_count += other.tx_count;
    }
}

impl AddressTotals {
    pub fn apply_delta(&mut self, delta: &TxTotals) {
        self.received.lovelace += delta.received.lovelace;
        self.sent.lovelace += delta.sent.lovelace;

        for (policy, assets) in &delta.received.assets {
            for asset in assets {
                Self::apply_asset(&mut self.received.assets, *policy, asset.name, asset.amount);
            }
        }

        for (policy, assets) in &delta.sent.assets {
            for asset in assets {
                Self::apply_asset(&mut self.sent.assets, *policy, asset.name, asset.amount);
            }
        }

        self.tx_count += 1;
    }

    fn apply_asset(
        target: &mut HashMap<PolicyId, HashMap<AssetName, u64>>,
        policy: PolicyId,
        name: AssetName,
        amount: u64,
    ) {
        target
            .entry(policy)
            .or_default()
            .entry(name)
            .and_modify(|v| *v += amount)
            .or_insert(amount);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::Hash;
    use anyhow::Result;
    use test_case::test_case;

    #[test]
    fn era_order() -> Result<()> {
        assert_eq!(Era::default() as u8, 0);
        assert_eq!(Era::Byron as u8, 0);
        assert_eq!(Era::Conway as u8, 6);
        assert!(Era::try_from(7).is_err());

        for ei in 0..=6 {
            for ej in 0..=6 {
                assert_eq!(
                    Era::try_from(ei).unwrap() < Era::try_from(ej).unwrap(),
                    ei < ej
                );
                assert_eq!(
                    Era::try_from(ei).unwrap() > Era::try_from(ej).unwrap(),
                    ei > ej
                );
                assert_eq!(
                    Era::try_from(ei).unwrap() == Era::try_from(ej).unwrap(),
                    ei == ej
                );
            }
        }

        Ok(())
    }

    fn make_committee_credential(addr_key_hash: bool, val: u8) -> CommitteeCredential {
        // Create a 28-byte array filled with the value
        let hash_bytes = [val; 28];
        if addr_key_hash {
            Credential::AddrKeyHash(KeyHash::from(hash_bytes))
        } else {
            Credential::ScriptHash(KeyHash::from(hash_bytes))
        }
    }

    #[test]
    fn test_utxo_identifier_to_bytes() -> Result<()> {
        let tx_hash = TxHash::try_from(
            hex::decode("000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f")
                .unwrap(),
        )
        .unwrap();
        let output_index = 42;
        let utxo = UTxOIdentifier::new(tx_hash, output_index);
        let bytes = utxo.to_bytes();
        assert_eq!(
            hex::encode(bytes),
            "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f002a"
        );

        Ok(())
    }

    #[test]
    fn governance_serialization_test() -> Result<()> {
        let gov_action_id = GovActionId::default();

        let mut voting = VotingProcedures::default();
        // Create a test hash with pattern [1, 2, 3, 4, 0, 0, ...]
        let mut test_hash_bytes = [0u8; 28];
        test_hash_bytes[0..4].copy_from_slice(&[1, 2, 3, 4]);
        voting.votes.insert(
            Voter::StakePoolKey(test_hash_bytes.into()),
            SingleVoterVotes::default(),
        );

        let mut single_voter = SingleVoterVotes::default();
        single_voter.voting_procedures.insert(
            gov_action_id.clone(),
            VotingProcedure {
                anchor: None,
                vote: Vote::Abstain,
                vote_index: 0,
            },
        );
        voting.votes.insert(
            Voter::StakePoolKey(PoolId::new(Hash::new(test_hash_bytes))),
            SingleVoterVotes::default(),
        );
        println!("Json: {}", serde_json::to_string(&voting)?);

        let gov_action = GovernanceAction::UpdateCommittee(UpdateCommitteeAction {
            previous_action_id: None,
            data: CommitteeChange {
                removed_committee_members: HashSet::from_iter([
                    make_committee_credential(true, 48),
                    make_committee_credential(false, 12),
                ]),
                new_committee_members: HashMap::from_iter([(
                    make_committee_credential(false, 87),
                    1234,
                )]),
                terms: RationalNumber::ONE,
            },
        });

        let proposal = ProposalProcedure {
            deposit: 9876,
            reward_account: StakeAddress::default(),
            gov_action_id,
            gov_action,
            anchor: Anchor {
                url: "some.url".to_owned(),
                data_hash: vec![2, 3, 4, 5],
            },
        };
        println!("Json: {}", serde_json::to_string(&proposal)?);

        Ok(())
    }

    #[test]
    fn parse_voting_values() -> Result<()> {
        let count = VoteCount::from_str("y0/n5/a1")?;
        assert_eq!(count.yes, 0);
        assert_eq!(count.no, 5);
        assert_eq!(count.abstain, 1);

        let counts: VoteResult<VoteCount> =
            VoteResult::from_str("cy0/n5/a1:dy0/n1/a2:sy123/n456/a0788890")?;
        assert_eq!(counts.committee, count);
        assert_eq!(counts.drep.yes, 0);
        assert_eq!(counts.drep.no, 1);
        assert_eq!(counts.drep.abstain, 2);
        assert_eq!(counts.pool.yes, 123);
        assert_eq!(counts.pool.no, 456);
        assert_eq!(counts.pool.abstain, 788890);
        Ok(())
    }

    #[test]
    fn serialize_stake_address() -> Result<()> {
        let serialized = "{\
            \"network\":\"Mainnet\",\
            \"credential\":{\
                \"AddrKeyHash\":\"45dee6ee5d7f631b6226d45f29da411c42fa7e816dc0948d31e0dba7\"\
            }\
        }";

        let addr = serde_json::from_str::<StakeAddress>(serialized)?;
        assert_eq!(addr.network, NetworkId::Mainnet);
        assert_eq!(
            addr.credential,
            StakeCredential::AddrKeyHash(KeyHash::from([
                0x45, 0xde, 0xe6, 0xee, 0x5d, 0x7f, 0x63, 0x1b, 0x62, 0x26, 0xd4, 0x5f, 0x29, 0xda,
                0x41, 0x1c, 0x42, 0xfa, 0x7e, 0x81, 0x6d, 0xc0, 0x94, 0x8d, 0x31, 0xe0, 0xdb, 0xa7,
            ]))
        );

        let serialized_back = serde_json::to_string(&addr)?;
        assert_eq!(serialized_back, serialized);

        Ok(())
    }

    #[test_case("origin")]
    #[test_case("48460699.7bfb6a677df577d2f0371236ecf63554b54b35b663d3ad9159695a609306e629")]
    #[test_case("123665404.5acee019d5550554aff5a044ed3b700decf29460e100218381f85a767af8c09f")]
    fn should_round_trip_points(point_str: &str) {
        let point: Point = point_str.parse().expect("invalid point");
        assert_eq!(point.to_string(), point_str);
    }

    #[test_case("onigiri", "invalid point: missing \".\"")]
    #[test_case(
        "4846069a.7bfb6a677df577d2f0371236ecf63554b54b35b663d3ad9159695a609306e629",
        "invalid slot"
    )]
    #[test_case(
        "123665404.5acee019d5550554aff5a044ed3b700decf29460e100218381f85a767af8c09fff",
        "invalid hash"
    )]
    fn should_report_errors_parsing_points(point_str: &str, message: &str) {
        let Err(error) = point_str.parse::<Point>() else {
            panic!("expected parsing error, got success");
        };
        assert_eq!(error.to_string(), message);
    }
}
