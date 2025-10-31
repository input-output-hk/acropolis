//! Common cryptography helper functions for Acropolis

use crate::hash::Hash;
use cryptoxide::hashing::blake2b::Blake2b;

/// Get a Blake2b-256 hash of a key
///
/// Returns a 32-byte hash.
pub fn keyhash_256(key: &[u8]) -> Hash<32> {
    let mut context = Blake2b::<256>::new();
    context.update_mut(key);
    Hash::new(context.finalize())
}

/// Get a Blake2b-224 hash of a key
///
/// Returns a 28-byte hash.
pub fn keyhash_224(key: &[u8]) -> Hash<28> {
    let mut context = Blake2b::<224>::new();
    context.update_mut(key);
    Hash::new(context.finalize())
}
