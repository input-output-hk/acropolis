use acropolis_common::{Address, BlockHash, BlockNumber, Datum, Epoch, TxHash, UTxOIdentifier};
use anyhow::{anyhow, Error};

/// ---------------------------------------------------------------------------
/// Getter Return Types
/// ---------------------------------------------------------------------------
///
/// These structs represent the asset data returned by this module's
/// public getter methods.
/// ---------------------------------------------------------------------------
pub struct AssetCreate {
    pub holder_address: Address,
    pub quantity: u64,
    pub tx_hash: TxHash,
    pub utxo_index: u16,
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub tx_index_in_block: u32,
    pub block_timestamp: i64,
}

pub struct AssetSpend {
    pub holder_address: Address,
    pub quantity: u64,
    pub spending_tx_hash: TxHash,
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub tx_index_in_block: u32,
    pub block_timestamp: i64,
    pub utxo_tx_hash: TxHash,
    pub utxo_index: u16,
}

pub struct Registration {
    pub full_datum: Datum,
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub block_timestamp: i64,
    pub tx_index_in_block: u32,
    pub tx_hash: TxHash,
    pub utxo_index: u16,
}

pub struct Deregistration {
    pub full_datum: Datum,
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub block_timestamp: i64,
    pub tx_index_in_block: u32,
    pub tx_hash: TxHash,
    pub utxo_tx_hash: TxHash,
    pub utxo_index: u16,
}

#[derive(Debug)]
pub struct BridgeAssetUtxo {
    pub tx_hash: TxHash,
    pub output_index: u16,
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub tx_index_in_block: u32,
    pub block_timestamp: i64,
    pub tokens_out: u64,
    pub tokens_in: u64,
    pub datum: Option<Vec<u8>>,
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
pub struct BridgeUtxoMeta {
    pub creation: BridgeCreation,
}

impl BridgeUtxoMeta {
    pub fn ordering_key(&self) -> (BlockNumber, u32, u16) {
        (
            self.creation.block_number,
            self.creation.tx_index,
            self.creation.utxo.output_index,
        )
    }
}

impl TryFrom<&UTxOMeta> for AssetCreate {
    type Error = Error;

    fn try_from(meta: &UTxOMeta) -> Result<Self, Self::Error> {
        let creation = &meta.creation;

        Ok(AssetCreate {
            block_number: creation.block_number,
            block_hash: creation.block_hash,
            block_timestamp: creation.block_timestamp,
            tx_index_in_block: creation.tx_index,
            quantity: creation.quantity,
            holder_address: creation.address.clone(),
            tx_hash: creation.utxo.tx_hash,
            utxo_index: creation.utxo.output_index,
        })
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

impl From<&BridgeUtxoMeta> for BridgeAssetUtxo {
    fn from(meta: &BridgeUtxoMeta) -> Self {
        let creation = &meta.creation;

        BridgeAssetUtxo {
            tx_hash: creation.utxo.tx_hash,
            output_index: creation.utxo.output_index,
            block_number: creation.block_number,
            block_hash: creation.block_hash,
            tx_index_in_block: creation.tx_index,
            block_timestamp: creation.block_timestamp,
            tokens_out: creation.tokens_out,
            tokens_in: creation.tokens_in,
            datum: creation.datum.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CNightCreation {
    pub address: Address,
    pub quantity: u64,
    pub utxo: UTxOIdentifier,
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub tx_index: u32,
    pub block_timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct CNightSpend {
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub tx_hash: TxHash,
    pub tx_index: u32,
    pub block_timestamp: i64,
}

#[derive(Debug, Clone)]
pub struct BridgeCreation {
    pub utxo: UTxOIdentifier,
    pub block_number: BlockNumber,
    pub block_hash: BlockHash,
    pub tx_index: u32,
    pub block_timestamp: i64,
    pub tokens_out: u64,
    pub tokens_in: u64,
    pub datum: Option<Vec<u8>>,
}

#[derive(Clone)]
pub struct RegistrationEvent {
    pub block_number: u64,
    pub block_hash: BlockHash,
    pub block_timestamp: i64,
    pub epoch: Epoch,
    pub slot_number: u64,
    pub tx_index: u32,
    pub tx_hash: TxHash,
    pub utxo_index: u16,
    pub tx_inputs: Vec<UTxOIdentifier>,
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
    pub registration_utxo: UTxOIdentifier,
    pub spent_block_timestamp: i64,
    pub spent_block_hash: BlockHash,
    pub spent_tx_hash: TxHash,
    pub spent_tx_index: u32,
}

impl From<(BlockNumber, &RegistrationEvent, &DeregistrationEvent)> for Deregistration {
    fn from(
        (block_number, registration, event): (
            BlockNumber,
            &RegistrationEvent,
            &DeregistrationEvent,
        ),
    ) -> Self {
        Deregistration {
            full_datum: registration.datum.clone(),
            block_number,
            block_hash: event.spent_block_hash,
            block_timestamp: event.spent_block_timestamp,
            tx_index_in_block: event.spent_tx_index,
            tx_hash: event.spent_tx_hash,
            utxo_tx_hash: registration.tx_hash,
            utxo_index: registration.utxo_index,
        }
    }
}
