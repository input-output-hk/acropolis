//! Core type definitions for Acropolis
// We don't use these types in the acropolis_common crate itself
#![allow(dead_code)]

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

/// Amount of Ada, in Lovelace
pub type Lovelace = u64;

/// Rational number = numerator / denominator
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Ratio {
    pub numerator: u64,
    pub denominator: u64,
}

/// Stake credential
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StakeCredential {
    /// Address key hash
    AddrKeyHash(KeyHash),

    /// Script hash
    ScriptHash(KeyHash),
}

impl Default for StakeCredential {
    fn default() -> Self { Self::AddrKeyHash(Vec::new()) }
}

/// Pool registration data
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct PoolRegistration {
    /// Operator pool key hash - used as ID
    pub operator: KeyHash,

    /// VRF key hash
    pub vrf_key_hash: KeyHash,

    /// Pledged Ada
    pub pledge: Lovelace,

    /// Fixed cost
    pub cost: Lovelace,

    /// Marginal cost (fraction)
    pub margin: Ratio,

    /// Reward account
    pub reward_account: Vec<u8>,

    /// Pool owners by their key hash
    pub pool_owners: Vec<KeyHash>,

    // TODO Relays
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
}
