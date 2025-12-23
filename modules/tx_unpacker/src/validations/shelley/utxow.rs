//! Shelley era UTxOW Rules
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L278

use std::collections::HashSet;

use crate::crypto::verify_ed25519_signature;
use acropolis_common::{
    validation::UTxOWValidationError, GenesisDelegates, KeyHash, NativeScript, TxHash, VKeyWitness,
};
use anyhow::Result;
use pallas::ledger::primitives::alonzo;

fn has_mir_certificate(mtx: &alonzo::MintedTx) -> bool {
    mtx.transaction_body
        .certificates
        .as_ref()
        .map(|certs| {
            certs
                .iter()
                .any(|cert| matches!(cert, alonzo::Certificate::MoveInstantaneousRewardsCert(_)))
        })
        .unwrap_or(false)
}

/// Validate Native Scripts from Transaction witnesses
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L373
pub fn validate_failed_native_scripts(
    native_scripts: &[NativeScript],
    vkey_hashes_provided: &HashSet<KeyHash>,
    low_bnd: Option<u64>,
    upp_bnd: Option<u64>,
) -> Result<(), Box<UTxOWValidationError>> {
    for native_script in native_scripts {
        if !native_script.eval(vkey_hashes_provided, low_bnd, upp_bnd) {
            return Err(Box::new(
                UTxOWValidationError::ScriptWitnessNotValidatingUTXOW {
                    script_hash: native_script.compute_hash(),
                },
            ));
        }
    }

    Ok(())
}

/// Validate that all vkey witnesses signatures
/// are verified
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L401
pub fn validate_verified_wits(
    vkey_witnesses: &[VKeyWitness],
    tx_hash: TxHash,
) -> Result<(), Box<UTxOWValidationError>> {
    for vkey_witness in vkey_witnesses.iter() {
        if !verify_ed25519_signature(vkey_witness, tx_hash.as_ref()) {
            return Err(Box::new(UTxOWValidationError::InvalidWitnessesUTxOW {
                key_hash: vkey_witness.key_hash(),
                witness: vkey_witness.clone(),
            }));
        }
    }
    Ok(())
}

/// Validate genesis keys signatures for MIR certificate
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L463
pub fn validate_mir_insufficient_genesis_sigs(
    vkey_hashes_provided: &HashSet<KeyHash>,
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
) -> Result<(), Box<UTxOWValidationError>> {
    let genesis_delegate_hashes =
        genesis_delegs.as_ref().values().map(|delegate| delegate.delegate).collect::<HashSet<_>>();

    // genSig := genDelegates ∩ witsKeyHashes
    let genesis_sigs =
        genesis_delegate_hashes.intersection(vkey_hashes_provided).copied().collect::<HashSet<_>>();

    // Check: |genSig| ≥ Quorum
    // If insufficient, report the signatures that were found (not the missing ones)
    if genesis_sigs.len() < update_quorum as usize {
        return Err(Box::new(
            UTxOWValidationError::MIRInsufficientGenesisSigsUTXOW {
                genesis_keys: genesis_sigs,
                quorum: update_quorum,
            },
        ));
    }

    Ok(())
}

pub fn validate(
    mtx: &alonzo::MintedTx,
    tx_hash: TxHash,
    vkey_witnesses: &[VKeyWitness],
    native_scripts: &[NativeScript],
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
) -> Result<(), Box<UTxOWValidationError>> {
    let transaction_body = &mtx.transaction_body;

    // Extract vkey hashes from vkey_witnesses
    let vkey_hashes_provided = vkey_witnesses.iter().map(|w| w.key_hash()).collect::<HashSet<_>>();

    // validate native scripts
    validate_failed_native_scripts(
        native_scripts,
        &vkey_hashes_provided,
        transaction_body.validity_interval_start,
        transaction_body.ttl,
    )?;

    // validate vkey witnesses signatures
    validate_verified_wits(vkey_witnesses, tx_hash)?;

    // NOTE:
    // need to validate metadata

    // validate mir certificate genesis sig
    if has_mir_certificate(mtx) {
        validate_mir_insufficient_genesis_sigs(
            &vkey_hashes_provided,
            genesis_delegs,
            update_quorum,
        )?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{test_utils::TestContext, validation_fixture};
    use pallas::ledger::traverse::{Era as PallasEra, MultiEraTx};
    use test_case::test_case;

    #[test_case(validation_fixture!(
        "shelley",
        "20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e"
    ) =>
        matches Ok(());
        "valid transaction 1 - with byron input & output"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "da350a9e2a14717172cee9e37df02b14b5718ea1934ce6bea25d739d9226f01b"
    ) =>
        matches Ok(());
        "valid transaction 2"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "0c993cb361c213e5b04d241321975e22870a0d658c03ea5b817c24fc48252ea0"
    ) =>
        matches Ok(());
        "valid transaction 2 - with mir certificates"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "da350a9e2a14717172cee9e37df02b14b5718ea1934ce6bea25d739d9226f01b", 
        "invalid_witnesses_utxow"
    ) =>
        matches Err(UTxOWValidationError::InvalidWitnessesUTxOW { key_hash, .. })
        if key_hash == KeyHash::from_str("b0baefb8dedefd7ec935514696ea5a66e9520f31dc8867737f0f0084").unwrap();
        "invalid_witnesses_utxow"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "0c993cb361c213e5b04d241321975e22870a0d658c03ea5b817c24fc48252ea0",
        "mir_insufficient_genesis_sigs_utxow"
    ) =>
        matches Err(UTxOWValidationError::MIRInsufficientGenesisSigsUTXOW { genesis_keys, quorum: 5 })
        if genesis_keys.len() == 4;
        "mir_insufficient_genesis_sigs_utxow - 4 genesis sigs"
    )]
    #[allow(clippy::result_large_err)]
    fn shelley_test((ctx, raw_tx): (TestContext, Vec<u8>)) -> Result<(), UTxOWValidationError> {
        let tx = MultiEraTx::decode_for_era(PallasEra::Shelley, &raw_tx).unwrap();
        let mtx = tx.as_alonzo().unwrap();
        let vkey_witnesses = acropolis_codec::map_vkey_witnesses(tx.vkey_witnesses()).0;
        let native_scripts = acropolis_codec::map_native_scripts(tx.native_scripts());
        validate(
            mtx,
            TxHash::from(*tx.hash()),
            &vkey_witnesses,
            &native_scripts,
            &ctx.shelley_params.gen_delegs,
            ctx.shelley_params.update_quorum,
        )
        .map_err(|e| *e)
    }
}
