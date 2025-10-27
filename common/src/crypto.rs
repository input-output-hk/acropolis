//! Common cryptography helper functions for Acropolis

use cryptoxide::hashing::blake2b::Blake2b;

/// Get a Blake2b-256 hash of a key
pub fn keyhash_256(key: &[u8]) -> Vec<u8> {
    let mut context = Blake2b::<256>::new();
    context.update_mut(key);
    context.finalize().to_vec()
}

/// Get a Blake2b-224 hash of a key
pub fn keyhash_224(key: &[u8]) -> Vec<u8> {
    let mut context = Blake2b::<224>::new();
    context.update_mut(key);
    context.finalize().to_vec()
}
