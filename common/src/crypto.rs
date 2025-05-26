//! Common cryptography helper functions for Acropolis

use blake2::{Blake2b, Digest, digest::consts::U32};
use crate::types::KeyHash;

/// Get a Blake2b-256 hash of a key
pub fn keyhash(key: &[u8]) -> KeyHash {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(key);
    hasher.finalize().to_vec()
}

