use acropolis_common::{
    Address, BlockHash, BlockNumber, Datum, Epoch, Slot, TxHash, UTxOIdentifier,
};
use chrono::NaiveDateTime;

/// ---------------------------------------------------------------------------
/// Getter Return Types
/// ---------------------------------------------------------------------------
///
/// These structs represent the asset data returned by this module's
/// public getter methods.
/// ---------------------------------------------------------------------------
#[allow(dead_code)]
pub struct AssetCreate {
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub block_timestamp: NaiveDateTime,
    pub tx_index_in_block: u32,
    pub quantity: i64,
    pub holder_address: Address,
    pub tx_hash: TxHash,
    pub utxo_index: u16,
}

#[allow(dead_code)]
pub struct AssetSpend {
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub block_timestamp: NaiveDateTime,
    pub tx_index_in_block: u32,
    pub quantity: i64,
    pub holder_address: Address,
    pub utxo_tx_hash: TxHash,
    pub utxo_index: u16,
    pub spending_tx_hash: TxHash,
}

#[allow(dead_code)]
pub struct Registration {
    pub full_datum: Datum,
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub block_timestamp: NaiveDateTime,
    pub tx_index_in_block: u32,
    pub tx_hash: TxHash,
    pub utxo_index: u16,
}

#[allow(dead_code)]
pub struct Deregistration {
    pub full_datum: Datum,
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub block_timestamp: NaiveDateTime,
    pub tx_index_in_block: u32,
    pub tx_hash: TxHash,
    pub utxo_tx_hash: TxHash,
    pub utxo_index: u16,
}

/// ---------------------------------------------------------------------------
/// Internal State Types
/// ---------------------------------------------------------------------------
///
/// These structs are used internally by the indexing state and are not
/// exposed by public getter methods.
/// ---------------------------------------------------------------------------
#[derive(Debug, Default, Clone)]
pub struct UTxOMeta {
    pub holder_address: Address,
    pub asset_quantity: i64,

    // Creation info
    pub created_in: BlockNumber,
    pub created_block_hash: BlockHash,
    pub created_tx: TxHash,
    pub created_tx_index: u32,
    pub created_utxo_index: u16,
    pub created_block_timestamp: NaiveDateTime,

    // Spend info
    pub spent_in: Option<BlockNumber>,
    pub spent_block_hash: Option<BlockHash>,
    pub spend_tx: Option<TxHash>,
    pub spent_tx_index: Option<u32>,
    pub spent_block_timestamp: Option<NaiveDateTime>,
}

pub struct RegistrationEvent {
    pub header: EventHeader,
    pub datum: Datum,
}

pub struct DeregistrationEvent {
    pub header: EventHeader,
    pub spent_block_timestamp: NaiveDateTime,
    pub spent_block_hash: BlockHash,
    pub spent_tx_hash: TxHash,
    pub spent_tx_index: u32,
    pub datum: Datum,
}

pub struct EventHeader {
    pub block_hash: BlockHash,
    pub block_timestamp: NaiveDateTime,
    pub tx_index: u32,
    pub tx_hash: TxHash,
    pub utxo_index: u16,
}

#[derive(Clone)]
pub struct CandidateUTxO {
    pub _utxo: UTxOIdentifier,
    pub _epoch_number: Epoch,
    pub _block_number: BlockNumber,
    pub _slot_number: Slot,
    pub _tx_index_within_block: u32,
    pub _datum: Datum,
    pub _inputs: Vec<UTxOIdentifier>,
}
