//! Core type definitions for Acropolis
// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]

use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use anyhow::anyhow;
use serde::{Deserialize, Serialize};
use serde_with::{hex::Hex, serde_as};
use bech32::{Bech32, Hrp};
use hex::decode;
use bitmask_enum::bitmask;
use crate::rational_number::RationalNumber;
use crate::address::{Address, StakeAddress};

/// Protocol era
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Era
{
    Byron,
    Shelley,
    Allegra,
    Mary,
    Alonzo,
    Babbage,
    Conway,
}

/// Block status
#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum BlockStatus
{
    Bootstrap,   // Pseudo-block from bootstrap data
    Immutable,   // Now immutable (more than 'k' blocks ago)
    Volatile,    // Volatile, in sequence
    RolledBack,  // Volatile, restarted after rollback
}

/// Block info, shared across multiple messages
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
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
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressDelta {
    /// Address
    pub address: Address,

    /// Balance change
    pub delta: i64,
}

/// Stake balance change
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeAddressDelta {
    /// Address
    pub address: StakeAddress,

    /// Balance change
    pub delta: i64
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

/// Script identifier
pub type ScriptHash = KeyHash;

/// Address key hash
pub type AddrKeyhash = KeyHash;

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

impl Credential {
    fn hex_string_to_hash(hex_str: &str) -> anyhow::Result<KeyHash> {
        let key_hash = decode(hex_str.to_owned().into_bytes())?;
        if key_hash.len() != 28 {
            Err(anyhow!("Invalid hash length for {:?}, expected 28 bytes", hex_str))
        }
        else {
            Ok(key_hash)
        }
    }

    pub fn from_json_string (credential: &str) -> anyhow::Result<Self> {
        if let Some(hash) = credential.strip_prefix("scriptHash-") {
            Ok(Credential::ScriptHash(Self::hex_string_to_hash(hash)?))
        }
        else if let Some(hash) = credential.strip_prefix("keyHash-") {
            Ok(Credential::AddrKeyHash(Self::hex_string_to_hash(hash)?))
        }
        else {
            Err(anyhow!("Incorrect credential {}, expected scriptHash- or keyHash- prefix", credential).into())
        }
    }

    pub fn get_hash(&self) -> KeyHash {
        match self {
            Self::AddrKeyHash(hash) => hash,
            Self::ScriptHash(hash) => hash
        }.clone()
    }
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

pub type CommitteeCredential = Credential;

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

pub type UnitInterval = RationalNumber;

#[derive(Debug, Default, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
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

    pub fn set_action_index(&mut self, action_index: usize) -> Result<&Self, anyhow::Error> {
        if action_index >= 256 {
            return Err(anyhow!("Action_index {action_index} >= 256"))
        }

        self.action_index = action_index as u8;
        Ok(self)
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
pub struct ConwayGenesisParams {
    pub pool_voting_thresholds: PoolVotingThresholds,
    pub d_rep_voting_thresholds: DRepVotingThresholds,
    pub committee_min_size: u64,
    pub committee_max_term_length: u32,
    pub gov_action_lifetime: u32,
    pub gov_action_deposit: u64,
    pub d_rep_deposit: u64,
    pub d_rep_activity: u32,
    pub min_fee_ref_script_cost_per_byte: RationalNumber,
    pub plutus_v3_cost_model: Vec<i64>,
    pub constitution: Constitution,
    pub committee: Committee,
}

#[bitmask(u8)]
#[derive(serde::Serialize, serde::Deserialize)]
pub enum ProtocolParamType {
    NetworkGroup,
    EconomicGroup,
    TechnicalGroup,
    GovernanceGroup,
    SecurityProperty
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

#[serde_as]
#[derive(Serialize, Debug, Deserialize, Clone)]
pub struct Committee {
    #[serde_as(as = "Vec<(_, _)>")]
    pub members: HashMap<CommitteeCredential, u64>,
    pub threshold: RationalNumber,
}

impl Committee {
    pub fn is_empty(&self) -> bool { return self.members.len() == 0; }
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParameterChangeAction {
    pub previous_action_id: Option<GovActionId>,
    pub protocol_param_update: Box<ProtocolParamUpdate>,
    pub script_hash: Option<Vec<u8>>
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct HardForkInitiationAction {
    pub previous_action_id: Option<GovActionId>,
    pub protocol_version: (u64, u64),
}

#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct TreasuryWithdrawalsAction {
    #[serde_as(as = "Vec<(_, _)>")]
    pub rewards: HashMap<Vec<u8>, Lovelace>,
    pub script_hash: Option<Vec<u8>>,
}

#[serde_as]
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UpdateCommitteeAction {
    pub previous_action_id: Option<GovActionId>,
    pub removed_committee_members: HashSet<CommitteeCredential>,
    #[serde_as(as = "Vec<(_, _)>")]
    pub new_committee_members: HashMap<CommitteeCredential, u64>,
    pub terms: UnitInterval,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewConstitutionAction {
    pub previous_action_id: Option<GovActionId>,
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

#[derive(serde::Serialize, serde::Deserialize, Debug, PartialEq, PartialOrd, Eq, Ord, Clone, Hash)]
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
            Voter::ConstitutionalCommitteeScript(s) => write!(f, "{}", self.to_bech32("cc_hot_script", &s)),
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
    pub anchor: Option<Anchor>
}

#[serde_as]
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SingleVoterVotes {
    #[serde_as(as = "Vec<(_, _)>")]
    pub voting_procedures: HashMap<GovActionId, VotingProcedure>
}

#[serde_as]
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct VotingProcedures {
    #[serde_as(as = "Vec<(_, _)>")]
    pub votes: HashMap <Voter, SingleVoterVotes>
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProposalProcedure {
    pub deposit: Lovelace,
    pub reward_account: RewardAccount,
    pub gov_action_id: GovActionId,
    pub gov_action: GovernanceAction,
    pub anchor: Anchor,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeCredentialWithPos {
    pub stake_credential: StakeCredential,
    pub tx_index: u64,
    pub cert_index: u64
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

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    fn make_committee_credential(addr_key_hash: bool, val: u8) -> CommitteeCredential {
        if addr_key_hash {
            Credential::AddrKeyHash(vec!(val))
        }
        else {
            Credential::ScriptHash(vec!(val))
        }
    }

    #[test]
    fn governance_serialization_test() -> Result<()> {
        let gov_action_id = GovActionId::default();

        let mut voting = VotingProcedures::default();
        voting.votes.insert(Voter::StakePoolKey(vec![1,2,3,4]), SingleVoterVotes::default());

        let mut single_voter = SingleVoterVotes::default();
        single_voter.voting_procedures.insert(gov_action_id.clone(), VotingProcedure { anchor: None, vote: Vote::Abstain });
        voting.votes.insert(Voter::StakePoolKey(vec![1,2,3,4]), SingleVoterVotes::default());
        println!("Json: {}", serde_json::to_string(&voting)?);

        let gov_action = GovernanceAction::UpdateCommittee(UpdateCommitteeAction {
            previous_action_id: None,
            removed_committee_members: HashSet::from_iter(vec![make_committee_credential(true, 48), make_committee_credential(false, 12)].into_iter()),
            new_committee_members: HashMap::from_iter(vec![(make_committee_credential(false, 87), 1234)].into_iter()),
            terms: RationalNumber::from(1),
        });

        let proposal = ProposalProcedure {
            deposit: 9876,
            reward_account: vec![7,4,6,7],
            gov_action_id,
            gov_action,
            anchor: Anchor { url: "some.url".to_owned(), data_hash: vec![2,3,4,5] }
        };
        println!("Json: {}", serde_json::to_string(&proposal)?);

        Ok(())
    }
}
