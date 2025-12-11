use acropolis_common::{
    protocol_params::ShelleyParams,
    validation::{Phase1ValidationError, TransactionValidationError},
};
use anyhow::Result;
use pallas::ledger::traverse::{self, Era as PallasEra, MultiEraTx};
mod shelley;

pub fn validate_shelley_tx(
    raw_tx: &[u8],
    shelley_params: &ShelleyParams,
    current_slot: u64,
) -> Result<(), TransactionValidationError> {
    let tx = MultiEraTx::decode_for_era(traverse::Era::Shelley, raw_tx)
        .map_err(|e| TransactionValidationError::CborDecodeError(e.to_string()))?;
    let tx_size = tx.size();

    let mtx = match tx {
        MultiEraTx::AlonzoCompatible(mtx, PallasEra::Shelley) => mtx,
        _ => {
            return Err(TransactionValidationError::MalformedTransaction(
                "Not a Shelley transaction".to_string(),
            ));
        }
    };

    shelley::tx::validate_shelley_tx(&mtx, tx_size as u32, shelley_params, current_slot)
        .map_err(|e| *e)?;
    shelley::utxo::validate_shelley_tx(&mtx, shelley_params)
        .map_err(|e| Phase1ValidationError::UTxOValidationError(*e))?;

    Ok(())
}
