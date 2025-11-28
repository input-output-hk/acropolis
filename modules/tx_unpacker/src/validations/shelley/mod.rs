use acropolis_common::{
    protocol_params::ShelleyParams, validation::TransactionValidationError, TxIdentifier, TxOutRef,
};
use anyhow::Result;
use pallas::ledger::traverse::MultiEraTx;

pub mod utxo;

pub fn validate_shelley_tx<F>(
    tx: &MultiEraTx,
    shelley_params: &ShelleyParams,
    current_slot: u64,
    lookup_by_hash: F,
) -> Result<(), TransactionValidationError>
where
    F: Fn(TxOutRef) -> Result<TxIdentifier>,
{
    utxo::validate_shelley_tx(tx, shelley_params, current_slot, lookup_by_hash).map_err(|e| *e)?;

    Ok(())
}
