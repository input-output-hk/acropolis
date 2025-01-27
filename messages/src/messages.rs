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

/// Transaction message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxMessage {
    /// Slot number
    pub slot: u64,

    /// Index in block
    pub index: u32,

    /// Raw Data
    pub raw: Vec<u8>,
}

/// UTXO created message (tx output)
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct OutputMessage {
    /// Slot number
    pub slot: u64,

    /// Tx index in block
    pub tx_index: u32,

    /// Output index in tx
    pub index: u32,

    /// Address data (raw)
    pub address: Vec<u8>,

    /// Output value (Lovelace)
    pub value: u64,
}

/// UTXO spent message (tx input)
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct InputMessage {
    /// Slot number
    pub slot: u64,

    /// Tx index in block
    pub tx_index: u32,

    /// Output index in tx
    pub index: u32,

    /// Tx hash of referenced UTXO
    pub ref_hash: Vec<u8>,

    /// Index of UTXO in referenced tx
    pub ref_index: u64,
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
    Tx(TxMessage),                          // Transaction available
    Output(OutputMessage),                  // New output (UTXO) created
    Input(InputMessage),                    // Input used - UTXO spent
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

impl From<TxMessage> for Message {
    fn from(msg: TxMessage) -> Self {
        Message::Tx(msg)
    }
}

impl From<OutputMessage> for Message {
    fn from(msg: OutputMessage) -> Self {
        Message::Output(msg)
    }
}

impl From<InputMessage> for Message {
    fn from(msg: InputMessage) -> Self {
        Message::Input(msg)
    }
}
