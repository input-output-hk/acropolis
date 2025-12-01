use acropolis_common::{
    protocol_params::ShelleyParams, validation::TransactionValidationError, TxIdentifier, TxOutRef,
};
use anyhow::Result;
use pallas::ledger::traverse::{self, MultiEraTx};

pub mod utxo;

pub fn validate_shelley_tx<F>(
    raw_tx: &[u8],
    shelley_params: &ShelleyParams,
    current_slot: u64,
    lookup_by_hash: F,
) -> Result<(), TransactionValidationError>
where
    F: Fn(TxOutRef) -> Result<TxIdentifier>,
{
    let tx = MultiEraTx::decode_for_era(traverse::Era::Shelley, raw_tx)
        .map_err(|e| TransactionValidationError::CborDecodeError(e.to_string()))?;
    utxo::validate_shelley_tx(&tx, shelley_params, current_slot, lookup_by_hash).map_err(|e| *e)?;

    Ok(())
}
