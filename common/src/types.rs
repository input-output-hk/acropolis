//! Core type definitions for Acropolis
// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]

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

/// A Shelley-era address - payment part
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ShelleyAddressPaymentPart {
    /// Payment to a key
    PaymentKeyHash(Vec<u8>),

    /// Payment to a script
    ScriptHash(Vec<u8>),
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
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
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
