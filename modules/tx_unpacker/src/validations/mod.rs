use acropolis_common::{
    protocol_params::ShelleyParams,
    validation::{Phase1ValidationError, TransactionValidationError},
    Era, GenesisDelegates, TxHash,
};
use anyhow::Result;
use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
mod alonzo;
mod babbage;
mod conway;
mod shelley;

pub fn validate_tx(
    raw_tx: &[u8],
    genesis_delegs: &GenesisDelegates,
    shelley_params: &Option<ShelleyParams>,
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

    match era {
        Era::Shelley | Era::Allegra | Era::Mary | Era::Alonzo => {
            validate_alonzo_compatible_tx(&tx, genesis_delegs, shelley_params, current_slot, era)?;
        }
        Era::Babbage => {
            validate_babbage_tx(&tx, genesis_delegs, shelley_params, era)?;
        }
        Era::Conway => {
            validate_conway_tx(&tx, era)?;
        }
        _ => (),
    }

    Ok(())
}

fn validate_alonzo_compatible_tx(
    tx: &MultiEraTx,
    genesis_delegs: &GenesisDelegates,
    shelley_params: &Option<ShelleyParams>,
    current_slot: u64,
    era: Era,
) -> Result<(), Box<TransactionValidationError>> {
    let Some(shelley_params) = shelley_params else {
        return Err(Box::new(TransactionValidationError::Other(
            "Shelley params are not set".to_string(),
        )));
    };

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
    let metadata = acropolis_codec::map_metadata(&tx.metadata());

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
                &metadata,
                genesis_delegs,
                shelley_params.update_quorum,
                &shelley_params.protocol_params.protocol_version,
            )
            .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;
        }
        Era::Allegra => {
            // TODO:
            // Add Tx and UTxO validation

            shelley::utxow::validate(
                mtx,
                tx_hash,
                &vkey_witnesses,
                &native_scripts,
                &metadata,
                genesis_delegs,
                shelley_params.update_quorum,
                &shelley_params.protocol_params.protocol_version,
            )
            .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;
        }
        Era::Mary => {
            // TODO:
            // Add Tx and UTxO validation

            shelley::utxow::validate(
                mtx,
                tx_hash,
                &vkey_witnesses,
                &native_scripts,
                &metadata,
                genesis_delegs,
                shelley_params.update_quorum,
                &shelley_params.protocol_params.protocol_version,
            )
            .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;
        }
        Era::Alonzo => {
            alonzo::utxow::validate(
                mtx,
                tx_hash,
                &vkey_witnesses,
                &native_scripts,
                &metadata,
                genesis_delegs,
                shelley_params.update_quorum,
                &shelley_params.protocol_params.protocol_version,
            )
            .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;
        }
        _ => {}
    }

    Ok(())
}

fn validate_babbage_tx(
    tx: &MultiEraTx,
    genesis_delegs: &GenesisDelegates,
    shelley_params: &Option<ShelleyParams>,
    era: Era,
) -> Result<(), Box<TransactionValidationError>> {
    let Some(shelley_params) = shelley_params else {
        return Err(Box::new(TransactionValidationError::Other(
            "Shelley params are not set".to_string(),
        )));
    };

    let tx_hash = TxHash::from(*tx.hash());

    let mtx = tx.as_babbage().ok_or_else(|| TransactionValidationError::CborDecodeError {
        era,
        reason: "Not Babbage Tx".to_string(),
    })?;
    let (vkey_witnesses, errors) = acropolis_codec::map_vkey_witnesses(tx.vkey_witnesses());
    if !errors.is_empty() {
        return Err(Box::new(
            (Phase1ValidationError::MalformedTransaction { errors }).into(),
        ));
    }
    let native_scripts = acropolis_codec::map_native_scripts(tx.native_scripts());
    let metadata = acropolis_codec::map_metadata(&tx.metadata());

    if era == Era::Babbage {
        babbage::utxow::validate(
            mtx,
            tx_hash,
            &vkey_witnesses,
            &native_scripts,
            &metadata,
            genesis_delegs,
            shelley_params.update_quorum,
            &shelley_params.protocol_params.protocol_version,
        )
        .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;
    }

    Ok(())
}

fn validate_conway_tx(tx: &MultiEraTx, era: Era) -> Result<(), Box<TransactionValidationError>> {
    let tx_hash = TxHash::from(*tx.hash());

    let mtx = tx.as_conway().ok_or_else(|| TransactionValidationError::CborDecodeError {
        era,
        reason: "Not Conway Tx".to_string(),
    })?;
    let (vkey_witnesses, errors) = acropolis_codec::map_vkey_witnesses(tx.vkey_witnesses());
    if !errors.is_empty() {
        return Err(Box::new(
            (Phase1ValidationError::MalformedTransaction { errors }).into(),
        ));
    }
    let native_scripts = acropolis_codec::map_native_scripts(tx.native_scripts());

    conway::utxow::validate(mtx, tx_hash, &vkey_witnesses, &native_scripts)
        .map_err(|e| Box::new((Phase1ValidationError::UTxOWValidationError(*e)).into()))?;

    Ok(())
}
