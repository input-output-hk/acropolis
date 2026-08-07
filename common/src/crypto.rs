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

/// Get a Blake2b-224 hash of a key with a tag
///
/// Returns a 28-byte hash.
pub fn keyhash_224_tagged(tag: u8, bytes: &[u8]) -> Hash<28> {
    let mut context = Blake2b::<224>::new();
    context.update_mut(&[tag]);
    context.update_mut(bytes);
    Hash::new(context.finalize())
}

/// Verify an Ed25519 signature using cardano-node's acceptance rule.
/// Returns `false` on any malformed input rather than panicking.
pub fn verify_ed25519_signature_strict(vkey: &[u8], signature: &[u8], message: &[u8]) -> bool {
    let Ok(vkey) = <[u8; 32]>::try_from(vkey) else {
        return false;
    };
    let Ok(signature) = <[u8; 64]>::try_from(signature) else {
        return false;
    };
    let Ok(verifying_key) = ed25519_dalek::VerifyingKey::from_bytes(&vkey) else {
        return false;
    };
    verifying_key.verify_strict(message, &ed25519_dalek::Signature::from_bytes(&signature)).is_ok()
}
