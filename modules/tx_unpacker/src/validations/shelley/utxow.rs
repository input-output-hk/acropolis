//! Shelley era UTxOW Rules
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L278

use std::collections::HashSet;

use crate::crypto::verify_ed25519_signature;
use acropolis_common::{
    crypto::keyhash_256, protocol_params::ProtocolVersion, soft_fork,
    validation::UTxOWValidationError, DataHash, GenesisDelegates, KeyHash, Metadata, Metadatum,
    NativeScript, TxHash, VKeyWitness,
};
use anyhow::Result;
use pallas::{codec::utils::Nullable, ledger::primitives::alonzo};

const METADATUM_MAX_BYTES: usize = 64;

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

fn get_aux_data_hash(
    mtx: &alonzo::MintedTx,
) -> Result<Option<DataHash>, Box<UTxOWValidationError>> {
    let aux_data_hash = match mtx.transaction_body.auxiliary_data_hash.as_ref() {
        Some(x) => Some(DataHash::try_from(x.to_vec()).map_err(|_| {
            Box::new(UTxOWValidationError::InvalidMetadataHash {
                reason: "invalid metadata hash".to_string(),
            })
        })?),
        None => None,
    };
    Ok(aux_data_hash)
}

fn get_aux_data(mtx: &alonzo::MintedTx) -> Option<Vec<u8>> {
    match &mtx.auxiliary_data {
        Nullable::Some(x) => Some(x.raw_cbor().to_vec()),
        _ => None,
    }
}

fn validate_metadatum(metadatum: &Metadatum) -> bool {
    match metadatum {
        Metadatum::Int(_) => true,
        Metadatum::Bytes(b) => b.len() <= METADATUM_MAX_BYTES,
        Metadatum::Text(s) => s.len() <= METADATUM_MAX_BYTES,
        Metadatum::Array(a) => a.iter().all(validate_metadatum),
        Metadatum::Map(m) => m.iter().all(|(k, v)| validate_metadatum(k) && validate_metadatum(v)),
    }
}

/// Validate Native Scripts from Transaction witnesses
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L373
pub fn validate_native_scripts(
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
pub fn validate_vkey_witnesses(
    vkey_witnesses: &HashSet<VKeyWitness>,
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

/// Validate transaction's aux metadata
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/libs/cardano-ledger-core/src/Cardano/Ledger/Metadata.hs#L75
pub fn validate_tx_aux_metadata(
    metadata: &Option<Metadata>,
) -> Result<(), Box<UTxOWValidationError>> {
    match metadata.as_ref() {
        Some(metadata) => {
            if metadata.as_ref().iter().all(|(_, v)| validate_metadatum(v)) {
                Ok(())
            } else {
                Err(Box::new(UTxOWValidationError::InvalidMetadata {
                    reason: "metadatum value-size exceeds 64 bytes".to_string(),
                }))
            }
        }
        None => Ok(()),
    }
}

/// Validate metadata (hash must match with computed one, check metadatum value-size when pv > 2.0)
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L440
pub fn validate_metadata(
    aux_data_hash: Option<DataHash>,
    aux_data: Option<Vec<u8>>,
    metadata: &Option<Metadata>,
    protocol_version: &ProtocolVersion,
) -> Result<(), Box<UTxOWValidationError>> {
    match (aux_data_hash, aux_data) {
        (None, None) => Ok(()),
        (Some(aux_data_hash), Some(aux_data)) => {
            let computed_hash = keyhash_256(aux_data.as_slice());
            if aux_data_hash != computed_hash {
                return Err(Box::new(UTxOWValidationError::ConflictingMetadataHash {
                    expected: aux_data_hash,
                    actual: computed_hash,
                }));
            }

            if soft_fork::should_check_metadata(protocol_version) {
                validate_tx_aux_metadata(metadata)?;
            }

            Ok(())
        }
        (Some(aux_data_hash), None) => Err(Box::new(UTxOWValidationError::MissingTxMetadata {
            metadata_hash: aux_data_hash,
        })),
        (None, Some(aux_data)) => Err(Box::new(UTxOWValidationError::MissingTxBodyMetadataHash {
            metadata_hash: keyhash_256(aux_data.as_slice()),
        })),
    }
}

/// Validate genesis keys signatures for MIR certificate
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L463
pub fn validate_mir_genesis_sigs(
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

#[allow(clippy::too_many_arguments)]
pub fn validate(
    mtx: &alonzo::MintedTx,
    tx_hash: TxHash,
    vkey_witnesses: &HashSet<VKeyWitness>,
    native_scripts: &[NativeScript],
    metadata: &Option<Metadata>,
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
    protocol_version: &ProtocolVersion,
) -> Result<(), Box<UTxOWValidationError>> {
    let transaction_body = &mtx.transaction_body;

    // Extract vkey hashes from vkey_witnesses
    let vkey_hashes_provided = vkey_witnesses.iter().map(|w| w.key_hash()).collect::<HashSet<_>>();

    // validate native scripts
    validate_native_scripts(
        native_scripts,
        &vkey_hashes_provided,
        transaction_body.validity_interval_start,
        transaction_body.ttl,
    )?;

    // validate vkey witnesses signatures
    validate_vkey_witnesses(vkey_witnesses, tx_hash)?;

    // validate metadata
    validate_metadata(
        get_aux_data_hash(mtx)?,
        get_aux_data(mtx),
        metadata,
        protocol_version,
    )?;

    // validate mir certificate genesis sig
    if has_mir_certificate(mtx) {
        validate_mir_genesis_sigs(&vkey_hashes_provided, genesis_delegs, update_quorum)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{
        test_utils::{to_pallas_era, TestContext},
        validation_fixture,
    };
    use pallas::ledger::traverse::MultiEraTx;
    use test_case::test_case;

    #[test_case(validation_fixture!(
        "shelley",
        "20ded0bfef32fc5eefba2c1f43bcd99acc0b1c3284617c3cb355ad0eadccaa6e"
    ) =>
        matches Ok(());
        "shelley - valid transaction 1 - with byron input & output"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "da350a9e2a14717172cee9e37df02b14b5718ea1934ce6bea25d739d9226f01b"
    ) =>
        matches Ok(());
        "shelley - valid transaction 2"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "0c993cb361c213e5b04d241321975e22870a0d658c03ea5b817c24fc48252ea0"
    ) =>
        matches Ok(());
        "shelley - valid transaction 2 - with mir certificates"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "c220e20cc480df9ce7cd871df491d7390c6a004b9252cf20f45fc3c968535b4a"
    ) =>
        matches Ok(());
        "shelley - valid transaction 3 - with metadata"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "da350a9e2a14717172cee9e37df02b14b5718ea1934ce6bea25d739d9226f01b", 
        "invalid_witnesses_utxow"
    ) =>
        matches Err(UTxOWValidationError::InvalidWitnessesUTxOW { key_hash, .. })
        if key_hash == KeyHash::from_str("b0baefb8dedefd7ec935514696ea5a66e9520f31dc8867737f0f0084").unwrap();
        "shelley - invalid_witnesses_utxow"
    )]
    #[test_case(validation_fixture!(
        "shelley",
        "0c993cb361c213e5b04d241321975e22870a0d658c03ea5b817c24fc48252ea0",
        "mir_insufficient_genesis_sigs_utxow"
    ) =>
        matches Err(UTxOWValidationError::MIRInsufficientGenesisSigsUTXOW { genesis_keys, quorum: 5 })
        if genesis_keys.len() == 4;
        "shelley - mir_insufficient_genesis_sigs_utxow - 4 genesis sigs"
    )]
    #[test_case(validation_fixture!(
        "allegra",
        "aee87cc55b9a4254497d2b2ea07981f32fd2cf0e1b4f94349a8c23f3d39eb576"
    ) =>
        matches Ok(());
        "allegra - valid transaction 1"
    )]
    #[test_case(validation_fixture!(
        "allegra",
        "fabfad0aaa2b52b8304f45edc0350659ad0d73f9d1065d9cd3ef7d5a599ac57d"
    ) =>
        matches Ok(());
        "allegra - valid transaction 2 - with mir certificates"
    )]
    #[test_case(validation_fixture!(
        "allegra",
        "aee87cc55b9a4254497d2b2ea07981f32fd2cf0e1b4f94349a8c23f3d39eb576",
        "invalid_witnesses_utxow"
    ) =>
        matches Err(UTxOWValidationError::InvalidWitnessesUTxOW { key_hash, .. })
        if key_hash == KeyHash::from_str("6a27b4eec5817b3f6c6af704c8936f2a6505c208e8c4933fdc154a08").unwrap();
        "allegra - invalid_witnesses_utxow"
    )]
    #[test_case(validation_fixture!(
        "allegra",
        "fabfad0aaa2b52b8304f45edc0350659ad0d73f9d1065d9cd3ef7d5a599ac57d",
        "mir_insufficient_genesis_sigs_utxow"
    ) =>
        matches Err(UTxOWValidationError::MIRInsufficientGenesisSigsUTXOW { genesis_keys, quorum: 5 })
        if genesis_keys.len() == 4;
        "allegra - mir_insufficient_genesis_sigs_utxow - 4 genesis sigs"
    )]
    #[allow(clippy::result_large_err)]
    fn shelley_utxow_test(
        (ctx, raw_tx, era): (TestContext, Vec<u8>, &str),
    ) -> Result<(), UTxOWValidationError> {
        let tx = MultiEraTx::decode_for_era(to_pallas_era(era), &raw_tx).unwrap();
        let mtx = tx.as_alonzo().unwrap();
        let vkey_witnesses = acropolis_codec::map_vkey_witnesses(tx.vkey_witnesses()).0;
        let native_scripts = acropolis_codec::map_native_scripts(tx.native_scripts());
        let metadata = acropolis_codec::map_metadata(&tx.metadata());
        validate(
            mtx,
            TxHash::from(*tx.hash()),
            &vkey_witnesses,
            &native_scripts,
            &metadata,
            &ctx.shelley_params.gen_delegs,
            ctx.shelley_params.update_quorum,
            &ctx.shelley_params.protocol_params.protocol_version,
        )
        .map_err(|e| *e)
    }
}
