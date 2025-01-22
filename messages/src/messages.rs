//! Definition of Acropolis messages

// We don't use these messages in the acropolis_messages crate itself
#![allow(dead_code)]

// Caryatid core messages
use caryatid_sdk::messages::ClockTickMessage;

/// New chain header message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct NewTipHeaderMessage {
    /// Slot number
    pub slot: u64,

    /// Header number
    pub number: u64,

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
    NewTipHeader(NewTipHeaderMessage),      // New tip of chain available
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

impl From<NewTipHeaderMessage> for Message {
    fn from(msg: NewTipHeaderMessage) -> Self {
        Message::NewTipHeader(msg)
    }
}

