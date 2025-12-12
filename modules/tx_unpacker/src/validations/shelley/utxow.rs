//! Shelley era UTxOW Rules
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L278
#![allow(dead_code)]

use std::collections::HashSet;

use crate::crypto::verify_ed25519_signature;
use acropolis_common::{
    validation::UTxOWValidationError, AlonzoBabbageUpdateProposal, GenesisDelegates, KeyHash,
    NativeScript, ScriptHash, ShelleyAddressPaymentPart, StakeCredential, TxCertificate,
    TxCertificateWithPos, TxHash, UTXOValue, UTxOIdentifier, VKeyWitness, Withdrawal,
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

/// This function extracts required VKey Hashes
/// from TxCert (pallas type)
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/TxCert.hs#L583
fn get_cert_authors(
    cert_with_pos: &TxCertificateWithPos,
) -> (HashSet<KeyHash>, HashSet<ScriptHash>) {
    let mut vkey_hashes = HashSet::new();
    let mut script_hashes = HashSet::new();

    let mut parse_cred = |cred: &StakeCredential| match cred {
        StakeCredential::AddrKeyHash(vkey_hash) => {
            vkey_hashes.insert(*vkey_hash);
        }
        StakeCredential::ScriptHash(script_hash) => {
            script_hashes.insert(*script_hash);
        }
    };

    match &cert_with_pos.cert {
        // Deregistration requires witness from stake credential
        TxCertificate::StakeDeregistration(addr) => {
            parse_cred(&addr.credential);
        }
        // Delegation requries withness from delegator
        TxCertificate::StakeDelegation(deleg) => {
            parse_cred(&deleg.stake_address.credential);
        }
        // Pool registration requires witness from pool cold key and owners
        TxCertificate::PoolRegistration(pool_reg) => {
            vkey_hashes.insert(*pool_reg.operator);
            vkey_hashes
                .extend(pool_reg.pool_owners.iter().map(|o| o.get_hash()).collect::<HashSet<_>>());
        }
        // Pool retirement requires withness from pool cold key
        TxCertificate::PoolRetirement(retirement) => {
            vkey_hashes.insert(*retirement.operator);
        }
        // Genesis delegation requires witness from genesis key
        TxCertificate::GenesisKeyDelegation(gen_deleg) => {
            vkey_hashes.insert(*gen_deleg.genesis_delegate_hash);
        }
        _ => {}
    }

    (vkey_hashes, script_hashes)
}

/// Get VKey Witnesses needed for transaction
/// Get Scripts needed for transaction
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/UTxO.hs#L274
/// https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/UTxO.hs#L226
/// https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/UTxO.hs#L103
///
/// VKey Witnesses needed
/// 1. UTxO authors: keys that own the UTxO being spent
/// 2. Certificate authors: keys authorizing certificates
/// 3. Pool owners: owners that must sign pool registration
/// 4. Withdrawal authors: keys authorizing reward withdrawals
/// 5. Governance authors: keys authorizing governance actions (e.g. protocol update)
///
/// Script Witnesses needed
/// 1. Input scripts: scripts locking UTxO being spent
/// 2. Withdrawal scripts: scripts controlling reward accounts
/// 3. Certificate scripts: scripts in certificate credentials.
pub fn get_vkey_script_needed<F>(
    inputs: &[UTxOIdentifier],
    certificates: &[TxCertificateWithPos],
    withdrawals: &[Withdrawal],
    alonzo_babbage_update_proposal: &Option<AlonzoBabbageUpdateProposal>,
    lookup_utxo: F,
) -> (HashSet<KeyHash>, HashSet<ScriptHash>)
where
    F: Fn(&UTxOIdentifier) -> Result<Option<UTXOValue>>,
{
    let mut vkey_hashes = HashSet::new();
    let mut script_hashes = HashSet::new();

    // for each UTxO, extract the needed vkey and script hashes
    for utxo in inputs.iter() {
        if let Ok(Some(utxo)) = lookup_utxo(utxo) {
            // NOTE:
            // Need to check inputs from byron bootstrap addresses
            // with bootstrap witnesses
            if let Some(payment_part) = utxo.address.get_payment_part() {
                match payment_part {
                    ShelleyAddressPaymentPart::PaymentKeyHash(payment_key_hash) => {
                        vkey_hashes.insert(payment_key_hash);
                    }
                    ShelleyAddressPaymentPart::ScriptHash(script_hash) => {
                        script_hashes.insert(script_hash);
                    }
                }
            }
        }
    }

    // for each certificate, get the required vkey and script hashes
    for cert in certificates.iter() {
        let (v, s) = get_cert_authors(cert);
        vkey_hashes.extend(v);
        script_hashes.extend(s);
    }

    // for each withdrawal, get the required vkey and script hashes
    for withdrawal in withdrawals.iter() {
        match withdrawal.address.credential {
            StakeCredential::AddrKeyHash(vkey_hash) => {
                vkey_hashes.insert(vkey_hash);
            }
            StakeCredential::ScriptHash(script_hash) => {
                script_hashes.insert(script_hash);
            }
        }
    }

    // for each governance action, get the required vkey hashes
    if let Some(update) = alonzo_babbage_update_proposal {
        for (genesis_key, _) in update.proposals.iter() {
            vkey_hashes.insert(*genesis_key);
        }
    }

    (vkey_hashes, script_hashes)
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

/// Validate all needed scripts are provided in witnesses
/// No missing, no extra
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L386
pub fn validate_missing_extra_scripts(
    script_hashes_needed: &HashSet<ScriptHash>,
    native_scripts: &[NativeScript],
) -> Result<(), Box<UTxOWValidationError>> {
    // check for missing & extra scripts
    let mut scripts_used =
        native_scripts.iter().map(|script| (false, script.compute_hash())).collect::<Vec<_>>();
    for script_hash in script_hashes_needed.iter() {
        if let Some((used, _)) = scripts_used.iter_mut().find(|(u, h)| !(*u) && script_hash.eq(h)) {
            *used = true;
        } else {
            return Err(Box::new(
                UTxOWValidationError::MissingScriptWitnessesUTxOW {
                    script_hash: *script_hash,
                },
            ));
        }
    }

    for (used, script_hash) in scripts_used.iter() {
        if !*used {
            return Err(Box::new(
                UTxOWValidationError::ExtraneousScriptWitnessesUTXOW {
                    script_hash: *script_hash,
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

/// Validate that all required witnesses are provided
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L424
pub fn validate_needed_witnesses(
    vkey_hashes_needed: &HashSet<KeyHash>,
    vkey_hashes_provided: &HashSet<KeyHash>,
) -> Result<(), Box<UTxOWValidationError>> {
    for vkey_hash in vkey_hashes_needed.iter() {
        if !vkey_hashes_provided.contains(vkey_hash) {
            return Err(Box::new(UTxOWValidationError::MissingVKeyWitnessesUTxOW {
                key_hash: *vkey_hash,
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
                gensis_keys: genesis_sigs,
                quorum: update_quorum,
            },
        ));
    }

    Ok(())
}

pub fn validate(
    mtx: &alonzo::MintedTx,
    tx_hash: TxHash,
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
) -> Result<(), Box<UTxOWValidationError>> {
    let transaction_body = &mtx.transaction_body;
    let transaction_witness_set = &mtx.transaction_witness_set;

    // extract vkey_witnesses and native_scripts
    let vkey_witnesses = transaction_witness_set
        .vkeywitness
        .as_ref()
        .map(|witnesses| acropolis_codec::map_vkey_witnesses(witnesses))
        .unwrap_or_default();
    let native_scripts = transaction_witness_set
        .native_script
        .as_ref()
        .map(|scripts| acropolis_codec::map_native_scripts(scripts))
        .unwrap_or_default();

    // Extract vkey hashes from vkey_witnesses
    let vkey_hashes_provided = vkey_witnesses.iter().map(|w| w.key_hash()).collect::<HashSet<_>>();

    // validate native scripts
    validate_failed_native_scripts(
        &native_scripts,
        &vkey_hashes_provided,
        transaction_body.validity_interval_start,
        transaction_body.ttl,
    )?;

    // validate vkey witnesses signatures
    validate_verified_wits(&vkey_witnesses, tx_hash)?;

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
