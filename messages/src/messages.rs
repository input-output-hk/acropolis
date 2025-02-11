//! Definition of Acropolis messages

// We don't use these messages in the acropolis_messages crate itself
#![allow(dead_code)]

// Caryatid core messages
use caryatid_sdk::messages::ClockTickMessage;

/// Block header message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockHeaderMessage {
    /// Slot number
    pub slot: u64,

    /// Header number
    pub number: u64,

    /// Raw Data
    pub raw: Vec<u8>,
}

/// Block body message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockBodyMessage {
    /// Slot number
    pub slot: u64,

    /// Raw Data
    pub raw: Vec<u8>,
}

/// Transactions message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawTxsMessage {
    /// Slot number
    pub slot: u64,

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

    /// Address data (raw)
    pub address: Vec<u8>,

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
    fn default() -> Self {
        UTXODelta::None(())
    }
}

/// Message encapsulating multiple UTXO deltas, in order
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct UTXODeltasMessage {
    /// Slot number
    pub slot: u64,

    /// Ordered set of deltas
    pub deltas: Vec<UTXODelta>
}

// === Global message enum ===
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Message {
    None(()),                               // Just so we have a simple default

    // Generic messages, get of jail free cards
    String(String),                         // Simple string
    JSON(serde_json::Value),                // JSON object

    // Caryatid standard messages
    Clock(ClockTickMessage),                // Clock tick

    // Cardano messages
    BlockHeader(BlockHeaderMessage),        // Block header available
    BlockBody(BlockBodyMessage),            // Block body available
    ReceivedTxs(RawTxsMessage),             // Transaction available
    UTXODeltas(UTXODeltasMessage),          // UTXO deltas received
}

impl Default for Message {
    fn default() -> Self {
        Message::None(())
    }
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

