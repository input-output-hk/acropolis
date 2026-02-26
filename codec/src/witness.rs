use acropolis_common::{AddrKeyhash, NativeScript, Signature, VKey, VKeyWitness};
use anyhow::{Result, anyhow};
use pallas_primitives::{KeepRaw, alonzo};

fn map_vkey_witness(vkey_witness: &alonzo::VKeyWitness) -> Result<VKeyWitness> {
    Ok(VKeyWitness::new(
        VKey::try_from(vkey_witness.vkey.to_vec()).map_err(|_| anyhow!("Invalid vkey length"))?,
        Signature::try_from(vkey_witness.signature.to_vec())
            .map_err(|_| anyhow!("Invalid signature length"))?,
    ))
}

pub fn map_vkey_witnesses(
    vkey_witnesses: &[alonzo::VKeyWitness],
) -> (Vec<VKeyWitness>, Vec<String>) {
    let mut wits = Vec::new();
    let mut errors = Vec::new();
    for (index, vkey_witness) in vkey_witnesses.iter().enumerate() {
        match map_vkey_witness(vkey_witness) {
            Ok(vkey_witness) => {
                wits.push(vkey_witness);
            }
            Err(e) => {
                errors.push(format!("Invalid vkey witness {index}: {e}"));
            }
        }
    }
    (wits, errors)
}

pub fn map_native_script(script: &alonzo::NativeScript) -> NativeScript {
    match script {
        alonzo::NativeScript::ScriptPubkey(addr_key_hash) => {
            NativeScript::ScriptPubkey(AddrKeyhash::from(**addr_key_hash))
        }
        alonzo::NativeScript::ScriptAll(scripts) => {
            NativeScript::ScriptAll(scripts.iter().map(map_native_script).collect())
        }
        alonzo::NativeScript::ScriptAny(scripts) => {
            NativeScript::ScriptAny(scripts.iter().map(map_native_script).collect())
        }
        alonzo::NativeScript::ScriptNOfK(n, scripts) => {
            NativeScript::ScriptNOfK(*n, scripts.iter().map(map_native_script).collect())
        }
        alonzo::NativeScript::InvalidBefore(slot_no) => NativeScript::InvalidBefore(*slot_no),
        alonzo::NativeScript::InvalidHereafter(slot_no) => NativeScript::InvalidHereafter(*slot_no),
    }
}

pub fn map_native_scripts<'b>(
    native_scripts: &[KeepRaw<'b, alonzo::NativeScript>],
) -> Vec<NativeScript> {
    native_scripts.iter().map(|script| map_native_script(script)).collect()
}
