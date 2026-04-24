use std::collections::HashMap;

use acropolis_common::{
    protocol_params::ProtocolParams, validation::UTxOWValidationError, ReferenceScript, ScriptHash,
};
use amaru_uplc::{arena::Arena, binder::DeBruijn, flat, machine::PlutusVersion, program::Program};
use rayon::prelude::*;

/// NEW Babbage Validation Rules
/// Since Babbage introduces **reference scripts** and **inline datums**, this requires new UTxOW validation rules.
///
/// 1. MalformedReferenceScripts
pub fn validate(
    created_reference_scripts: HashMap<ScriptHash, &ReferenceScript>,
    protocol_params: &ProtocolParams,
) -> Result<(), Box<UTxOWValidationError>> {
    let protocol_version = protocol_params.protocol_version().ok_or_else(|| {
        Box::new(UTxOWValidationError::Other(
            "Protocol version is not set".to_string(),
        ))
    })?;
    let protocol_major_version = protocol_version.major;
    validate_reference_scripts(created_reference_scripts, protocol_major_version)?;

    Ok(())
}

/// Validate that the reference scripts created by the transaction are well-formed.
/// Deduplication by script hash is expected to happen at the call site
/// (via HashMap), so each script is only validated once.
pub fn validate_reference_scripts(
    reference_scripts: HashMap<ScriptHash, &ReferenceScript>,
    protocol_major_version: u64,
) -> Result<(), Box<UTxOWValidationError>> {
    reference_scripts.par_iter().try_for_each(|(script_hash, reference_script)| {
        validate_script_wellformedness(script_hash, reference_script, protocol_major_version)
    })?;

    Ok(())
}

fn validate_script_wellformedness(
    script_hash: &ScriptHash,
    reference_script: &ReferenceScript,
    protocol_major_version: u64,
) -> Result<(), Box<UTxOWValidationError>> {
    let (plutus_version, script_bytes) = match reference_script {
        ReferenceScript::PlutusV1(bytes) => (PlutusVersion::V1, bytes),
        ReferenceScript::PlutusV2(bytes) => (PlutusVersion::V2, bytes),
        ReferenceScript::PlutusV3(bytes) => (PlutusVersion::V3, bytes),
        _ => return Ok(()),
    };

    let arena = Arena::new();
    let _: &Program<DeBruijn> = flat::decode(
        &arena,
        script_bytes,
        plutus_version,
        protocol_major_version as u32,
    )
    .map_err(|e| {
        Box::new(UTxOWValidationError::MalformedReferenceScripts {
            script_hash: *script_hash,
            protocol_major_version,
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
    use crate::validation_fixture;
    use acropolis_common::{NetworkId, TxIdentifier};
    use pallas::ledger::traverse::MultiEraTx;
    use test_case::test_case;

    #[test_case(validation_fixture!(
        "conway",
        "2536194d2a976370a932174c10975493ab58fd7c16395d50e62b7c0e1949baea"
    ) =>
        matches Ok(());
        "conway - valid transaction 1 - created 1 reference script"
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
        let created_reference_scripts = tx_deltas
            .created_reference_scripts
            .as_deref()
            .unwrap_or(&[])
            .iter()
            .map(|(k, v)| (*k, v))
            .collect::<HashMap<ScriptHash, &ReferenceScript>>();

        validate(created_reference_scripts, &ctx.protocol_params).map_err(|e| *e)
    }
}
