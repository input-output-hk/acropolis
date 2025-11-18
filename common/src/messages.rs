//! Definition of Acropolis messages

// We don't use these messages in the acropolis_common crate itself
#![allow(dead_code)]

use crate::commands::transactions::{TransactionsCommand, TransactionsCommandResponse};
use crate::genesis_values::GenesisValues;
use crate::ledger_state::SPOState;
use crate::protocol_params::{Nonce, ProtocolParams};
use crate::queries::parameters::{ParametersStateQuery, ParametersStateQueryResponse};
use crate::queries::spdd::{SPDDStateQuery, SPDDStateQueryResponse};
use crate::queries::utxos::{UTxOStateQuery, UTxOStateQueryResponse};
use crate::queries::{
    accounts::{AccountsStateQuery, AccountsStateQueryResponse},
    addresses::{AddressStateQuery, AddressStateQueryResponse},
    assets::{AssetsStateQuery, AssetsStateQueryResponse},
    blocks::{BlocksStateQuery, BlocksStateQueryResponse},
    epochs::{EpochsStateQuery, EpochsStateQueryResponse},
    governance::{GovernanceStateQuery, GovernanceStateQueryResponse},
    ledger::{LedgerStateQuery, LedgerStateQueryResponse},
    mempool::{MempoolStateQuery, MempoolStateQueryResponse},
    metadata::{MetadataStateQuery, MetadataStateQueryResponse},
    network::{NetworkStateQuery, NetworkStateQueryResponse},
    pools::{PoolsStateQuery, PoolsStateQueryResponse},
    scripts::{ScriptsStateQuery, ScriptsStateQueryResponse},
    transactions::{TransactionsStateQuery, TransactionsStateQueryResponse},
};

use crate::cbor::u128_cbor_codec;
use crate::types::*;
use crate::validation::ValidationStatus;

// Caryatid core messages which we re-export
pub use caryatid_module_clock::messages::ClockTickMessage;
pub use caryatid_module_rest_server::messages::{GetRESTResponse, RESTRequest, RESTResponse};

/// Raw block data message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RawBlockMessage {
    /// Header raw data
    pub header: Vec<u8>,

    /// Body raw data
    pub body: Vec<u8>,
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
    pub values: GenesisValues,
}

// Genesis tx hashes used to seed TxRegistry
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct GenesisUTxOsMessage {
    pub utxos: Vec<(TxOutRef, TxIdentifier)>,
}

/// Message encapsulating multiple UTXO deltas, in order
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct UTXODeltasMessage {
    /// Ordered set of deltas
    pub deltas: Vec<TxUTxODeltas>,
}

/// Message encapsulating multiple asset deltas
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AssetDeltasMessage {
    /// Mint and burn deltas per tx
    pub deltas: Vec<(TxIdentifier, NativeAssetsDelta)>,

    /// CIP 25 metadata blobs (Using 721 label)
    pub cip25_metadata_updates: Vec<Vec<u8>>,
}

/// Message encapsulating multiple transaction certificates, in order
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxCertificatesMessage {
    /// Ordered set of certificates
    pub certificates: Vec<TxCertificateWithPos>,
}

/// Address deltas message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddressDeltasMessage {
    /// Set of deltas
    pub deltas: Vec<AddressDelta>,
}

/// Withdrawals message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct WithdrawalsMessage {
    /// Set of withdrawals
    pub withdrawals: Vec<Withdrawal>,
}

/// Pot deltas message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PotDeltasMessage {
    /// Set of pot deltas
    pub deltas: Vec<PotDelta>,
}

/// Stake address part of address deltas message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeAddressDeltasMessage {
    /// Set of deltas
    pub deltas: Vec<StakeAddressDelta>,
}

/// Stake reward deltas message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct StakeRewardDeltasMessage {
    /// Stake Reward Deltas
    pub deltas: Vec<StakeRewardDelta>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BlockTxsMessage {
    /// Total transactions
    pub total_txs: u64,

    /// Total output
    pub total_output: u128,

    /// Total fees
    pub total_fees: u64,
}

/// Epoch activity - sent at end of epoch
#[derive(
    Debug,
    Clone,
    serde::Serialize,
    serde::Deserialize,
    minicbor::Encode,
    minicbor::Decode,
    PartialEq,
)]
pub struct EpochActivityMessage {
    /// Epoch which has ended
    #[n(0)]
    pub epoch: u64,

    /// Epoch start time
    /// UNIX timestamp
    #[n(1)]
    pub epoch_start_time: u64,

    /// Epoch end time
    /// UNIX timestamp
    #[n(2)]
    pub epoch_end_time: u64,

    /// When first block of this epoch was created
    #[n(3)]
    pub first_block_time: u64,

    /// Block height of first block of this epoch
    #[n(4)]
    pub first_block_height: u64,

    /// When last block of this epoch was created
    #[n(5)]
    pub last_block_time: u64,

    /// Block height of last block of this epoch
    #[n(6)]
    pub last_block_height: u64,

    /// Total blocks in this epoch
    #[n(7)]
    pub total_blocks: usize,

    /// Total txs in this epoch
    #[n(8)]
    pub total_txs: u64,

    /// Total outputs of all txs in this epoch
    #[cbor(n(9), with = "u128_cbor_codec")]
    pub total_outputs: u128,

    /// Total fees in this epoch
    #[n(10)]
    pub total_fees: u64,

    /// Map of SPO IDs to blocks produced
    #[n(11)]
    pub spo_blocks: Vec<(PoolId, usize)>,

    /// Nonce
    #[n(12)]
    pub nonce: Option<Nonce>,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct GovernanceProceduresMessage {
    /// Proposals
    pub proposal_procedures: Vec<ProposalProcedure>,

    /// Voting
    pub voting_procedures: Vec<(TxHash, VotingProcedures)>,

    /// Alonzo-compatible (from Shelley) and Babbage updates
    pub alonzo_babbage_updates: Vec<AlonzoBabbageUpdateProposal>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepStateMessage {
    /// Epoch which has ended
    pub epoch: u64,

    /// DRep initial deposit by id, for all active DReps.
    pub dreps: Vec<(DRepCredential, Lovelace)>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DRepDelegationDistribution {
    /// DRep stake assigned to the special "abstain" DRep.
    pub abstain: Lovelace,

    /// DRep stake assigned to the special "no confidence" DRep
    pub no_confidence: Lovelace,

    /// DRep stake distribution by ID
    pub dreps: Vec<(DRepCredential, Lovelace)>,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct DRepStakeDistributionMessage {
    /// Epoch which has ended
    pub epoch: u64,

    /// DRep delegation distribution
    pub drdd: DRepDelegationDistribution,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SPOStakeDistributionMessage {
    /// Epoch which has ended
    pub epoch: u64,

    /// SPO stake distribution by operator ID
    pub spos: Vec<(PoolId, DelegatedStake)>,
}

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct SPORewardsMessage {
    /// Epoch which has ended
    pub epoch: u64,

    /// SPO rewards by operator ID (total rewards before distribution, pool operator's rewards)
    pub spos: Vec<(PoolId, SPORewards)>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ProtocolParamsMessage {
    pub params: ProtocolParams,
}

/// Generated after all governance actions for the current epoch are processed.
/// Includes info about all actions that are accepted or expired at the epoch edge.
/// `VotingOutcome` informs about action_id, voting outcome and votes cast for the
/// action. If the action is not accepted or has no associated state change (like
/// Information), then it is included into `refunds` field. Otherwise info is
/// specified in `enact_state`/`withdrawals` field and not repeated in `refunds`.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct GovernanceOutcomesMessage {
    pub alonzo_babbage_outcomes: Vec<AlonzoBabbageVotingOutcome>,
    pub conway_outcomes: Vec<GovernanceOutcome>,
}

/// SPO state message
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SPOStateMessage {
    /// Epoch which has ended
    pub epoch: u64,

    /// All active and retiring SPOs
    pub spos: Vec<PoolRegistration>,

    /// SPOs in the above list which retired at the start of this epoch, by operator ID
    pub retired_spos: Vec<PoolId>,
}

/// Cardano message enum
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum CardanoMessage {
    BlockAvailable(RawBlockMessage),         // Block body available
    BlockValidation(ValidationStatus),       // Result of a block validation
    SnapshotComplete,                        // Mithril snapshot loaded
    ReceivedTxs(RawTxsMessage),              // Transaction available
    GenesisComplete(GenesisCompleteMessage), // Genesis UTXOs done + genesis params
    GenesisUTxOs(GenesisUTxOsMessage),       // Genesis UTxOs with their UTxOIdentifiers
    UTXODeltas(UTXODeltasMessage),           // UTXO deltas received
    AssetDeltas(AssetDeltasMessage),         // Asset mint and burn deltas
    TxCertificates(TxCertificatesMessage),   // Transaction certificates received
    AddressDeltas(AddressDeltasMessage),     // Address deltas received
    Withdrawals(WithdrawalsMessage),         // Withdrawals from reward accounts
    PotDeltas(PotDeltasMessage),             // Changes to pot balances
    BlockInfoMessage(BlockTxsMessage), // Transaction Info (total count, total output, total fees in a block)
    EpochActivity(EpochActivityMessage), // Total fees and VRF keys for an epoch
    DRepState(DRepStateMessage),       // Active DReps at epoch end
    SPOState(SPOStateMessage),         // Active SPOs at epoch end
    GovernanceProcedures(GovernanceProceduresMessage), // Governance procedures received

    // Protocol Parameters
    ProtocolParams(ProtocolParamsMessage), // Generated by Parameter State module
    GovernanceOutcomes(GovernanceOutcomesMessage), // Enacted updates from Governance

    // Stake distribution info
    DRepStakeDistribution(DRepStakeDistributionMessage), // Info about drep stake
    SPOStakeDistribution(SPOStakeDistributionMessage),   // SPO delegation distribution (SPDD)
    SPORewards(SPORewardsMessage),                       // SPO rewards distribution (SPRD)
    StakeAddressDeltas(StakeAddressDeltasMessage),       // Stake part of address deltas
    StakeRewardDeltas(StakeRewardDeltasMessage),         // Stake Reward Deltas
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SnapshotMessage {
    Startup, // subscribers should listen for incremental snapshot data
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
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub enum Message {
    #[default]
    None, // Just so we have a simple default

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

    // State query messages
    StateQuery(StateQuery),
    StateQueryResponse(StateQueryResponse),

    // Commands
    Command(Command),
    CommandResponse(CommandResponse),
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

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum StateQuery {
    Accounts(AccountsStateQuery),
    Addresses(AddressStateQuery),
    Assets(AssetsStateQuery),
    Blocks(BlocksStateQuery),
    Epochs(EpochsStateQuery),
    Governance(GovernanceStateQuery),
    Ledger(LedgerStateQuery),
    Mempool(MempoolStateQuery),
    Metadata(MetadataStateQuery),
    Network(NetworkStateQuery),
    Parameters(ParametersStateQuery),
    Pools(PoolsStateQuery),
    Scripts(ScriptsStateQuery),
    Transactions(TransactionsStateQuery),
    UTxOs(UTxOStateQuery),
    SPDD(SPDDStateQuery),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[allow(clippy::large_enum_variant)]
pub enum StateQueryResponse {
    Accounts(AccountsStateQueryResponse),
    Addresses(AddressStateQueryResponse),
    Assets(AssetsStateQueryResponse),
    Blocks(BlocksStateQueryResponse),
    Epochs(EpochsStateQueryResponse),
    Governance(GovernanceStateQueryResponse),
    Ledger(LedgerStateQueryResponse),
    Mempool(MempoolStateQueryResponse),
    Metadata(MetadataStateQueryResponse),
    Network(NetworkStateQueryResponse),
    Parameters(ParametersStateQueryResponse),
    Pools(PoolsStateQueryResponse),
    Scripts(ScriptsStateQueryResponse),
    Transactions(TransactionsStateQueryResponse),
    UTxOs(UTxOStateQueryResponse),
    SPDD(SPDDStateQueryResponse),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum Command {
    Transactions(TransactionsCommand),
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CommandResponse {
    Transactions(TransactionsCommandResponse),
}
