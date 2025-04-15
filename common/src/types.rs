//! Core type definitions for Acropolis
// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, hex::Hex};

/// Block status
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum BlockStatus
{
    Bootstrap,   // Pseudo-block from bootstrap data
    Immutable,   // Now immutable (more than 'k' blocks ago)
    Volatile,    // Volatile, in sequence
    RolledBack,  // Volatile, restarted after rollback
}

impl Default for BlockStatus {
    fn default() -> Self { Self::Immutable }
}

/// Block info, shared across multiple messages
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
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
}

/// a Byron-era address
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ByronAddress {
    /// Raw payload
    pub payload: Vec<u8>,
}

/// Address network identifier
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum AddressNetwork {
    /// Mainnet
    Main,

    /// Testnet
    Test,
}

impl Default for AddressNetwork {
    fn default() -> Self { Self::Main }
}

type ScriptHash = KeyHash;
type AddrKeyhash = KeyHash;

/// A Shelley-era address - payment part
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ShelleyAddressPaymentPart {
    /// Payment to a key
    PaymentKeyHash(KeyHash),

    /// Payment to a script
    ScriptHash(ScriptHash),
}

impl Default for ShelleyAddressPaymentPart {
    fn default() -> Self { Self::PaymentKeyHash(Vec::new()) }
}

/// Delegation pointer
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShelleyAddressPointer {
    /// Slot number
    pub slot: u64,

    /// Transaction index within the slot
    pub tx_index: u64,

    /// Certificate index within the transaction
    pub cert_index: u64,
}

/// A Shelley-era address - delegation part
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ShelleyAddressDelegationPart {
    /// No delegation (enterprise addresses)
    None,

    /// Delegation to stake key
    StakeKeyHash(Vec<u8>),

    /// Delegation to script key
    ScriptHash(Vec<u8>),

    /// Delegation to pointer
    Pointer(ShelleyAddressPointer),
}

impl Default for ShelleyAddressDelegationPart {
    fn default() -> Self { Self::None }
}

/// A Shelley-era address
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ShelleyAddress {
    /// Network id
    pub network: AddressNetwork,

    /// Payment part
    pub payment: ShelleyAddressPaymentPart,

    /// Delegation part
    pub delegation: ShelleyAddressDelegationPart,
}

/// Payload of a stake address
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StakeAddressPayload {
    /// Stake key
    StakeKeyHash(Vec<u8>),

    /// Script hash
    ScriptHash(Vec<u8>),    
}

impl Default for StakeAddressPayload {
    fn default() -> Self { Self::StakeKeyHash(Vec::new()) }
}

/// A stake address
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeAddress {
    /// Network id
    pub network: AddressNetwork,

    /// Payload
    pub payload: StakeAddressPayload,
}

/// A Cardano address
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Address {
    None,
    Byron(ByronAddress),
    Shelley(ShelleyAddress),
    Stake(StakeAddress),
}

impl Default for Address {
    fn default() -> Self { Self::None }
}

/// Individual address balance change
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressDelta {
    /// Address
    pub address: Address,

    /// Balance change
    pub delta: i64,
}

/// Transaction output (UTXO)
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
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
    fn default() -> Self { Self::None(()) }
}

/// Key hash used for pool IDs etc.
pub type KeyHash = Vec<u8>;

/// Data hash used for metadata, anchors (SHA256)
pub type DataHash = Vec<u8>;

/// Amount of Ada, in Lovelace
pub type Lovelace = u64;

/// Rational number = numerator / denominator
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Ratio {
    pub numerator: u64,
    pub denominator: u64,
}

/// General credential
#[derive(Debug, Clone, Ord, Eq, PartialEq, PartialOrd, Hash,
        serde::Serialize, serde::Deserialize)]
pub enum Credential {
    /// Address key hash
    AddrKeyHash(KeyHash),

    /// Script hash
    ScriptHash(KeyHash),
}

impl Default for Credential {
    fn default() -> Self { Self::AddrKeyHash(Vec::new()) }
}

pub type StakeCredential = Credential;

/// Relay single host address
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SingleHostAddr {
    /// Optional port number
    pub port: Option<u16>,

    /// Optional IPv4 address
    pub ipv4: Option<[u8; 4]>,

    /// Optional IPv6 address
    pub ipv6: Option<[u8; 16]>,
}

/// Relay hostname
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SingleHostName {
    /// Optional port number
    pub port: Option<u16>,

    /// DNS name (A or AAAA record)
    pub dns_name: String,
}

/// Relay multihost (SRV)
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct MultiHostName {
    /// DNS name (SRC record)
    pub dns_name: String,
}

/// Pool relay
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Relay {
    SingleHostAddr(SingleHostAddr),
    SingleHostName(SingleHostName),
    MultiHostName(MultiHostName),
}

impl Default for Relay {
    fn default() -> Self { Self::SingleHostAddr(SingleHostAddr::default()) }
}

/// Pool metadata
#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolMetadata {
    /// Metadata URL
    pub url: String,

    /// Metadata hash
    #[serde_as(as = "Hex")]
    pub hash: DataHash,
}

type RewardAccount = Vec<u8>;

/// Pool registration data
#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolRegistration {
    /// Operator pool key hash - used as ID
    #[serde_as(as = "Hex")]
    pub operator: KeyHash,

    /// VRF key hash
    #[serde_as(as = "Hex")]
    pub vrf_key_hash: KeyHash,

    /// Pledged Ada
    pub pledge: Lovelace,

    /// Fixed cost
    pub cost: Lovelace,

    /// Marginal cost (fraction)
    pub margin: Ratio,

    /// Reward account
    #[serde_as(as = "Hex")]
    pub reward_account: Vec<u8>,

    /// Pool owners by their key hash
    #[serde_as(as = "Vec<Hex>")]
    pub pool_owners: Vec<KeyHash>,

    // Relays
    pub relays: Vec<Relay>,

    // Metadata
    pub pool_metadata: Option<PoolMetadata>,
}

/// Pool retirement data
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolRetirement {
    /// Operator pool key hash - used as ID
    pub operator: KeyHash,

    /// Epoch it will retire at the end of
    pub epoch: u64,
}

/// Stake delegation data
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeDelegation {
    /// Stake credential
    pub credential: StakeCredential,

    /// Pool ID to delegate to
    pub operator: KeyHash,
}

/// Genesis key delegation
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
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

impl Default for InstantaneousRewardSource {
    fn default() -> Self { Self::Reserves }
}

/// Target of a MIR
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum InstantaneousRewardTarget {
    StakeCredentials(Vec<(StakeCredential, u64)>),
    OtherAccountingPot(u64),
}

impl Default for InstantaneousRewardTarget {
    fn default() -> Self { Self::OtherAccountingPot(0) }
}

/// Move instantaneous reward
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct MoveInstantaneosReward {
    /// Source
    pub source: InstantaneousRewardSource,

    /// Target
    pub target: InstantaneousRewardTarget,
}

/// Register stake (Conway version) = 'reg_cert'
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Registration {
    /// Stake credential
    pub credential: StakeCredential,

    /// Deposit paid
    pub deposit: Lovelace,
}

/// Deregister stake (Conway version) = 'unreg_cert'
 #[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
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

impl Default for DRepChoice {
    fn default() -> Self { Self::Abstain }
}

/// Vote delegation (simple, existing registration) = vote_deleg_cert
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct VoteDelegation {
    /// Stake credential
    pub credential: StakeCredential,

    // DRep choice
    pub drep: DRepChoice,
}

/// Stake+vote delegation (to SPO and DRep) = stake_vote_deleg_cert
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeAndVoteDelegation {
    /// Stake credential
    pub credential: StakeCredential,

    /// Pool
    pub operator: KeyHash,
   
    // DRep vote
    pub drep: DRepChoice,
}

/// Stake delegation to SPO + registration = stake_reg_deleg_cert
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeRegistrationAndDelegation {
    /// Stake credential
    pub credential: StakeCredential,

    /// Pool
    pub operator: KeyHash,
   
    // Deposit paid
    pub deposit: Lovelace,
}

/// Vote delegation to DRep + registration = vote_reg_deleg_cert
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Default, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Anchor {
    /// Metadata URL
    pub url: String,

    /// Metadata hash
    #[serde_as(as = "Hex")]
    pub data_hash: DataHash,
}

pub type DRepCredential = Credential;

/// DRep Registration = reg_drep_cert
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepRegistration {
    /// DRep credential
    pub credential: DRepCredential,

    /// Deposit paid
    pub deposit: Lovelace,

    /// Optional anchor
    pub anchor: Option<Anchor>,
}

/// DRep Deregistration = unreg_drep_cert
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepDeregistration {
    /// DRep credential
    pub credential: DRepCredential,

    /// Deposit to refund
    pub refund: Lovelace,
}

/// DRep Update = update_drep_cert
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepUpdate {
    /// DRep credential
    pub credential: DRepCredential,

    /// Optional anchor
    pub anchor: Option<Anchor>,
}

type CommitteeCredential = Credential;

/// Authorise a committee hot credential
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuthCommitteeHot {
    /// Cold credential
    pub cold_credential: CommitteeCredential,

    /// Hot credential
    pub hot_credential: CommitteeCredential,
}

/// Resign a committee cold credential
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
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

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, Eq, Clone)]
pub struct RationalNumber {
    pub numerator: u64,
    pub denominator: u64,
}

pub type UnitInterval = RationalNumber;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct GovActionId {
    pub transaction_id: DataHash,
    pub action_index: u32,
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

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolParamUpdate {
    pub minfee_a: Option<u64>,
    pub minfee_b: Option<u64>,
    pub max_block_body_size: Option<u64>,
    pub max_transaction_size: Option<u64>,
    pub max_block_header_size: Option<u64>,
    pub key_deposit: Option<Lovelace>,
    pub pool_deposit: Option<Lovelace>,
    pub maximum_epoch: Option<u64>,
    pub desired_number_of_stake_pools: Option<u64>,
    pub pool_pledge_influence: Option<RationalNumber>,
    pub expansion_rate: Option<UnitInterval>,
    pub treasury_growth_rate: Option<UnitInterval>,

    pub min_pool_cost: Option<Lovelace>,
    pub ada_per_utxo_byte: Option<Lovelace>,
    pub cost_models_for_script_languages: Option<CostModels>,
    pub execution_costs: Option<ExUnitPrices>,
    pub max_tx_ex_units: Option<ExUnits>,
    pub max_block_ex_units: Option<ExUnits>,
    pub max_value_size: Option<u64>,
    pub collateral_percentage: Option<u64>,
    pub max_collateral_inputs: Option<u64>,

    pub pool_voting_thresholds: Option<PoolVotingThresholds>,
    pub drep_voting_thresholds: Option<DRepVotingThresholds>,
    pub min_committee_size: Option<u64>,
    pub committee_term_limit: Option<u64>,
    pub governance_action_validity_period: Option<u64>,
    pub governance_action_deposit: Option<Lovelace>,
    pub drep_deposit: Option<Lovelace>,
    pub drep_inactivity_period: Option<u64>,
    pub minfee_refscript_cost_per_byte: Option<UnitInterval>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Constitution {
    pub anchor: Anchor,
    pub guardrail_script: Option<ScriptHash>,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParameterChangeAction {
    pub action_id: Option<GovActionId>,
    pub protocol_param_update: Box<ProtocolParamUpdate>,
    pub script_hash: Option<Vec<u8>>
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct HardForkInitiationAction {
    pub action_id: Option<GovActionId>,
    pub protocol_version: (u64, u64),
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct TreasuryWithdrawalsAction {
    pub rewards: HashMap<Vec<u8>, Lovelace>,
    pub script_hash: Option<Vec<u8>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateCommitteeAction {
    pub action_id: Option<GovActionId>,
    pub committee: HashSet<CommitteeCredential>,
    pub committee_thresold: HashMap<CommitteeCredential, u64>,
    pub terms: UnitInterval,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewConstitutionAction {
    pub action_id: Option<GovActionId>,
    pub new_constitution: Constitution
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum GovernanceAction {
    ParameterChange(ParameterChangeAction),
    HardForkInitiation(HardForkInitiationAction),
    TreasuryWithdrawals(TreasuryWithdrawalsAction),
    NoConfidence(Option<GovActionId>),
    UpdateCommittee(UpdateCommitteeAction),
    NewConstitution(NewConstitutionAction),
    Information
}

impl GovernanceAction {
    pub fn get_gov_action_id(&self) -> Option<&GovActionId> {
        match &self {
            GovernanceAction::ParameterChange(p) => p.action_id.as_ref(),
            GovernanceAction::HardForkInitiation(h) => h.action_id.as_ref(),
            GovernanceAction::TreasuryWithdrawals(_t) => None,
            GovernanceAction::NoConfidence(n) => n.as_ref(),
            GovernanceAction::UpdateCommittee(u) => u.action_id.as_ref(),
            GovernanceAction::NewConstitution(n) => n.action_id.as_ref(),
            GovernanceAction::Information => None
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Hash)]
pub enum Voter {
    ConstitutionalCommitteeKey(AddrKeyhash),
    ConstitutionalCommitteeScript(ScriptHash),
    DRepKey(AddrKeyhash),
    DRepScript(ScriptHash),
    StakePoolKey(AddrKeyhash),
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
    pub anchor: Option<Anchor>
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VotingProcedures {
    pub votes: HashMap <Voter, HashMap<GovActionId, VotingProcedure>>
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalProcedure {
    pub deposit: Lovelace,
    pub reward_account: RewardAccount,
    pub gov_action: GovernanceAction,
    pub anchor: Anchor,
}

/// Certificate in a transaction
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum TxCertificate {
    /// Default
    None(()),

    /// Stake registration
    StakeRegistration(StakeCredential),

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
    MoveInstantaneousReward(MoveInstantaneosReward),

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
