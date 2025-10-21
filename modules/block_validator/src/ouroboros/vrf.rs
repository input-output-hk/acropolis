use std::{array::TryFromSliceError, ops::Deref};

use acropolis_common::protocol_params::Nonce;
use anyhow::Result;
use blake2::{digest::consts::U32, Blake2b, Digest};
use thiserror::Error;
use vrf_dalek::{
    errors::VrfError,
    vrf03::{PublicKey03, VrfProof03},
};

/// A VRF public key
#[derive(Debug, PartialEq)]
pub struct PublicKey(PublicKey03);

impl PublicKey {
    /// Size of a VRF public key, in bytes.
    pub const SIZE: usize = 32;

    /// Size of a VRF public key hash digest (Blake2b-256), in bytes.
    pub const HASH_SIZE: usize = 32;
}

impl AsRef<[u8]> for PublicKey {
    fn as_ref(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl Deref for PublicKey {
    type Target = [u8; PublicKey::SIZE];

    fn deref(&self) -> &Self::Target {
        self.0.as_bytes()
    }
}

impl From<&[u8; Self::SIZE]> for PublicKey {
    fn from(slice: &[u8; Self::SIZE]) -> Self {
        PublicKey(PublicKey03::from_bytes(slice))
    }
}

impl TryFrom<&[u8]> for PublicKey {
    type Error = TryFromSliceError;

    fn try_from(slice: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self::from(<&[u8; Self::SIZE]>::try_from(slice)?))
    }
}

/// A VRF input
///

pub type VrfInputHash = [u8; 32];
pub type VrfProofHash = [u8; 64];

#[derive(Debug, PartialEq)]
pub struct VrfInput(VrfInputHash);

impl VrfInput {
    /// Size of a VRF input challenge, in bytes
    pub const SIZE: usize = 32;

    /// Create a new input challenge from an absolute slot number and an epoch entropy (nonce) (a.k.a η0)
    pub fn new(absolute_slot_number: u64, epoch_nonce: &Nonce) -> Self {
        let mut hasher = Blake2b::<U32>::new();
        let mut data = Vec::<u8>::with_capacity(8 + 32);
        data.extend_from_slice(&absolute_slot_number.to_be_bytes());
        if let Some(hash) = epoch_nonce.hash {
            data.extend_from_slice(&hash);
        }
        hasher.update(&data);
        let hash: VrfInputHash = hasher.finalize().into();
        VrfInput(hash)
    }
}

impl AsRef<[u8]> for VrfInput {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Deref for VrfInput {
    type Target = VrfInputHash;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<&[u8; Self::SIZE]> for VrfInput {
    fn from(slice: &[u8; Self::SIZE]) -> Self {
        VrfInput(*slice)
    }
}

impl TryFrom<&[u8]> for VrfInput {
    type Error = TryFromSliceError;

    fn try_from(slice: &[u8]) -> Result<Self, Self::Error> {
        Ok(VrfInput::from(<&[u8; Self::SIZE]>::try_from(slice)?))
    }
}

/// A VRF proof formed by an Edward point and two scalars.
#[derive(Debug)]
pub struct Proof(VrfProof03);

impl Proof {
    /// Size of a VRF proof, in bytes.
    pub const SIZE: usize = 80;

    /// Size of a VRF proof hash digest (SHA512), in bytes.
    pub const HASH_SIZE: usize = 64;

    /// Verify a proof signature with a vrf public key. This will return a hash to compare with the original
    /// signature hash, but any non-error result is considered a successful verification without needing
    /// to do the extra comparison check.
    pub fn verify(
        &self,
        public_key: &PublicKey,
        input: &VrfInput,
    ) -> Result<VrfProofHash, ProofVerifyError> {
        Ok(self.0.verify(&public_key.0, input.as_ref())?)
    }
}

#[derive(Error, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ProofFromBytesError {
    #[error("Decompression from Edwards point failed.")]
    DecompressionFailed,
}

impl TryFrom<&[u8; Self::SIZE]> for Proof {
    type Error = ProofFromBytesError;

    fn try_from(slice: &[u8; Self::SIZE]) -> Result<Self, Self::Error> {
        Ok(Proof(VrfProof03::from_bytes(slice).map_err(
            |e| match e {
                VrfError::DecompressionFailed => ProofFromBytesError::DecompressionFailed,
                _ => unreachable!(
                    "Other error than decompression failure found when deserialising proof: {e:?}"
                ),
            },
        )?))
    }
}

impl From<&Proof> for [u8; Proof::SIZE] {
    fn from(proof: &Proof) -> Self {
        proof.0.to_bytes()
    }
}

impl From<&Proof> for [u8; Proof::HASH_SIZE] {
    fn from(proof: &Proof) -> [u8; Proof::HASH_SIZE] {
        proof.0.proof_to_hash()
    }
}

/// error that can be returned if the verification of a [`VrfProof`] fails
/// see [`VrfProof::verify`]
#[derive(Error, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[error("VRF proof verification failed: {:?}", .0)]
pub struct ProofVerifyError(
    #[from]
    #[source]
    #[serde(with = "serde_remote::VrfError")]
    VrfError,
);

mod serde_remote {
    #[derive(serde::Serialize, serde::Deserialize)]
    #[serde(remote = "super::VrfError")]
    pub enum VrfError {
        VerificationFailed,
        DecompressionFailed,
        PkSmallOrder,
        VrfOutputInvalid,
    }
}
