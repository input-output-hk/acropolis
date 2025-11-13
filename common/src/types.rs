//! Core type definitions for Acropolis
// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]

use crate::hash::Hash;
use crate::serialization::Bech32Conversion;
use crate::{
    address::{Address, ShelleyAddress, StakeAddress},
    declare_hash_type, declare_hash_type_with_bech32, protocol_params,
    rational_number::RationalNumber,
};
use anyhow::{anyhow, bail, Error, Result};
use bech32::{Bech32, Hrp};
use bitmask_enum::bitmask;
use hex::decode;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_with::{hex::Hex, serde_as};
use std::collections::BTreeMap;
use std::{
    cmp::Ordering,
    collections::{HashMap, HashSet},
    fmt,
    fmt::{Display, Formatter},
    ops::{AddAssign, Neg},
    str::FromStr,
};

/// Network identifier
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
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
    pub pool_id: String,

    /// Number of blocks produced by this pool in the epoch
    pub block_count: u8,

    /// Epoch number
    pub epoch: u64,
}

/// Block status
#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BlockStatus {
    Bootstrap,  // Pseudo-block from bootstrap data
    Immutable,  // Now immutable (more than 'k' blocks ago)
    Volatile,   // Volatile, in sequence
    RolledBack, // Volatile, restarted after rollback
}

/// Block info, shared across multiple messages
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BlockInfo {
    /// Block status
    pub status: BlockStatus,

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

/// Individual address balance change
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressDelta {
    /// Address
    pub address: Address,

    /// UTxO causing address delta
    pub utxo: UTxOIdentifier,

    /// Balance change
    pub value: ValueDelta,
}

/// Stake balance change
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeAddressDelta {
    /// Stake address
    pub stake_address: StakeAddress,

    /// Shelley addresses contributing to the delta
    pub addresses: Vec<ShelleyAddress>,

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
}

impl fmt::Display for RewardType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RewardType::Leader => write!(f, "leader"),
            RewardType::Member => write!(f, "member"),
            RewardType::PoolRefund => write!(f, "pool_deposit_refund"),
        }
    }
}

pub type PolicyId = [u8; 28];
pub type NativeAssets = Vec<(PolicyId, Vec<NativeAsset>)>;
pub type NativeAssetsDelta = Vec<(PolicyId, Vec<NativeAssetDelta>)>;
pub type NativeAssetsMap = HashMap<PolicyId, HashMap<AssetName, u64>>;
pub type NativeAssetsDeltaMap = HashMap<PolicyId, HashMap<AssetName, i64>>;

#[derive(
    Debug,
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
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

#[derive(
    Debug, Clone, serde::Serialize, serde::Deserialize, minicbor::Encode, minicbor::Decode,
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

/// Datum (inline or hash)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Datum {
    Hash(Vec<u8>),
    Inline(Vec<u8>),
}

// The full CBOR bytes of a reference script
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum ReferenceScript {
    Native(Vec<u8>),
    PlutusV1(Vec<u8>),
    PlutusV2(Vec<u8>),
    PlutusV3(Vec<u8>),
}

/// Value (lovelace + multiasset)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

/// Hashmap representation of Value (lovelace + multiasset)
#[derive(
    Debug, Default, Clone, serde::Serialize, serde::Deserialize, minicbor::Encode, minicbor::Decode,
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

/// Hashmap representation of ValueDelta (lovelace + multiasset)
pub struct ValueDeltaMap {
    pub lovelace: i64,
    pub assets: NativeAssetsDeltaMap,
}

impl From<ValueDelta> for ValueDeltaMap {
    fn from(value: ValueDelta) -> Self {
        let mut assets = HashMap::new();

        for (policy, asset_list) in value.assets {
            let policy_entry = assets.entry(policy).or_insert_with(HashMap::new);
            for asset in asset_list {
                *policy_entry.entry(asset.name).or_insert(0) += asset.amount;
            }
        }

        ValueDeltaMap {
            lovelace: value.lovelace,
            assets,
        }
    }
}

impl AddAssign<ValueDelta> for ValueDeltaMap {
    fn add_assign(&mut self, delta: ValueDelta) {
        self.lovelace += delta.lovelace;

        for (policy, assets) in delta.assets {
            let policy_entry = self.assets.entry(policy).or_default();
            for asset in assets {
                *policy_entry.entry(asset.name).or_insert(0) += asset.amount;
            }
        }
    }
}

impl From<ValueDeltaMap> for ValueDelta {
    fn from(map: ValueDeltaMap) -> Self {
        let mut assets_vec = Vec::with_capacity(map.assets.len());

        for (policy, asset_map) in map.assets {
            let inner_assets = asset_map
                .into_iter()
                .map(|(name, amount)| NativeAssetDelta { name, amount })
                .collect();

            assets_vec.push((policy, inner_assets));
        }

        ValueDelta {
            lovelace: map.lovelace,
            assets: assets_vec,
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

/// Value stored in UTXO
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UTXOValue {
    /// Address in binary
    pub address: Address,

    /// Value in Lovelace
    pub value: Value,

    /// Datum
    pub datum: Option<Datum>,

    /// Reference script
    pub reference_script: Option<ReferenceScript>,
}

/// Transaction output (UTXO)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxOutput {
    /// Identifier for this UTxO
    pub utxo_identifier: UTxOIdentifier,

    /// Address data
    pub address: Address,

    /// Output value (Lovelace + native assets)
    pub value: Value,

    /// Datum (Inline or Hash)
    pub datum: Option<Datum>,

    /// Reference script
    pub reference_script: Option<ReferenceScript>,
}

/// Transaction input (UTXO reference)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxInput {
    /// Identifer of the referenced UTxO
    pub utxo_identifier: UTxOIdentifier,
}

/// Option of either TxOutput or TxInput
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum UTXODelta {
    None(()),
    Output(TxOutput),
    Input(TxInput),
}

impl Default for UTXODelta {
    fn default() -> Self {
        Self::None(())
    }
}

/// Key hash
pub type KeyHash = Hash<28>;

/// Script hash
pub type ScriptHash = KeyHash;

/// Address key hash
pub type AddrKeyhash = KeyHash;

/// Script identifier
pub type GenesisKeyhash = Hash<28>;

declare_hash_type!(BlockHash, 32);
declare_hash_type!(TxHash, 32);
declare_hash_type_with_bech32!(VrfKeyHash, 32, "vrf_vk");
declare_hash_type_with_bech32!(PoolId, 28, "pool");

declare_hash_type_with_bech32!(ConstitutionalCommitteeKeyHash, 28, "cc_hot");
declare_hash_type_with_bech32!(ConstitutionalCommitteeScriptHash, 28, "cc_hot_script");
declare_hash_type_with_bech32!(DrepKeyHash, 28, "drep");
declare_hash_type_with_bech32!(DRepScriptHash, 28, "drep_script");

/// Data hash used for metadata, anchors (SHA256)
pub type DataHash = Vec<u8>;

/// Compact transaction identifier (block_number, tx_index).
#[derive(
    Debug,
    Default,
    Clone,
    Copy,
    PartialEq,
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

impl From<UTxOIdentifier> for TxIdentifier {
    fn from(id: UTxOIdentifier) -> Self {
        Self::new(id.block_number(), id.tx_index())
    }
}

// Compact UTxO identifier (block_number, tx_index, output_index)
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
)]
pub struct UTxOIdentifier(#[n(0)] [u8; 8]);

impl UTxOIdentifier {
    pub fn new(block_number: u32, tx_index: u16, output_index: u16) -> Self {
        let mut buf = [0u8; 8];
        buf[..4].copy_from_slice(&block_number.to_be_bytes());
        buf[4..6].copy_from_slice(&tx_index.to_be_bytes());
        buf[6..].copy_from_slice(&output_index.to_be_bytes());
        Self(buf)
    }

    pub fn block_number(&self) -> u32 {
        u32::from_be_bytes(self.0[..4].try_into().unwrap())
    }

    pub fn tx_index(&self) -> u16 {
        u16::from_be_bytes(self.0[4..6].try_into().unwrap())
    }

    pub fn output_index(&self) -> u16 {
        u16::from_be_bytes(self.0[6..8].try_into().unwrap())
    }

    pub fn to_bytes(&self) -> [u8; 8] {
        self.0
    }
}

impl fmt::Display for UTxOIdentifier {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}:{}",
            self.block_number(),
            self.tx_index(),
            self.output_index()
        )
    }
}

// Full TxOutRef stored in UTxORegistry for UTxOIdentifier lookups
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct TxOutRef {
    pub tx_hash: TxHash,
    pub output_index: u16,
}

impl TxOutRef {
    pub fn new(tx_hash: TxHash, output_index: u16) -> Self {
        TxOutRef {
            tx_hash,
            output_index,
        }
    }
}

/// Slot
pub type Slot = u64;

/// Amount of Ada, in Lovelace
pub type Lovelace = u64;
pub type LovelaceDelta = i64;

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
    /// Stake address to withdraw to
    pub address: StakeAddress,

    /// Value to withdraw
    pub value: Lovelace,

    // Identifier of withdrawal tx
    pub tx_identifier: TxIdentifier,
}

/// Treasury pot account
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Pot {
    Reserves,
    Treasury,
    Deposits,
}

/// Pot Delta - internal change of pot values at genesis / era boundaries
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PotDelta {
    /// Stake address to withdraw to
    pub pot: Pot,

    /// Delta to apply
    pub delta: LovelaceDelta,
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
    pub ipv4: Option<[u8; 4]>,

    /// Optional IPv6 address
    pub ipv6: Option<[u8; 16]>,
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

/// Pool registration data
#[serde_as]
#[derive(
    Debug,
    Default,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Decode,
    minicbor::Encode,
    PartialEq,
    Eq,
)]
pub struct PoolRegistration {
    /// Operator pool key hash - used as ID
    #[serde_as(as = "Hex")]
    #[n(0)]
    pub operator: PoolId,

    /// VRF key hash
    #[serde_as(as = "Hex")]
    #[n(1)]
    pub vrf_key_hash: VrfKeyHash,

    /// Pledged Ada
    #[n(2)]
    pub pledge: Lovelace,

    /// Fixed cost
    #[n(3)]
    pub cost: Lovelace,

    /// Marginal cost (fraction)
    #[n(4)]
    pub margin: Ratio,

    /// Reward account
    #[n(5)]
    pub reward_account: StakeAddress,

    /// Pool owners by their key hash
    #[n(6)]
    pub pool_owners: Vec<StakeAddress>,

    // Relays
    #[n(7)]
    pub relays: Vec<Relay>,

    // Metadata
    #[n(8)]
    pub pool_metadata: Option<PoolMetadata>,
}

/// Pool retirement data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolRetirement {
    /// Operator pool key hash - used as ID
    pub operator: PoolId,

    /// Epoch it will retire at the end of
    pub epoch: u64,
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

/// Stake delegation data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeDelegation {
    /// Stake address
    pub stake_address: StakeAddress,

    /// Pool ID to delegate to
    pub operator: PoolId,
}

/// SPO total delegation data (for SPDD)
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct DelegatedStake {
    /// Active stake - UTXO values only (used for reward calcs)
    pub active: Lovelace,

    /// Active delegators count - delegators making active stakes (used for pool history)
    pub active_delegators_count: u64,

    /// Total 'live' stake - UTXO values and rewards (used for VRF)
    pub live: Lovelace,
}

/// SPO rewards data (for SPORewardsMessage)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SPORewards {
    /// Total rewards before distribution
    pub total_rewards: Lovelace,

    /// Pool operator's rewards
    pub operator_rewards: Lovelace,
}

/// Genesis key delegation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisKeyDelegation {
    /// Genesis hash
    pub genesis_hash: Hash<28>,

    /// Genesis delegate hash
    pub genesis_delegate_hash: PoolId,

    /// VRF key hash
    pub vrf_key_hash: VrfKeyHash,
}

/// Source of a MIR
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum InstantaneousRewardSource {
    Reserves,
    Treasury,
}

/// Target of a MIR
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum InstantaneousRewardTarget {
    StakeAddresses(Vec<(StakeAddress, i64)>),
    OtherAccountingPot(u64),
}

/// Move instantaneous reward
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MoveInstantaneousReward {
    /// Source
    pub source: InstantaneousRewardSource,

    /// Target
    pub target: InstantaneousRewardTarget,
}

/// Register stake (Conway version) = 'reg_cert'
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Registration {
    /// Stake address
    pub stake_address: StakeAddress,

    /// Deposit paid
    pub deposit: Lovelace,
}

/// Deregister stake (Conway version) = 'unreg_cert'
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Deregistration {
    /// Stake address
    pub stake_address: StakeAddress,

    /// Deposit to be refunded
    pub refund: Lovelace,
}

/// DRepChoice (=CDDL drep, badly named)
#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum DRepChoice {
    /// Address key
    Key(KeyHash),

    /// Script key
    Script(KeyHash),

    /// Abstain
    Abstain,

    /// No confidence
    NoConfidence,
}

/// Vote delegation (simple, existing registration) = vote_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VoteDelegation {
    /// Stake address
    pub stake_address: StakeAddress,

    // DRep choice
    pub drep: DRepChoice,
}

/// Stake+vote delegation (to SPO and DRep) = stake_vote_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeAndVoteDelegation {
    /// Stake address
    pub stake_address: StakeAddress,

    /// Pool
    pub operator: PoolId,

    // DRep vote
    pub drep: DRepChoice,
}

/// Stake delegation to SPO + registration = stake_reg_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeRegistrationAndDelegation {
    /// Stake address
    pub stake_address: StakeAddress,

    /// Pool
    pub operator: PoolId,

    // Deposit paid
    pub deposit: Lovelace,
}

/// Vote delegation to DRep + registration = vote_reg_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeRegistrationAndVoteDelegation {
    /// Stake address
    pub stake_address: StakeAddress,

    /// DRep choice
    pub drep: DRepChoice,

    // Deposit paid
    pub deposit: Lovelace,
}

/// All the trimmings:
/// Vote delegation to DRep + Stake delegation to SPO + registration
/// = stake_vote_reg_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeRegistrationAndStakeAndVoteDelegation {
    /// Stake credential
    pub stake_address: StakeAddress,

    /// Pool
    pub operator: PoolId,

    /// DRep choice
    pub drep: DRepChoice,

    // Deposit paid
    pub deposit: Lovelace,
}

/// Anchor
#[serde_as]
#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Anchor {
    /// Metadata URL
    pub url: String,

    /// Metadata hash
    #[serde_as(as = "Hex")]
    pub data_hash: DataHash,
}

pub type DRepCredential = Credential;

/// DRep Registration = reg_drep_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepRegistration {
    /// DRep credential
    pub credential: DRepCredential,

    /// Deposit paid
    pub deposit: Lovelace,

    /// Optional anchor
    pub anchor: Option<Anchor>,
}

/// DRep Deregistration = unreg_drep_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepDeregistration {
    /// DRep credential
    pub credential: DRepCredential,

    /// Deposit to refund
    pub refund: Lovelace,
}

/// DRep Update = update_drep_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepUpdate {
    /// DRep credential
    pub credential: DRepCredential,

    /// Optional anchor
    pub anchor: Option<Anchor>,
}

pub type CommitteeCredential = Credential;

/// Authorise a committee hot credential
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthCommitteeHot {
    /// Cold credential
    pub cold_credential: CommitteeCredential,

    /// Hot credential
    pub hot_credential: CommitteeCredential,
}

/// Resign a committee cold credential
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResignCommitteeCold {
    /// Cold credential
    pub cold_credential: CommitteeCredential,

    /// Associated anchor (reasoning?)
    pub anchor: Option<Anchor>,
}

/// Governance actions data structures

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone, Copy)]
pub struct ExUnits {
    pub mem: u64,
    pub steps: u64,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct ExUnitPrices {
    pub mem_price: RationalNumber,
    pub step_price: RationalNumber,
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

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct CostModel(Vec<i64>);

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

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct PoolVotingThresholds {
    pub motion_no_confidence: RationalNumber,
    pub committee_normal: RationalNumber,
    pub committee_no_confidence: RationalNumber,
    pub hard_fork_initiation: RationalNumber,
    pub security_voting_threshold: RationalNumber,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct DRepVotingThresholds {
    pub motion_no_confidence: RationalNumber,
    pub committee_normal: RationalNumber,
    pub committee_no_confidence: RationalNumber,
    pub update_constitution: RationalNumber,
    pub hard_fork_initiation: RationalNumber,
    pub pp_network_group: RationalNumber,
    pub pp_economic_group: RationalNumber,
    pub pp_technical_group: RationalNumber,
    pub pp_governance_group: RationalNumber,
    pub treasury_withdrawal: RationalNumber,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SoftForkRule {
    pub init_thd: u64,
    pub min_thd: u64,
    pub thd_decrement: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct TxFeePolicy {
    pub multiplier: u64,
    pub summand: u64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GenesisDelegate {
    #[serde_as(as = "Hex")]
    pub delegate: Hash<28>,
    #[serde_as(as = "Hex")]
    pub vrf: VrfKeyHash,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisDelegates(pub BTreeMap<GenesisKeyhash, GenesisDelegate>);

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

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ProtocolConsts {
    pub k: usize,
    pub protocol_magic: u32,
    pub vss_max_ttl: Option<u32>,
    pub vss_min_ttl: Option<u32>,
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

#[derive(Serialize, PartialEq, Deserialize, Debug, Clone)]
pub struct Constitution {
    pub anchor: Anchor,
    pub guardrail_script: Option<ScriptHash>,
}

#[serde_as]
#[derive(Serialize, PartialEq, Debug, Deserialize, Clone)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParameterChangeAction {
    pub previous_action_id: Option<GovActionId>,
    pub protocol_param_update: Box<ProtocolParamUpdate>,
    pub script_hash: Option<Vec<u8>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct HardForkInitiationAction {
    pub previous_action_id: Option<GovActionId>,
    pub protocol_version: protocol_params::ProtocolVersion,
}

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TreasuryWithdrawalsAction {
    #[serde_as(as = "Vec<(_, _)>")]
    pub rewards: HashMap<Vec<u8>, Lovelace>,
    pub script_hash: Option<Vec<u8>>,
}

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CommitteeChange {
    pub removed_committee_members: HashSet<CommitteeCredential>,
    #[serde_as(as = "Vec<(_, _)>")]
    pub new_committee_members: HashMap<CommitteeCredential, u64>,
    pub terms: RationalNumber,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateCommitteeAction {
    pub previous_action_id: Option<GovActionId>,
    pub data: CommitteeChange,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewConstitutionAction {
    pub previous_action_id: Option<GovActionId>,
    pub new_constitution: Constitution,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
}

#[derive(
    serde::Serialize, serde::Deserialize, Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Hash,
)]
pub enum Voter {
    ConstitutionalCommitteeKey(ConstitutionalCommitteeKeyHash),
    ConstitutionalCommitteeScript(ConstitutionalCommitteeScriptHash),
    DRepKey(DrepKeyHash),
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalProcedure {
    pub deposit: Lovelace,
    pub reward_account: StakeAddress,
    pub gov_action_id: GovActionId,
    pub gov_action: GovernanceAction,
    pub anchor: Anchor,
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

/// Certificate in a transaction
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TxCertificate {
    /// Default
    None(()),

    /// Stake registration
    StakeRegistration(StakeAddress),

    /// Stake de-registration
    StakeDeregistration(StakeAddress),

    /// Stake Delegation to a pool
    StakeDelegation(StakeDelegation),

    /// Pool registration
    PoolRegistration(PoolRegistration),

    /// Pool retirement
    PoolRetirement(PoolRetirement),

    /// Genesis key delegation
    GenesisKeyDelegation(GenesisKeyDelegation),

    /// Move instantaneous rewards
    MoveInstantaneousReward(MoveInstantaneousReward),

    /// New stake registration
    Registration(Registration),

    /// Stake deregistration
    Deregistration(Deregistration),

    /// Vote delegation
    VoteDelegation(VoteDelegation),

    /// Combined stake and vote delegation
    StakeAndVoteDelegation(StakeAndVoteDelegation),

    /// Stake registration and SPO delegation
    StakeRegistrationAndDelegation(StakeRegistrationAndDelegation),

    /// Stake registration and vote delegation
    StakeRegistrationAndVoteDelegation(StakeRegistrationAndVoteDelegation),

    /// Stake registration and combined SPO and vote delegation
    StakeRegistrationAndStakeAndVoteDelegation(StakeRegistrationAndStakeAndVoteDelegation),

    /// Authorise a committee hot credential
    AuthCommitteeHot(AuthCommitteeHot),

    /// Resign a committee cold credential
    ResignCommitteeCold(ResignCommitteeCold),

    /// DRep registration
    DRepRegistration(DRepRegistration),

    /// DRep deregistration
    DRepDeregistration(DRepDeregistration),

    /// DRep update
    DRepUpdate(DRepUpdate),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxCertificateWithPos {
    pub cert: TxCertificate,
    pub tx_identifier: TxIdentifier,
    pub cert_index: u64,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetInfoRecord {
    pub initial_mint_tx: TxIdentifier,
    pub mint_or_burn_count: u64,
    pub onchain_metadata: Option<Vec<u8>>,
    pub metadata_standard: Option<AssetMetadataStandard>,
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
    pub fn apply_delta(&mut self, delta: &ValueDelta) {
        if delta.lovelace > 0 {
            self.received.lovelace += delta.lovelace as u64;
        } else if delta.lovelace < 0 {
            self.sent.lovelace += (-delta.lovelace) as u64;
        }

        for (policy, assets) in &delta.assets {
            for a in assets {
                if a.amount > 0 {
                    Self::apply_asset(&mut self.received.assets, *policy, a.name, a.amount as u64);
                } else if a.amount < 0 {
                    Self::apply_asset(
                        &mut self.sent.assets,
                        *policy,
                        a.name,
                        a.amount.unsigned_abs(),
                    );
                }
            }
        }

        self.tx_count += 1;
    }

    fn apply_asset(
        target: &mut HashMap<[u8; 28], HashMap<AssetName, u64>>,
        policy: [u8; 28],
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
                terms: RationalNumber::from(1),
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
    fn serialize_stake_addres() -> Result<()> {
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
}
