//! Definition of Acropolis messages

// We don't use these messages in the acropolis_common crate itself
#![allow(dead_code)]

use crate::types::*;

// Caryatid core messages which we re-export
pub use caryatid_module_clock::messages::ClockTickMessage;
pub use caryatid_module_rest_server::messages::{
    RESTRequest,
    RESTResponse,
    GetRESTResponse
};

/// Block header message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockHeaderMessage {
    /// Event sequence number (for serialisation)
    pub sequence: u64,

    /// Block info
    pub block: BlockInfo,

    /// Raw Data
    pub raw: Vec<u8>,
}

/// Block body message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockBodyMessage {
    /// Event sequence number (for serialisation)
    pub sequence: u64,
    
    /// Block info
    pub block: BlockInfo,

    /// Raw Data
    pub raw: Vec<u8>,
}

/// Snapshot completion message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotCompleteMessage {
    /// Next event sequence number to use
    pub next_sequence: u64,
    
    /// Last block in snapshot data
    pub last_block: BlockInfo,
}

/// Transactions message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawTxsMessage {
    /// Event sequence number (for serialisation)
    pub sequence: u64,
    
    /// Block info
    pub block: BlockInfo,

    /// Raw Data for each transaction
    pub txs: Vec<Vec<u8>>,
}

/// Genesis completion message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisCompleteMessage {
    /// Next event sequence number to use
    pub next_sequence: u64,
}

/// Message encapsulating multiple UTXO deltas, in order
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct UTXODeltasMessage {
    /// Event sequence number (for serialisation)
    pub sequence: u64,
    
    /// Block info
    pub block: BlockInfo,

    /// Ordered set of deltas
    pub deltas: Vec<UTXODelta>
}

/// Message encapsulating multiple transaction certificates, in order
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxCertificatesMessage {
    /// Event sequence number (for serialisation)
    pub sequence: u64,
    
    /// Block info
    pub block: BlockInfo,

    /// Ordered set of certificates
    pub certificates: Vec<TxCertificate>
}

/// Message encapsulating multiple governance events: voting procedures and proposal procedures
pub struct GovernanceMessage {
    /// Event sequence number (for serialisation)
    pub sequence: u64,

    /// Block info
    pub block: BlockInfo,

    /// Ordered sequence of voting procedures and proposal procedures
    pub voting_procedures: Vec<VotingProcedure>,
    pub proposal_procedures: Vec<ProposalProcedure>,
}

/// Address deltas message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressDeltasMessage {
    /// Event sequence number (for serialisation)
    pub sequence: u64,
    
    /// Block info
    pub block: BlockInfo,

    /// Set of deltas
    pub deltas: Vec<AddressDelta>
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct GovernanceProceduresMessage {
    pub sequence: u64,

    pub block: BlockInfo,

    pub proposal_procedures: Vec<ProposalProcedure>,

    pub voting_procedures: Vec<VotingProcedures>
}

impl GovernanceProceduresMessage {
    pub fn is_empty(&self) -> bool {
        self.proposal_procedures.is_empty() && self.voting_procedures.is_empty()
    }
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
    RESTRequest(RESTRequest),                  // REST request
    RESTResponse(RESTResponse),                // REST response

    // Cardano messages
    BlockHeader(BlockHeaderMessage),           // Block header available
    BlockBody(BlockBodyMessage),               // Block body available
    SnapshotComplete(SnapshotCompleteMessage), // Mithril snapshot loaded
    ReceivedTxs(RawTxsMessage),                // Transaction available
    GenesisComplete(GenesisCompleteMessage),   // Genesis UTXOs done
    UTXODeltas(UTXODeltasMessage),             // UTXO deltas received
    TxCertificates(TxCertificatesMessage),     // Transaction certificates received
    AddressDeltas(AddressDeltasMessage),       // Address deltas received
    GovernanceProcedures(GovernanceProceduresMessage) // Governance procedures received
}

impl Default for Message {
    fn default() -> Self { Self::None(()) }
}

// Casts from specific Caryatid messages
impl From<ClockTickMessage> for Message {
    fn from(msg: ClockTickMessage) -> Self {
        Message::Clock(msg)
    }
}

impl From<RESTRequest> for Message {
    fn from(msg: RESTRequest) -> Self {
        Message::RESTRequest(msg)
    }
}

impl From<RESTResponse> for Message {
    fn from(msg: RESTResponse) -> Self {
        Message::RESTResponse(msg)
    }
}

// Casts from platform-wide messages
impl GetRESTResponse for Message {
    fn get_rest_response(&self) -> Option<RESTResponse> {
        if let Message::RESTResponse(result) = self {
            Some(result.clone())
        } else {
            None
        }
    }
}


