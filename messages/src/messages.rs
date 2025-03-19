//! Definition of Acropolis messages

// We don't use these messages in the acropolis_messages crate itself
#![allow(dead_code)]

// Caryatid core messages
use caryatid_sdk::messages::ClockTickMessage;

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

/// Block header message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockHeaderMessage {
    /// Block info
    pub block: BlockInfo,

    /// Raw Data
    pub raw: Vec<u8>,
}

/// Block body message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockBodyMessage {
    /// Block info
    pub block: BlockInfo,

    /// Raw Data
    pub raw: Vec<u8>,
}

/// Snapshot completion message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotCompleteMessage {
    /// Last block in snapshot data
    pub last_block: BlockInfo,
}

/// Transactions message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawTxsMessage {
    /// Block info
    pub block: BlockInfo,

    /// Raw Data for each transaction
    pub txs: Vec<Vec<u8>>,
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

/// Message encapsulating multiple UTXO deltas, in order
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct UTXODeltasMessage {
    /// Block info
    pub block: BlockInfo,

    /// Ordered set of deltas
    pub deltas: Vec<UTXODelta>
}

/// Individual address balance change
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressDelta {
    /// Address
    pub address: Address,

    /// Balance change
    pub delta: i64,
}

/// Address deltas message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressDeltasMessage {
    /// Block info
    pub block: BlockInfo,

    /// Set of deltas
    pub deltas: Vec<AddressDelta>
}


// === Global message enum ===
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Message {
    None(()),                                  // Just so we have a simple default

    // Generic messages, get of jail free cards
    String(String),                            // Simple string
    JSON(serde_json::Value),                   // JSON object

    // Caryatid standard messages
    Clock(ClockTickMessage),                   // Clock tick

    // Cardano messages
    BlockHeader(BlockHeaderMessage),           // Block header available
    BlockBody(BlockBodyMessage),               // Block body available
    SnapshotComplete(SnapshotCompleteMessage), // Mithril snapshot loaded
    ReceivedTxs(RawTxsMessage),                // Transaction available
    UTXODeltas(UTXODeltasMessage),             // UTXO deltas received
    AddressDeltas(AddressDeltasMessage),       // Address deltas received
}

impl Default for Message {
    fn default() -> Self { Self::None(()) }
}

// Casts from specific messages
impl From<ClockTickMessage> for Message {
    fn from(msg: ClockTickMessage) -> Self {
        Message::Clock(msg)
    }
}

impl From<BlockHeaderMessage> for Message {
    fn from(msg: BlockHeaderMessage) -> Self {
        Message::BlockHeader(msg)
    }
}

impl From<BlockBodyMessage> for Message {
    fn from(msg: BlockBodyMessage) -> Self {
        Message::BlockBody(msg)
    }
}

impl From<SnapshotCompleteMessage> for Message {
    fn from(msg: SnapshotCompleteMessage) -> Self {
        Message::SnapshotComplete(msg)
    }
}

impl From<RawTxsMessage> for Message {
    fn from(msg: RawTxsMessage) -> Self {
        Message::ReceivedTxs(msg)
    }
}

impl From<UTXODeltasMessage> for Message {
    fn from(msg: UTXODeltasMessage) -> Self {
        Message::UTXODeltas(msg)
    }
}

impl From<AddressDeltasMessage> for Message {
    fn from(msg: AddressDeltasMessage) -> Self {
        Message::AddressDeltas(msg)
    }
}

