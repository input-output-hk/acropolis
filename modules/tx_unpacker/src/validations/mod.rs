use acropolis_common::{
    protocol_params::ShelleyParams,
    validation::{Phase1ValidationError, TransactionValidationError},
    Era, GenesisDelegates, TxHash,
};
use anyhow::Result;
use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
mod allegra;
mod shelley;

pub fn validate_alonzo_compatible_tx(
    raw_tx: &[u8],
    genesis_delegs: &GenesisDelegates,
    shelley_params: &ShelleyParams,
    current_slot: u64,
    era: Era,
) -> Result<(), Box<TransactionValidationError>> {
    let pallas_era = match era {
        Era::Shelley => PallasEra::Shelley,
        Era::Allegra => PallasEra::Allegra,
        Era::Mary => PallasEra::Mary,
        Era::Alonzo => PallasEra::Alonzo,
        Era::Babbage => PallasEra::Babbage,
        Era::Conway => PallasEra::Conway,
        Era::Byron => PallasEra::Byron,
    };

    let tx = MultiEraTx::decode_for_era(pallas_era, raw_tx).map_err(|e| {
        TransactionValidationError::CborDecodeError {
            era,
            reason: e.to_string(),
        }
    })?;
    let tx_hash = TxHash::from(*tx.hash());
    let tx_size = tx.size() as u32;

    // because we decode_for_era as shelley, which is alonzo compatible.
    let mtx = tx.as_alonzo().ok_or_else(|| TransactionValidationError::CborDecodeError {
        era,
        reason: "Not Alonzo-compatible Tx".to_string(),
    })?;

    let (vkey_witnesses, errors) = acropolis_codec::map_vkey_witnesses(tx.vkey_witnesses());
    if !errors.is_empty() {
        return Err(Box::new(
            (Phase1ValidationError::MalformedTransaction { errors }).into(),
        ));
    }
    let native_scripts = acropolis_codec::map_native_scripts(tx.native_scripts());

    match era {
        Era::Shelley => {
            shelley::tx::validate(
                tx_size,
                tx.fee().unwrap_or(0),
                tx.ttl(),
                shelley_params,
                current_slot,
            )
            .map_err(|e| Box::new((*e).into()))?;
            shelley::utxo::validate(mtx, shelley_params)
                .map_err(|e| Box::new((Phase1ValidationError::UTxOValidationError(*e)).into()))?;
            shelley::utxow::validate(
                mtx,
                tx_hash,
                &vkey_witnesses,
                &native_scripts,
                genesis_delegs,
                shelley_params.update_quorum,
            )
            .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;
        }
        Era::Allegra => {
            // NOTE:
            // Need to add Tx and UTxO validation

            allegra::utxow::validate(
                mtx,
                tx_hash,
                &vkey_witnesses,
                &native_scripts,
                genesis_delegs,
                shelley_params.update_quorum,
            )
            .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;
        }
        _ => {}
    }

    Ok(())
}
