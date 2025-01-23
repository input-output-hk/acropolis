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

