use acropolis_common::protocol_params::Nonce;
use blake2::{digest::consts::U32, Blake2b, Digest};

use crate::ouroboros::types::Seed;

/// Construct a seed to use in the VRF computation.
///
/// This seed is used for VRF proofs in the Praos consensus protocol.
/// It combines the slot number and epoch nonce, optionally with a
/// universal constant for domain separation.
///
/// # Arguments
///
/// * `uc_nonce` - Universal constant nonce (domain separator)
///   - Use `seed_eta()` for randomness/eta computation
///   - Use `seed_l()` for leader election computation  
/// * `slot` - The slot number
/// * `e_nonce` - The epoch nonce (randomness from the epoch)
///
/// # Returns
///
/// A `Seed` that can be used for VRF computation
///
/// https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/BHeader.hs#L405
///
pub fn mk_seed(uc_nonce: &Nonce, slot: u64, epoch_nonce: &Nonce) -> Seed {
    // 8 bytes for slot + optionally 32 bytes for epoch nonce
    let mut data = Vec::with_capacity(8 + 32);
    data.extend_from_slice(&slot.to_be_bytes());
    if let Some(e_hash) = epoch_nonce.hash {
        data.extend_from_slice(&e_hash);
    }
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(data);
    let seed_hash: [u8; 32] = hasher.finalize().into();

    // XOR with universal constant if provided
    let final_hash = match uc_nonce.hash.as_ref() {
        Some(uc_hash) => xor_hash(&seed_hash, uc_hash),
        None => seed_hash,
    };

    Seed::from(final_hash)
}

fn xor_hash(hash1: &[u8; 32], hash2: &[u8; 32]) -> [u8; 32] {
    let mut result = [0u8; 32];
    for i in 0..32 {
        result[i] = hash1[i] ^ hash2[i];
    }
    result
}
