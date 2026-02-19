use acropolis_common::{Address, BlockHash, BlockNumber, Datum, TxHash, UTxOIdentifier};
use anyhow::{anyhow, Error};
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

impl From<&UTxOMeta> for AssetCreate {
    fn from(meta: &UTxOMeta) -> Self {
        let creation = &meta.creation;

        AssetCreate {
            block_number: creation.block_number,
            block_hash: creation.block_hash,
            block_timestamp: creation.block_timestamp,
            tx_index_in_block: creation.tx_index,
            quantity: creation.quantity,
            holder_address: creation.address.clone(),
            tx_hash: creation.utxo.tx_hash,
            utxo_index: creation.utxo.output_index,
        }
    }
}

impl TryFrom<&UTxOMeta> for AssetSpend {
    type Error = Error;

    fn try_from(meta: &UTxOMeta) -> Result<Self, Self::Error> {
        let spend = meta.spend.as_ref().ok_or_else(|| anyhow!("UTxO has no spend record"))?;

        Ok(AssetSpend {
            block_number: spend.block_number,
            block_hash: spend.block_hash,
            block_timestamp: spend.block_timestamp,
            tx_index_in_block: spend.tx_index,
            quantity: meta.creation.quantity,
            holder_address: meta.creation.address.clone(),
            utxo_tx_hash: meta.creation.utxo.tx_hash,
            utxo_index: meta.creation.utxo.output_index,
            spending_tx_hash: spend.tx_hash,
        })
    }
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

impl From<(BlockNumber, &RegistrationEvent)> for Registration {
    fn from((block_number, event): (BlockNumber, &RegistrationEvent)) -> Self {
        Registration {
            full_datum: event.datum.clone(),
            block_number,
            block_hash: event.block_hash,
            block_timestamp: event.block_timestamp,
            tx_index_in_block: event.tx_index,
            tx_hash: event.tx_hash,
            utxo_index: event.utxo_index,
        }
    }
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

impl From<(BlockNumber, &DeregistrationEvent)> for Deregistration {
    fn from((block_number, event): (BlockNumber, &DeregistrationEvent)) -> Self {
        Deregistration {
            full_datum: event.datum.clone(),
            block_number,
            block_hash: event.spent_block_hash,
            block_timestamp: event.spent_block_timestamp,
            tx_index_in_block: event.spent_tx_index,
            tx_hash: event.spent_tx_hash,
            utxo_tx_hash: event.registration.tx_hash,
            utxo_index: event.registration.utxo_index,
        }
    }
}
