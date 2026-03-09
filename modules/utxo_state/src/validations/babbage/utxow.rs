use std::collections::HashMap;

use acropolis_common::{validation::UTxOWValidationError, ReferenceScript, ScriptHash};
use rayon::prelude::*;
use uplc_turbo::{arena::Arena, binder::DeBruijn, flat, program::Program};

/// NEW Babbage Validation Rules
/// Since Babbage introduces **reference scripts** and **inline datums**, this requires new UTxOW validation rules.
///
/// 1. MalformedReferenceScripts
pub fn validate(
    reference_scripts: &HashMap<ScriptHash, ReferenceScript>,
) -> Result<(), Box<UTxOWValidationError>> {
    validate_reference_scripts(reference_scripts)?;

    Ok(())
}

pub fn validate_reference_scripts(
    reference_scripts: &HashMap<ScriptHash, ReferenceScript>,
) -> Result<(), Box<UTxOWValidationError>> {
    reference_scripts.par_iter().try_for_each(|(script_hash, reference_script)| {
        validate_script_wellformedness(script_hash, reference_script)
    })?;

    Ok(())
}

fn validate_script_wellformedness(
    script_hash: &ScriptHash,
    reference_script: &ReferenceScript,
) -> Result<(), Box<UTxOWValidationError>> {
    let script_bytes = match reference_script {
        ReferenceScript::PlutusV1(bytes) => bytes,
        ReferenceScript::PlutusV2(bytes) => bytes,
        ReferenceScript::PlutusV3(bytes) => bytes,
        _ => return Ok(()),
    };

    let arena = Arena::new();
    let _: &Program<DeBruijn> = flat::decode(&arena, script_bytes).map_err(|e| {
        Box::new(UTxOWValidationError::MalformedReferenceScripts {
            script_hash: *script_hash,
            reason: format!("Invalid script: {}", e),
        })
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::TestContext;
    use crate::test_utils::{to_era, to_pallas_era};
    use crate::{utils, validation_fixture};
    use acropolis_common::{NetworkId, TxIdentifier};
    use pallas::ledger::traverse::MultiEraTx;
    use test_case::test_case;

    #[test_case(validation_fixture!(
        "conway",
        "5ed7de96fba4fd5dbf5eecd3a6abee9b8bc3cacce55672257fe3a2a97006bda3"
    ) =>
        matches Ok(());
        "conway - valid transaction 1 - 3 reference scripts"
    )]
    #[test_case(validation_fixture!(
        "conway",
        "502f7a04cf763d681931adc0f20ad3e1f8f5515e78f36d6fcb97f9a374ae76d2"
    ) =>
        matches Ok(());
        "conway - valid transaction 2 - one reference script but that is native script"
    )]
    #[allow(clippy::result_large_err)]
    fn babbage_utxow_test(
        (ctx, raw_tx, era): (TestContext, Vec<u8>, &str),
    ) -> Result<(), UTxOWValidationError> {
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), &raw_tx).unwrap();
        let raw_tx = tx.encode();
        let tx_identifier = TxIdentifier::new(4533644, 1);
        let mapped_tx = acropolis_codec::map_transaction(
            &tx,
            &raw_tx,
            tx_identifier,
            NetworkId::Mainnet,
            to_era(era),
        );
        let tx_error = mapped_tx.error.as_ref();
        assert!(tx_error.is_none());

        let tx_deltas = mapped_tx.convert_to_utxo_deltas(true);
        let tx_ref_scripts =
            utils::get_reference_scripts(&tx_deltas, &ctx.utxos, &ctx.reference_scripts);

        validate(&tx_ref_scripts).map_err(|e| *e)
    }
}
