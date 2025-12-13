use acropolis_common::{AddrKeyhash, NativeScript, VKeyWitness};
use pallas_primitives::{KeepRaw, alonzo};

pub fn map_vkey_witness(vkey_witness: &alonzo::VKeyWitness) -> VKeyWitness {
    VKeyWitness::new(vkey_witness.vkey.to_vec(), vkey_witness.signature.to_vec())
}

pub fn map_vkey_witnesses(vkey_witnesses: &[alonzo::VKeyWitness]) -> Vec<VKeyWitness> {
    vkey_witnesses.iter().map(map_vkey_witness).collect()
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
