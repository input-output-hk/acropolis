//! Core type definitions for Acropolis
// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]

use crate::address::{Address, StakeAddress};
use crate::rational_number::RationalNumber;
use anyhow::anyhow;
use bech32::{Bech32, Hrp};
use bitmask_enum::bitmask;
use chrono::{DateTime, Utc};
use hex::decode;
use serde::{Deserialize, Serialize, Serializer};
use serde_with::{hex::Hex, serde_as};
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};

/// Protocol era
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub enum Era {
    Byron,
    Shelley,
    Allegra,
    Mary,
    Alonzo,
    Babbage,
    Conway,
}

impl Default for Era {
    fn default() -> Era {
        Era::Byron
    }
}

impl Display for Era {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
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
    pub hash: Vec<u8>,

    /// Epoch number
    pub epoch: u64,

    /// Does this block start a new epoch?
    pub new_epoch: bool,

    /// Protocol era
    pub era: Era,
}

/// Individual address balance change
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressDelta {
    /// Address
    pub address: Address,

    /// Balance change
    pub delta: i64,
}

/// Stake balance change
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeAddressDelta {
    /// Address
    pub address: StakeAddress,

    /// Balance change
    pub delta: i64,
}

/// Transaction output (UTXO)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxOutput {
    /// Tx hash
    pub tx_hash: Vec<u8>,

    /// Output index in tx
    pub index: u64,

    /// Address data
    pub address: Address,

    /// Output value (Lovelace)
    pub value: u64,
    // todo: Implement datum    /// Datum (raw)
    // !!!    pub datum: Vec<u8>,
}

/// Transaction input (UTXO reference)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxInput {
    /// Tx hash of referenced UTXO
    pub tx_hash: Vec<u8>,

    /// Index of UTXO in referenced tx
    pub index: u64,
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

/// Key hash used for pool IDs etc.
pub type KeyHash = Vec<u8>;

/// Script identifier
pub type ScriptHash = KeyHash;

/// Address key hash
pub type AddrKeyhash = KeyHash;

/// Data hash used for metadata, anchors (SHA256)
pub type DataHash = Vec<u8>;

/// Amount of Ada, in Lovelace
pub type Lovelace = u64;
pub type LovelaceDelta = i64;

/// Rational number = numerator / denominator
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Eq, PartialEq)]
pub struct Ratio {
    pub numerator: u64,
    pub denominator: u64,
}

/// Withdrawal
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Withdrawal {
    /// Stake address to withdraw to
    pub address: StakeAddress,

    /// Value to withdraw
    pub value: Lovelace,
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

/// General credential
#[derive(
    Debug, Clone, Ord, Eq, PartialEq, PartialOrd, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Credential {
    /// Address key hash
    AddrKeyHash(KeyHash),

    /// Script hash
    ScriptHash(KeyHash),
}

impl Credential {
    fn hex_string_to_hash(hex_str: &str) -> anyhow::Result<KeyHash> {
        let key_hash = decode(hex_str.to_owned().into_bytes())?;
        if key_hash.len() != 28 {
            Err(anyhow!(
                "Invalid hash length for {:?}, expected 28 bytes",
                hex_str
            ))
        } else {
            Ok(key_hash)
        }
    }

    pub fn from_json_string(credential: &str) -> anyhow::Result<Self> {
        if let Some(hash) = credential.strip_prefix("scriptHash-") {
            Ok(Credential::ScriptHash(Self::hex_string_to_hash(hash)?))
        } else if let Some(hash) = credential.strip_prefix("keyHash-") {
            Ok(Credential::AddrKeyHash(Self::hex_string_to_hash(hash)?))
        } else {
            Err(anyhow!(
                "Incorrect credential {}, expected scriptHash- or keyHash- prefix",
                credential
            )
            .into())
        }
    }

    pub fn to_json_string(&self) -> String {
        match self {
            Self::ScriptHash(hash) => format!("scriptHash-{}", hex::encode(hash)),
            Self::AddrKeyHash(hash) => format!("keyHash-{}", hex::encode(hash)),
        }
    }

    pub fn get_hash(&self) -> KeyHash {
        match self {
            Self::AddrKeyHash(hash) => hash,
            Self::ScriptHash(hash) => hash,
        }
        .clone()
    }
}

pub type StakeCredential = Credential;

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

/// Pool relay
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

type RewardAccount = Vec<u8>;

/// Pool registration data
#[serde_as]
#[derive(
    Debug,
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
    pub operator: KeyHash,

    /// VRF key hash
    #[serde_as(as = "Hex")]
    #[n(1)]
    pub vrf_key_hash: KeyHash,

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
    #[serde_as(as = "Hex")]
    #[n(5)]
    pub reward_account: Vec<u8>,

    /// Pool owners by their key hash
    #[serde_as(as = "Vec<Hex>")]
    #[n(6)]
    pub pool_owners: Vec<KeyHash>,

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
    pub operator: KeyHash,

    /// Epoch it will retire at the end of
    pub epoch: u64,
}

/// Stake delegation data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeDelegation {
    /// Stake credential
    pub credential: StakeCredential,

    /// Pool ID to delegate to
    pub operator: KeyHash,
}

/// Genesis key delegation
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisKeyDelegation {
    /// Genesis hash
    pub genesis_hash: KeyHash,

    /// Genesis delegate hash
    pub genesis_delegate_hash: KeyHash,

    /// VRF key hash
    pub vrf_key_hash: KeyHash,
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
    StakeCredentials(Vec<(StakeCredential, i64)>),
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
    /// Stake credential
    pub credential: StakeCredential,

    /// Deposit paid
    pub deposit: Lovelace,
}

/// Deregister stake (Conway version) = 'unreg_cert'
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Deregistration {
    /// Stake credential
    pub credential: StakeCredential,

    /// Deposit to be refunded
    pub refund: Lovelace,
}

/// DRepChoice (=CDDL drep, badly named)
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
    /// Stake credential
    pub credential: StakeCredential,

    // DRep choice
    pub drep: DRepChoice,
}

/// Stake+vote delegation (to SPO and DRep) = stake_vote_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeAndVoteDelegation {
    /// Stake credential
    pub credential: StakeCredential,

    /// Pool
    pub operator: KeyHash,

    // DRep vote
    pub drep: DRepChoice,
}

/// Stake delegation to SPO + registration = stake_reg_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeRegistrationAndDelegation {
    /// Stake credential
    pub credential: StakeCredential,

    /// Pool
    pub operator: KeyHash,

    // Deposit paid
    pub deposit: Lovelace,
}

/// Vote delegation to DRep + registration = vote_reg_deleg_cert
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeRegistrationAndVoteDelegation {
    /// Stake credential
    pub credential: StakeCredential,

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
    pub credential: StakeCredential,

    /// Pool
    pub operator: KeyHash,

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

pub type UnitInterval = RationalNumber;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Deserialize)]
pub struct GovActionId {
    pub transaction_id: DataHash,
    pub action_index: u8,
}

impl GovActionId {
    pub fn to_bech32(&self) -> String {
        let mut buf = self.transaction_id.clone();
        buf.push(self.action_index);

        let gov_action_hrp: Hrp = Hrp::parse("gov_action").unwrap();
        bech32::encode::<Bech32>(gov_action_hrp, &buf)
            .unwrap_or_else(|e| format!("Cannot convert {:?} to bech32: {e}", self.transaction_id))
    }

    pub fn from_bech32(bech32_str: &str) -> Result<Self, anyhow::Error> {
        let (hrp, data) = bech32::decode(bech32_str)?;

        if hrp != Hrp::parse("gov_action")? {
            return Err(anyhow!("Invalid HRP, expected 'gov_action', got: {}", hrp));
        }

        if data.len() < 33 {
            return Err(anyhow!("Invalid Bech32 governance action"));
        }

        let transaction_id: DataHash = data[..32].to_vec();
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

impl Serialize for GovActionId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Convert GovActionId to Bech32 before serializing
        let bech32_str = self.to_bech32();
        serializer.serialize_str(&bech32_str)
    }
}

impl Display for GovActionId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_bech32())
    }
}

type CostModel = Vec<i64>;

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct CostModels {
    pub plutus_v1: Option<CostModel>,
    pub plutus_v2: Option<CostModel>,
    pub plutus_v3: Option<CostModel>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct PoolVotingThresholds {
    pub motion_no_confidence: UnitInterval,
    pub committee_normal: UnitInterval,
    pub committee_no_confidence: UnitInterval,
    pub hard_fork_initiation: UnitInterval,
    pub security_voting_threshold: UnitInterval,
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct DRepVotingThresholds {
    pub motion_no_confidence: UnitInterval,
    pub committee_normal: UnitInterval,
    pub committee_no_confidence: UnitInterval,
    pub update_constitution: UnitInterval,
    pub hard_fork_initiation: UnitInterval,
    pub pp_network_group: UnitInterval,
    pub pp_economic_group: UnitInterval,
    pub pp_technical_group: UnitInterval,
    pub pp_governance_group: UnitInterval,
    pub treasury_withdrawal: UnitInterval,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SoftForkRule {
    pub init_thd: u64,
    pub min_thd: u64,
    pub thd_decrement: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxFeePolicy {
    pub multiplier: u64,
    pub summand: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolConsts {
    pub k: usize,
    pub protocol_magic: u32,
    pub vss_max_ttl: Option<u32>,
    pub vss_min_ttl: Option<u32>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolVersion {
    pub minor: u64,
    pub major: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NonceVariant {
    NeutralNonce,
    Nonce,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Nonce {
    pub tag: NonceVariant,
    pub hash: Option<Vec<u8>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShelleyProtocolParams {
    pub protocol_version: ProtocolVersion,
    pub max_tx_size: u32,
    pub max_block_body_size: u32,
    pub max_block_header_size: u32,
    pub key_deposit: u64,
    pub min_utxo_value: u64,

    pub minfee_a: u32,
    pub minfee_b: u32,
    pub pool_deposit: u64,

    /// AKA desired_number_of_stake_pools, n_opt, k parameter
    pub stake_pool_target_num: u32,
    pub min_pool_cost: u64,

    /// AKA eMax, e_max
    pub pool_retire_max_epoch: u64,
    pub extra_entropy: Nonce,
    pub decentralisation_param: RationalNumber,

    /// AKA Rho, expansion_rate
    pub monetary_expansion: RationalNumber,

    /// AKA Tau, treasury_growth_rate
    pub treasury_cut: RationalNumber,

    /// AKA a0
    pub pool_pledge_influence: RationalNumber,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AlonzoParams {
    pub lovelace_per_utxo_word: u64,
    pub execution_prices: ExUnitPrices,
    pub max_tx_ex_units: ExUnits,
    pub max_block_ex_units: ExUnits,
    pub max_value_size: u32,
    pub collateral_percentage: u32,
    pub max_collateral_inputs: u32,
    pub plutus_v1_cost_model: Option<CostModel>,
    pub plutus_v2_cost_model: Option<CostModel>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ByronParams {
    pub block_version_data: BlockVersionData,
    pub fts_seed: Option<Vec<u8>>,
    pub protocol_consts: ProtocolConsts,
    pub start_time: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum NetworkId {
    Testnet,
    Mainnet,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShelleyParams {
    pub active_slots_coeff: f32,
    pub epoch_length: u32,
    pub max_kes_evolutions: u32,
    pub max_lovelace_supply: u64,
    pub network_id: NetworkId,
    pub network_magic: u32,
    pub protocol_params: ShelleyProtocolParams,
    pub security_param: u32,
    pub slot_length: u32,
    pub slots_per_kes_period: u32,
    pub system_start: DateTime<Utc>,
    pub update_quorum: u32,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ConwayParams {
    pub pool_voting_thresholds: PoolVotingThresholds,
    pub d_rep_voting_thresholds: DRepVotingThresholds,
    pub committee_min_size: u64,
    pub committee_max_term_length: u32,
    pub gov_action_lifetime: u32,
    pub gov_action_deposit: u64,
    pub d_rep_deposit: u64,
    pub d_rep_activity: u32,
    pub min_fee_ref_script_cost_per_byte: RationalNumber,
    pub plutus_v3_cost_model: CostModel,
    pub constitution: Constitution,
    pub committee: Committee,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolParams {
    pub alonzo: Option<AlonzoParams>,
    pub byron: Option<ByronParams>,
    pub shelley: Option<ShelleyParams>,
    pub conway: Option<ConwayParams>,
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

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolParamUpdate {
    /// AKA txFeePerByte, tx_fee_per_byte (Shelley)
    pub minfee_a: Option<u64>,

    /// AKA txFeeFixed, tx_fee_fixed (Shelley)
    pub minfee_b: Option<u64>,

    /// (Shelley)
    pub max_block_body_size: Option<u64>,

    /// AKA max_tx_size (Shelley)
    pub max_transaction_size: Option<u64>,

    /// (Shelley)
    pub max_block_header_size: Option<u64>,

    /// (Shelley)
    pub key_deposit: Option<Lovelace>,

    /// (Shelley)
    pub pool_deposit: Option<Lovelace>,

    /// AKA poolRetireMaxEpoch, eMax (Shelley)
    pub maximum_epoch: Option<u64>,

    /// AKA stakePoolTargetNum, nOpt (Shelley)
    pub desired_number_of_stake_pools: Option<u64>,

    /// AKA a0 (Shelley)
    pub pool_pledge_influence: Option<RationalNumber>,

    /// AKA rho, monetary_expansion (Shelley)
    pub expansion_rate: Option<UnitInterval>,

    /// AKA tau, treasury_cut (Shelley)
    pub treasury_growth_rate: Option<UnitInterval>,

    /// (Shelley)
    pub min_pool_cost: Option<Lovelace>,

    /// AKA lovelacePerUTxOWord, utxoCostPerWord (Alonzo)
    /// TODO: was there any moment, when this value had different
    /// meaning? (words were recounted to bytes)
    pub ada_per_utxo_byte: Option<Lovelace>,

    /// AKA plutus_v1_cost_model, plutus_v2_cost_model (Shelley)
    /// plutus_v3_cost_model (Conway)
    pub cost_models_for_script_languages: Option<CostModels>,

    /// AKA execution_prices (Alonzo)
    pub execution_costs: Option<ExUnitPrices>,

    /// (Alonzo)
    pub max_tx_ex_units: Option<ExUnits>,

    /// (Alonzo)
    pub max_block_ex_units: Option<ExUnits>,

    /// (Alonzo)
    pub max_value_size: Option<u64>,

    /// (Alonzo)
    pub collateral_percentage: Option<u64>,

    /// (Alonzo)
    pub max_collateral_inputs: Option<u64>,

    /// (Conway)
    pub pool_voting_thresholds: Option<PoolVotingThresholds>,

    /// (Conway)
    pub drep_voting_thresholds: Option<DRepVotingThresholds>,

    /// (Conway)
    pub min_committee_size: Option<u64>,

    /// AKA committee_max_term_limit (Conway)
    pub committee_term_limit: Option<u64>,

    /// AKA gov_action_lifetime (Cownay)
    pub governance_action_validity_period: Option<u64>,

    /// AKA gov_action_deposit (Conway)
    pub governance_action_deposit: Option<Lovelace>,

    /// AKA d_rep_deposit (Conway)
    pub drep_deposit: Option<Lovelace>,

    /// AKA drep_inactivity (Conway)
    pub drep_inactivity_period: Option<u64>,

    /// AKA min_fee_ref_script_cost_per_byte (Conway)
    pub minfee_refscript_cost_per_byte: Option<UnitInterval>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Constitution {
    pub anchor: Anchor,
    pub guardrail_script: Option<ScriptHash>,
}

#[serde_as]
#[derive(Serialize, Debug, Deserialize, Clone)]
pub struct Committee {
    #[serde_as(as = "Vec<(_, _)>")]
    pub members: HashMap<CommitteeCredential, u64>,
    pub threshold: RationalNumber,
}

impl Committee {
    pub fn is_empty(&self) -> bool {
        return self.members.len() == 0;
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
    pub protocol_version: (u64, u64),
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
    pub terms: UnitInterval,
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

#[derive(
    serde::Serialize, serde::Deserialize, Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Hash,
)]
pub enum Voter {
    ConstitutionalCommitteeKey(AddrKeyhash),
    ConstitutionalCommitteeScript(ScriptHash),
    DRepKey(AddrKeyhash),
    DRepScript(ScriptHash),
    StakePoolKey(AddrKeyhash),
}

impl Voter {
    pub fn to_bech32(&self, hrp: &str, buf: &[u8]) -> String {
        let voter_hrp: Hrp = Hrp::parse(hrp).unwrap();
        bech32::encode::<Bech32>(voter_hrp, &buf)
            .unwrap_or_else(|e| format!("Cannot convert {:?} to bech32: {e}", self))
    }
}

impl Display for Voter {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Voter::ConstitutionalCommitteeKey(h) => write!(f, "{}", self.to_bech32("cc_hot", &h)),
            Voter::ConstitutionalCommitteeScript(s) => {
                write!(f, "{}", self.to_bech32("cc_hot_script", &s))
            }
            Voter::DRepKey(k) => write!(f, "{}", self.to_bech32("drep", &k)),
            Voter::DRepScript(s) => write!(f, "{}", self.to_bech32("drep_script", &s)),
            Voter::StakePoolKey(k) => write!(f, "{}", self.to_bech32("pool", &k)),
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VotesCount {
    pub committee: u64,
    pub drep: u64,
    pub pool: u64,
}

impl VotesCount {
    pub fn zero() -> Self {
        Self {
            committee: 0,
            drep: 0,
            pool: 0,
        }
    }

    pub fn majorizes(&self, v: &VotesCount) -> bool {
        self.committee >= v.committee && self.drep >= v.drep && self.pool >= v.pool
    }
}

impl Display for VotesCount {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "c{}:d{}:p{}", self.committee, self.drep, self.pool)
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct VotingOutcome {
    pub procedure: ProposalProcedure,
    pub votes_cast: VotesCount,
    pub votes_threshold: VotesCount,
    pub accepted: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalProcedure {
    pub deposit: Lovelace,
    pub reward_account: RewardAccount,
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
    NoConfidence,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum GovernanceOutcomeVariant {
    EnactStateElem(EnactStateElem),
    TreasuryWithdrawal(TreasuryWithdrawalsAction),
    NoAction,
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeCredentialWithPos {
    pub stake_credential: StakeCredential,
    pub tx_index: u64,
    pub cert_index: u64,
}

/// Certificate in a transaction
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TxCertificate {
    /// Default
    None(()),

    /// Stake registration
    StakeRegistration(StakeCredentialWithPos),

    /// Stake de-registration
    StakeDeregistration(StakeCredential),

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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    fn make_committee_credential(addr_key_hash: bool, val: u8) -> CommitteeCredential {
        if addr_key_hash {
            Credential::AddrKeyHash(vec![val])
        } else {
            Credential::ScriptHash(vec![val])
        }
    }

    #[test]
    fn governance_serialization_test() -> Result<()> {
        let gov_action_id = GovActionId::default();

        let mut voting = VotingProcedures::default();
        voting.votes.insert(
            Voter::StakePoolKey(vec![1, 2, 3, 4]),
            SingleVoterVotes::default(),
        );

        let mut single_voter = SingleVoterVotes::default();
        single_voter.voting_procedures.insert(
            gov_action_id.clone(),
            VotingProcedure {
                anchor: None,
                vote: Vote::Abstain,
            },
        );
        voting.votes.insert(
            Voter::StakePoolKey(vec![1, 2, 3, 4]),
            SingleVoterVotes::default(),
        );
        println!("Json: {}", serde_json::to_string(&voting)?);

        let gov_action = GovernanceAction::UpdateCommittee(UpdateCommitteeAction {
            previous_action_id: None,
            data: CommitteeChange {
                removed_committee_members: HashSet::from_iter(
                    vec![
                        make_committee_credential(true, 48),
                        make_committee_credential(false, 12),
                    ]
                    .into_iter(),
                ),
                new_committee_members: HashMap::from_iter(
                    vec![(make_committee_credential(false, 87), 1234)].into_iter(),
                ),
                terms: RationalNumber::from(1),
            },
        });

        let proposal = ProposalProcedure {
            deposit: 9876,
            reward_account: vec![7, 4, 6, 7],
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
}
