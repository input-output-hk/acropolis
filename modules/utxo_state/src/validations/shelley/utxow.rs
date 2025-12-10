//! Shelley era UTxOW Rules
//! Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L278
#![allow(dead_code)]

use std::collections::HashSet;

use crate::crypto::verify_ed25519_signature;
use acropolis_common::{
    validation::UTxOWValidationError, AddrKeyhash, GenesisDelegates, KeyHash, NativeScript,
    ScriptHash, ShelleyAddressPaymentPart, TxHash, UTXOValue, UTxOIdentifier, VKeyWitness,
};
use anyhow::Result;
use pallas::ledger::primitives::alonzo;

fn get_vkey_witnesses(tx: &alonzo::MintedTx) -> Vec<VKeyWitness> {
    tx.transaction_witness_set
        .vkeywitness
        .as_ref()
        .map(|witnesses| {
            witnesses
                .iter()
                .map(|witness| VKeyWitness::new(witness.vkey.to_vec(), witness.signature.to_vec()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn eval_native_script(
    native_script: &NativeScript,
    vkey_hashes_provided: &HashSet<KeyHash>,
    low_bnd: Option<u64>,
    upp_bnd: Option<u64>,
) -> bool {
    match native_script {
        NativeScript::ScriptAll(scripts) => scripts
            .iter()
            .all(|script| eval_native_script(script, vkey_hashes_provided, low_bnd, upp_bnd)),
        NativeScript::ScriptAny(scripts) => scripts
            .iter()
            .any(|script| eval_native_script(script, vkey_hashes_provided, low_bnd, upp_bnd)),
        NativeScript::ScriptPubkey(hash) => vkey_hashes_provided.contains(hash),
        NativeScript::ScriptNOfK(val, scripts) => {
            let count = scripts
                .iter()
                .map(|script| eval_native_script(script, vkey_hashes_provided, low_bnd, upp_bnd))
                .fold(0, |x, y| x + y as u32);
            count >= *val
        }
        NativeScript::InvalidBefore(val) => {
            match low_bnd {
                Some(time) => *val >= time,
                None => false, // as per mary-ledger.pdf, p.20
            }
        }
        NativeScript::InvalidHereafter(val) => {
            match upp_bnd {
                Some(time) => *val <= time,
                None => false, // as per mary-ledger.pdf, p.20
            }
        }
    }
}

/// This function extracts required VKey Hashes
/// from TxCert (pallas type)
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/TxCert.hs#L583
fn get_cert_authors(cert: &alonzo::Certificate) -> (HashSet<KeyHash>, HashSet<ScriptHash>) {
    let mut vkey_hashes = HashSet::new();
    let mut script_hashes = HashSet::new();

    let mut parse_cred = |cred: &alonzo::StakeCredential| match cred {
        alonzo::StakeCredential::AddrKeyhash(vkey_hash) => {
            vkey_hashes.insert(AddrKeyhash::from(**vkey_hash));
        }
        alonzo::StakeCredential::ScriptHash(script_hash) => {
            script_hashes.insert(ScriptHash::from(**script_hash));
        }
    };

    match cert {
        // Deregistration requires witness from stake credential
        alonzo::Certificate::StakeDeregistration(cred) => {
            parse_cred(cred);
        }
        // Delegation requries withness from delegator
        alonzo::Certificate::StakeDelegation(cred, _) => {
            parse_cred(cred);
        }
        // Pool registration requires witness from pool cold key and owners
        alonzo::Certificate::PoolRegistration {
            operator,
            pool_owners,
            ..
        } => {
            vkey_hashes.insert(AddrKeyhash::from(**operator));
            vkey_hashes.extend(pool_owners.iter().map(|o| AddrKeyhash::from(**o)));
        }
        // Pool retirement requires withness from pool cold key
        alonzo::Certificate::PoolRetirement(operator, _) => {
            vkey_hashes.insert(AddrKeyhash::from(**operator));
        }
        // Genesis delegation requires witness from genesis key
        alonzo::Certificate::GenesisKeyDelegation(_, genesis_delegate_hash, _) => {
            vkey_hashes.insert(AddrKeyhash::try_from(genesis_delegate_hash.as_ref()).unwrap());
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
    transaction_body: &alonzo::TransactionBody,
    tx_hash: TxHash,
    lookup_utxo: F,
) -> (HashSet<KeyHash>, HashSet<ScriptHash>)
where
    F: Fn(UTxOIdentifier) -> Result<Option<UTXOValue>>,
{
    let mut vkey_hashes = HashSet::new();
    let mut script_hashes = HashSet::new();

    // for each UTxO, extract the needed vkey and script hashes
    for utxo in transaction_body.inputs.iter() {
        let tx_out_ref = UTxOIdentifier::new(tx_hash, utxo.index as u16);
        if let Ok(Some(utxo)) = lookup_utxo(tx_out_ref) {
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
    for cert in transaction_body.certificates.as_ref().unwrap_or(&vec![]) {
        let (v, s) = get_cert_authors(cert);
        vkey_hashes.extend(v);
        script_hashes.extend(s);
    }

    // for each withdrawal, get the required vkey and script hashes
    if let Some(withdrawals) = transaction_body.withdrawals.as_ref() {
        for (key_hash, _) in withdrawals.iter() {
            // NOTE:
            // Withdrawal is guaranteed to be always AddrKeyhash???
            vkey_hashes.insert(AddrKeyhash::try_from(key_hash.as_ref()).unwrap());
        }
    }

    // for each governance action, get the required vkey hashes
    if let Some(update) = transaction_body.update.as_ref() {
        for (genesis_key, _) in update.proposed_protocol_parameter_updates.iter() {
            vkey_hashes.insert(AddrKeyhash::try_from(genesis_key.as_ref()).unwrap());
        }
    }

    (vkey_hashes, script_hashes)
}

/// Validate Native Scripts from Transaction witnesses
/// Reference: https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Utxow.hs#L373
pub fn validate_failed_native_scripts(
    native_scripts: &Vec<NativeScript>,
    vkey_hashes_provided: &HashSet<KeyHash>,
    low_bnd: Option<u64>,
    upp_bnd: Option<u64>,
) -> Result<(), Box<UTxOWValidationError>> {
    for native_script in native_scripts {
        if !eval_native_script(native_script, vkey_hashes_provided, low_bnd, upp_bnd) {
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
    transaction_body: &alonzo::TransactionBody,
    vkey_hashes_provided: &HashSet<KeyHash>,
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
) -> Result<(), Box<UTxOWValidationError>> {
    let has_mir = transaction_body
        .certificates
        .as_ref()
        .map(|certs| {
            certs
                .iter()
                .any(|cert| matches!(cert, alonzo::Certificate::MoveInstantaneousRewardsCert(_)))
        })
        .unwrap_or(false);
    if !has_mir {
        return Ok(());
    }

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

pub fn validate_withnesses<F>(
    tx: &alonzo::MintedTx,
    tx_hash: TxHash,
    genesis_delegs: &GenesisDelegates,
    update_quorum: u32,
    lookup_utxo: F,
) -> Result<(), Box<UTxOWValidationError>>
where
    F: Fn(UTxOIdentifier) -> Result<Option<UTXOValue>>,
{
    let transaction_body = &tx.transaction_body;
    // Extract required vkey and script hashes
    let (vkey_hashes_needed, script_hashes_needed) =
        get_vkey_script_needed(transaction_body, tx_hash, lookup_utxo);

    // Extract vkey hashes from witnesses
    let vkey_witnesses = get_vkey_witnesses(tx);
    let vkey_hashes_provided = vkey_witnesses.iter().map(|w| w.key_hash()).collect::<HashSet<_>>();

    let native_scripts: Vec<NativeScript> = tx
        .transaction_witness_set
        .native_script
        .as_ref()
        .map(|scripts| {
            scripts.iter().map(|script| acropolis_codec::map_native_script(script)).collect()
        })
        .unwrap_or_default();

    // validate native scripts
    validate_failed_native_scripts(
        &native_scripts,
        &vkey_hashes_provided,
        transaction_body.validity_interval_start,
        transaction_body.ttl,
    )?;

    // validate missing & extra scripts
    validate_missing_extra_scripts(&script_hashes_needed, &native_scripts)?;

    // validate vkey witnesses signatures
    validate_verified_wits(&vkey_witnesses, tx_hash)?;

    // validate required vkey witnesses are provided
    validate_needed_witnesses(&vkey_hashes_needed, &vkey_hashes_provided)?;

    // NOTE:
    // need to validate metadata

    // validate mir certificate genesis sig
    validate_mir_insufficient_genesis_sigs(
        transaction_body,
        &vkey_hashes_provided,
        genesis_delegs,
        update_quorum,
    )?;

    Ok(())
}
