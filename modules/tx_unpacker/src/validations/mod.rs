use acropolis_common::{
    protocol_params::ShelleyParams,
    validation::{Phase1ValidationError, TransactionValidationError},
    Era, GenesisDelegates, TxHash,
};
use anyhow::Result;
use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
mod shelley;

pub fn validate_shelley_tx(
    raw_tx: &[u8],
    genesis_delegs: &GenesisDelegates,
    shelley_params: &ShelleyParams,
    current_slot: u64,
) -> Result<(), TransactionValidationError> {
    let tx = MultiEraTx::decode_for_era(PallasEra::Shelley, raw_tx).map_err(|e| {
        TransactionValidationError::CborDecodeError {
            era: Era::Shelley,
            reason: e.to_string(),
        }
    })?;
    let tx_hash = TxHash::from(*tx.hash());
    let tx_size = tx.size() as u32;

    // because we decode_for_era as shelley, which is alonzo compatible.
    let mtx = tx.as_alonzo().ok_or_else(|| TransactionValidationError::CborDecodeError {
        era: Era::Shelley,
        reason: "Not Alonzo-compatible".to_string(),
    })?;

    shelley::tx::validate(mtx, tx_size, shelley_params, current_slot).map_err(|e| *e)?;
    shelley::utxo::validate(mtx, shelley_params)
        .map_err(|e| Phase1ValidationError::UTxOValidationError(*e))?;
    shelley::utxow::validate(mtx, tx_hash, genesis_delegs, shelley_params.update_quorum)
        .map_err(|e| Phase1ValidationError::UTxOWValidationError(*e))?;

    Ok(())
}
