//! Definition of Acropolis messages

// We don't use these messages in the acropolis_messages crate itself
#![allow(dead_code)]

// Caryatid core messages
use caryatid_sdk::messages::ClockTickMessage;

/// Incoming mini-protocol message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct MiniprotocolIncomingMessage {
    /// Data
    pub data: Vec<u8>,
}

// === Global message enum ===
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Message {
    None(()),                 // Just so we have a simple default
    String(String),           // Simple string
    Clock(ClockTickMessage),  // Clock tick
    MiniprotocolIncoming(MiniprotocolIncomingMessage),  // MP incoming
    JSON(serde_json::Value),  // Get out of jail free card
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

impl From<MiniprotocolIncomingMessage> for Message {
    fn from(msg: MiniprotocolIncomingMessage) -> Self {
        Message::MiniprotocolIncoming(msg)
    }
}

