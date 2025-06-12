//! Definition of Acropolis messages

// We don't use these messages in the acropolis_common crate itself
#![allow(dead_code)]

use crate::ledger_state::SPOState;

use crate::types::*;

// Caryatid core messages which we re-export
pub use caryatid_module_clock::messages::ClockTickMessage;
pub use caryatid_module_rest_server::messages::{GetRESTResponse, RESTRequest, RESTResponse};

/// Block header message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockHeaderMessage {
    /// Raw Data
    pub raw: Vec<u8>,
}

/// Block body message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockBodyMessage {
    /// Raw Data
    pub raw: Vec<u8>,
}

/// Snapshot completion message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotCompleteMessage {
    /// Last block in snapshot data
    pub last_block: BlockInfo,
}

/// Transactions message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawTxsMessage {
    /// Raw Data for each transaction
    pub txs: Vec<Vec<u8>>,
}

/// Genesis completion message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisCompleteMessage {
}

/// Message encapsulating multiple UTXO deltas, in order
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UTXODeltasMessage {
    /// Ordered set of deltas
    pub deltas: Vec<UTXODelta>,
}

/// Message encapsulating multiple transaction certificates, in order
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxCertificatesMessage {
    /// Ordered set of certificates
    pub certificates: Vec<TxCertificate>,
}

/// Address deltas message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressDeltasMessage {
    /// Set of deltas
    pub deltas: Vec<AddressDelta>,
}

/// Stake address part of address deltas message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeAddressDeltasMessage {
    /// Set of deltas
    pub deltas: Vec<StakeAddressDelta>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockFeesMessage {
    /// Total fees
    pub total_fees: u64,
}

/// Epoch activity - sent at end of epoch
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpochActivityMessage {
    /// Epoch which has ended
    pub epoch: u64,

    /// Total blocks in this epoch
    pub total_blocks: usize,

    /// Total fees in this epoch
    pub total_fees: u64,

    /// List of all VRF vkey hashes used on blocks (SPO indicator) and
    /// number of blocks produced
    pub vrf_vkey_hashes: Vec<(KeyHash, usize)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GovernanceProceduresMessage {
    /// Proposals
    pub proposal_procedures: Vec<ProposalProcedure>,

    /// Voting
    pub voting_procedures: Vec<(DataHash, VotingProcedures)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepStateMessage {
    /// Epoch which has ended
    pub epoch: u64,

    /// DRep initial deposit by id, for all active DReps.
    pub dreps: Vec<(DRepCredential, Lovelace)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepStakeDistributionMessage {
    /// Epoch which has ended
    pub epoch: u64,

    /// DRep stake assigned to the special "abstain" DRep.
    pub abstain: Lovelace,

    /// DRep stake assigned to the special "no confidence" DRep
    pub no_confidence: Lovelace,

    /// DRep stake distribution by ID
    pub dreps: Vec<(DRepCredential, Lovelace)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolParamsMessage {
    pub params: ProtocolParams,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct EnactStateMessage {
    pub enactments: Vec<EnactStateElem>,
}

/// SPO state message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SPOStateMessage {
    /// Epoch which has ended
    pub epoch: u64,

    /// All active SPOs
    pub spos: Vec<PoolRegistration>,
}

/// Cardano message enum
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CardanoMessage {
    BlockHeader(BlockHeaderMessage),         // Block header available
    BlockBody(BlockBodyMessage),             // Block body available
    SnapshotComplete,                        // Mithril snapshot loaded
    ReceivedTxs(RawTxsMessage),              // Transaction available
    GenesisComplete(GenesisCompleteMessage), // Genesis UTXOs done + genesis params
    UTXODeltas(UTXODeltasMessage),           // UTXO deltas received
    TxCertificates(TxCertificatesMessage),   // Transaction certificates received
    AddressDeltas(AddressDeltasMessage),     // Address deltas received
    BlockFees(BlockFeesMessage),             // Total fees in a block
    EpochActivity(EpochActivityMessage),     // Total fees and VRF keys for an epoch
    DRepState(DRepStateMessage),             // Active DReps at epoch end
    SPOState(SPOStateMessage),               // Active SPOs at epoch end
    GovernanceProcedures(GovernanceProceduresMessage), // Governance procedures received

    // Protocol Parameters
    ProtocolParams(ProtocolParamsMessage), // Generated by Parameter State module
    EnactState(EnactStateMessage),         // Enacted updates from Governance

    // Stake distribution info
    DRepStakeDistribution(DRepStakeDistributionMessage), // Info about drep stake
    StakeAddressDeltas(StakeAddressDeltasMessage),       // Stake part of address deltas
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SnapshotMessage {
    Bootstrap(SnapshotStateMessage),
    DumpRequest(SnapshotDumpMessage),
    Dump(SnapshotStateMessage),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SnapshotDumpMessage {
    pub block_height: u64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SnapshotStateMessage {
    SPOState(SPOState),
}

// === Global message enum ===
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Message {
    None(()), // Just so we have a simple default

    // Generic messages, get of jail free cards
    String(String),          // Simple string
    JSON(serde_json::Value), // JSON object

    // Caryatid standard messages
    Clock(ClockTickMessage),    // Clock tick
    RESTRequest(RESTRequest),   // REST request
    RESTResponse(RESTResponse), // REST response

    // Cardano messages with attached BlockInfo
    Cardano((BlockInfo, CardanoMessage)),

    // Initialize state from a snapshot
    Snapshot(SnapshotMessage),
}

impl Default for Message {
    fn default() -> Self {
        Self::None(())
    }
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
