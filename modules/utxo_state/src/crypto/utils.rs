use crate::crypto::ed25519;
use acropolis_common::VKeyWitness;

#[allow(dead_code)]
pub fn verify_ed25519_signature(witness: &VKeyWitness, data_to_verify: &[u8]) -> bool {
    let mut pub_key_src: [u8; ed25519::PublicKey::SIZE] = [0; ed25519::PublicKey::SIZE];
    pub_key_src.copy_from_slice(&witness.vkey);
    let pub_key = ed25519::PublicKey::from(pub_key_src);

    let mut sig_src: [u8; ed25519::Signature::SIZE] = [0; ed25519::Signature::SIZE];
    sig_src.copy_from_slice(&witness.signature);
    let sig = ed25519::Signature::from(sig_src);

    pub_key.verify(data_to_verify, &sig)
}
