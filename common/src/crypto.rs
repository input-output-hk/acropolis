//! Common cryptography helper functions for Acropolis

use crate::types::KeyHash;
use blake2::{digest::consts::U32, Blake2b, Digest};

/// Get a Blake2b-256 hash of a key
pub fn keyhash(key: &[u8]) -> KeyHash {
    let mut hasher = Blake2b::<U32>::new();
    hasher.update(key);
    hasher.finalize().to_vec()
}
