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
#[derive(Debug, Clone)]
pub struct UTxOMeta {
    pub creation: CNightCreation,
    pub spend: Option<CNightSpend>,
}

#[derive(Debug, Clone)]
pub struct CNightCreation {
    pub address: Address,
    pub quantity: i64,
    pub utxo: UTxOIdentifier,
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub tx_index: u32,
    pub block_timestamp: NaiveDateTime,
}

#[derive(Debug, Clone)]
pub struct CNightSpend {
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub tx_hash: TxHash,
    pub tx_index: u32,
    pub block_timestamp: NaiveDateTime,
}

#[derive(Clone)]
pub struct RegistrationEvent {
    pub block_hash: BlockHash,
    pub block_timestamp: NaiveDateTime,
    pub tx_index: u32,
    pub tx_hash: TxHash,
    pub utxo_index: u16,
    pub datum: Datum,
}

#[derive(Clone)]
pub struct DeregistrationEvent {
    pub registration: RegistrationEvent,
    pub spent_block_timestamp: NaiveDateTime,
    pub spent_block_hash: BlockHash,
    pub spent_tx_hash: TxHash,
    pub spent_tx_index: u32,
    pub datum: Datum,
}

#[derive(Clone)]
pub struct TxOutput {
    pub utxo: UTxOIdentifier,
    pub _epoch_number: Epoch,
    pub _block_number: BlockNumber,
    pub _slot_number: Slot,
    pub _tx_index_within_block: u32,
    pub _datum: Datum,
    pub _inputs: Vec<UTxOIdentifier>,
}
