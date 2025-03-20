//! Definition of Acropolis messages

// We don't use these messages in the acropolis_common crate itself
#![allow(dead_code)]

use crate::types::*;

// Caryatid core messages
use caryatid_sdk::messages::ClockTickMessage;

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


/// Message encapsulating multiple UTXO deltas, in order
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct UTXODeltasMessage {
    /// Block info
    pub block: BlockInfo,

    /// Ordered set of deltas
    pub deltas: Vec<UTXODelta>
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

