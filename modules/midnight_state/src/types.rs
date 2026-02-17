use acropolis_common::{
    Address, BlockHash, BlockNumber, Datum, Epoch, Slot, TxHash, UTxOIdentifier,
};
use chrono::NaiveDateTime;

#[derive(Debug, Default, Clone)]
pub struct UTxOMeta {
    pub _holder_address: Address,
    pub _asset_quantity: i64,
    pub _created_in: BlockNumber,
    pub _created_tx: TxHash,
    pub _spent_in: Option<BlockNumber>,
    pub _spend_tx: Option<TxHash>,
}

pub struct RegistrationEvent {
    pub _header: EventHeader,
    pub _datum: Datum,
}

pub struct DeregistrationEvent {
    pub _header: EventHeader,
    pub _spent_utxo_hash: TxHash,
    pub _spent_utxo_index: u16,
    pub _datum: Datum,
}

pub struct EventHeader {
    pub _block_hash: BlockHash,
    pub _block_timestamp: NaiveDateTime,
    pub _tx_index_in_block: u32,
    pub _tx_hash: TxHash,
    pub _tx_index: u16,
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
