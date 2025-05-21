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
    /// Raw Data
    pub raw: Vec<u8>,
}

/// Block body message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockBodyMessage {
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
    /// Raw Data for each transaction
    pub txs: Vec<Vec<u8>>,
}

/// Genesis completion message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisCompleteMessage {
    // Conway genesis block
    pub conway_genesis: Option<ConwayGenesisParams>,
}

/// Message encapsulating multiple UTXO deltas, in order
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct UTXODeltasMessage {
    /// Ordered set of deltas
    pub deltas: Vec<UTXODelta>
}

/// Message encapsulating multiple transaction certificates, in order
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxCertificatesMessage {
    /// Ordered set of certificates
    pub certificates: Vec<TxCertificate>
}

/// Address deltas message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressDeltasMessage {
    /// Set of deltas
    pub deltas: Vec<AddressDelta>
}

/// Stake address part of address deltas message
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeAddressDeltasMessage {
    /// Set of deltas
    pub deltas: Vec<StakeAddressDelta>
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct GovernanceProceduresMessage {
    /// Proposals
    pub proposal_procedures: Vec<ProposalProcedure>,

    /// Voting
    pub voting_procedures: Vec<(DataHash, VotingProcedures)>
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepStakeDistributionMessage {
    // DRep stake distribution by ID
    pub data: Vec<(DRepCredential, Lovelace)>
}

/// Cardano message enum
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CardanoMessage {
    BlockHeader(BlockHeaderMessage),           // Block header available
    BlockBody(BlockBodyMessage),               // Block body available
    SnapshotComplete,                          // Mithril snapshot loaded
    ReceivedTxs(RawTxsMessage),                // Transaction available
    GenesisComplete(GenesisCompleteMessage),   // Genesis UTXOs done + genesis params
    UTXODeltas(UTXODeltasMessage),             // UTXO deltas received
    TxCertificates(TxCertificatesMessage),     // Transaction certificates received
    AddressDeltas(AddressDeltasMessage),       // Address deltas received
    GovernanceProcedures(GovernanceProceduresMessage), // Governance procedures received

    // Stake distribution info
    DRepStakeDistribution(DRepStakeDistributionMessage), // Info about drep stake
    StakeAddressDeltas(StakeAddressDeltasMessage),       // Stake part of address deltas
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

    // Cardano messages with attached BlockInfo
    Cardano((BlockInfo, CardanoMessage)),
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


