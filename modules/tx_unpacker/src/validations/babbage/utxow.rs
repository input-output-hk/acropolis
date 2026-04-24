//! Babbage era UTxOW Rules
//! https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/babbage/impl/src/Cardano/Ledger/Babbage/Rules/Utxow.hs#L287
//!
//! NOTE: Babbage UTxOW re-uses Alonzo UTxOW rules, but introduces several new validation rules.

use acropolis_common::{
    protocol_params::ProtocolParams, validation::UTxOWValidationError, ReferenceScript,
};
use amaru_uplc::{arena::Arena, binder::DeBruijn, flat, machine::PlutusVersion, program::Program};
use rayon::prelude::*;

/// NEW Babbage Validation Rules
/// Since Babbage introduces **reference scripts** and **inline datums**, this requires new UTxOW validation rules.
///
/// 1. MalformedScriptWitnesses
pub fn validate(
    plutus_scripts_witnesses: &[ReferenceScript],
    protocol_params: &ProtocolParams,
) -> Result<(), Box<UTxOWValidationError>> {
    let protocol_version = protocol_params.protocol_version().ok_or_else(|| {
        Box::new(UTxOWValidationError::Other(
            "Protocol version is not set".to_string(),
        ))
    })?;
    let protocol_major_version = protocol_version.major;

    validate_plutus_scripts_witnesses(plutus_scripts_witnesses, protocol_major_version)?;

    Ok(())
}

/// Validate Plutus Script Witnesses are well formed
/// This is added from Babbage era.
/// Native scripts are not considered here. (Native script is valid one if that is decoded as NativeScript.)
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/babbage/impl/src/Cardano/Ledger/Babbage/Rules/Utxow.hs#L254
pub fn validate_plutus_scripts_witnesses(
    plutus_scripts_witnesses: &[ReferenceScript],
    protocol_major_version: u64,
) -> Result<(), Box<UTxOWValidationError>> {
    plutus_scripts_witnesses
        .par_iter()
        .try_for_each(|script| validate_script_wellformedness(script, protocol_major_version))?;

    Ok(())
}

fn validate_script_wellformedness(
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
        Box::new(UTxOWValidationError::MalformedScriptWitnesses {
            script_hash: reference_script.compute_hash(),
            protocol_major_version,
            reason: format!("Invalid script: {}", e),
        })
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use pallas::ledger::traverse::MultiEraTx;

    use crate::test_utils::{to_pallas_era, TestContext};
    use crate::validation_fixture;
    use test_case::test_case;

    use super::*;

    #[test]
    fn test_validate_script_wellformedness() {
        let script = ReferenceScript::PlutusV2(hex::decode("59014c01000032323232323232322223232325333009300e30070021323233533300b3370e9000180480109118011bae30100031225001232533300d3300e22533301300114a02a66601e66ebcc04800400c5288980118070009bac3010300c300c300c300c300c300c300c007149858dd48008b18060009baa300c300b3754601860166ea80184ccccc0288894ccc04000440084c8c94ccc038cd4ccc038c04cc030008488c008dd718098018912800919b8f0014891ce1317b152faac13426e6a83e06ff88a4d62cce3c1634ab0a5ec133090014a0266008444a00226600a446004602600a601a00626600a008601a006601e0026ea8c03cc038dd5180798071baa300f300b300e3754601e00244a0026eb0c03000c92616300a001375400660106ea8c024c020dd5000aab9d5744ae688c8c0088cc0080080048c0088cc00800800555cf2ba15573e6e1d200201").unwrap());
        let result = validate_script_wellformedness(&script, 7);
        assert!(result.is_ok());
    }

    #[test_case(validation_fixture!(
        "babbage",
        "2f0468a9b39a46eecd5576bc440895fc968a6aefe504341ad5a59b5f60d299de"
    ) =>
        matches Ok(());
        "babbage - valid transaction 1 with 4 plutus scripts witnesses"
    )]
    #[allow(clippy::result_large_err)]
    fn babbage_utxow_test(
        (ctx, raw_tx, era): (TestContext, Vec<u8>, &str),
    ) -> Result<(), UTxOWValidationError> {
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), &raw_tx).unwrap();
        let plutus_scripts_witnesses = acropolis_codec::extract_plutus_scripts_witnesses(&tx);

        validate(&plutus_scripts_witnesses, &ctx.protocol_params).map_err(|e| *e)
    }
}
