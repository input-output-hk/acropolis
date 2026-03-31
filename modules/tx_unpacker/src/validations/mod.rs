use acropolis_common::{
    protocol_params::ProtocolParams,
    validation::{Phase1ValidationError, TransactionValidationError},
    Era, GenesisDelegates,
};
use anyhow::Result;
use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
mod allegra;
mod alonzo;
mod babbage;
mod shelley;
mod utils;

pub fn validate_tx(
    raw_tx: &[u8],
    protocol_params: &ProtocolParams,
    genesis_delegs: &GenesisDelegates,
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

    if era >= Era::Shelley {
        shelley::tx::validate(&tx, protocol_params, current_slot, era)
            .map_err(|e| Box::new((*e).into()))?;

        shelley::utxo::validate(&tx, protocol_params, era)
            .map_err(|e| Box::new(Phase1ValidationError::from(*e).into()))?;

        let (vkey_witnesses, errors) = acropolis_codec::map_vkey_witnesses(tx.vkey_witnesses());
        if !errors.is_empty() {
            return Err(Box::new(
                (Phase1ValidationError::MalformedTransaction { errors }).into(),
            ));
        }
        let native_scripts = acropolis_codec::map_native_scripts(tx.native_scripts());
        let metadata = acropolis_codec::map_metadata(&tx.metadata());

        shelley::utxow::validate(
            &tx,
            &vkey_witnesses,
            &native_scripts,
            &metadata,
            protocol_params,
            genesis_delegs,
        )
        .map_err(|e| Box::new(Phase1ValidationError::from(*e).into()))?;
    }

    if era >= Era::Allegra {
        let validity_interval = acropolis_codec::map_validity_interval(&tx);
        allegra::utxo::validate(&tx, &validity_interval, protocol_params, current_slot, era)
            .map_err(|e| Box::new(Phase1ValidationError::from(*e).into()))?;
    }

    if era >= Era::Alonzo {
        alonzo::utxow::validate(&tx)
            .map_err(|e| Box::new(Phase1ValidationError::from(*e).into()))?;
    }

    if era >= Era::Babbage {
        let plutus_scripts_witnesses = acropolis_codec::extract_plutus_scripts_witnesses(&tx);
        babbage::utxow::validate(&plutus_scripts_witnesses)
            .map_err(|e| Box::new(Phase1ValidationError::from(*e).into()))?;
    }

    Ok(())
}
