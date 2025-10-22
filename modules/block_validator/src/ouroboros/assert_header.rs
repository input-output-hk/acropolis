use crate::ouroboros::vrf;
use acropolis_common::{crypto::keyhash_256, protocol_params::Nonce, KeyHash, Slot};
use pallas::ledger::primitives::babbage::{derive_tagged_vrf_output, VrfDerivation};
use std::array::TryFromSliceError;
use thiserror::Error;

#[derive(Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum BlockHeaderValidationError {
    #[error("{0}")]
    KnownLeaderVrf(#[from] KnownLeaderVrfError),
    #[error("{0}")]
    VrfProof(#[from] VrfProofError),
}

/// AssertKnownLeaderVrfError

#[derive(Error, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[error(
    "declared leader's VRF credentials differs from those registered in the ledger (registered={} vs declared={})",
    hex::encode(&registered_vrf[0..7]),
    hex::encode(&declared_vrf[0..7]),
)]
pub struct KnownLeaderVrfError {
    registered_vrf: KeyHash,
    declared_vrf: KeyHash,
}

impl KnownLeaderVrfError {
    /// Asserts that the declared VRF credentials advertised in a block do indeed match those
    /// registered for the corresponding leader.
    pub fn new(registered_vrf_hash: &KeyHash, vrf_vkey: &[u8]) -> Result<(), Self> {
        let declared_vrf_hash = keyhash_256(vrf_vkey);
        if !declared_vrf_hash.eq(registered_vrf_hash) {
            return Err(Self {
                registered_vrf: registered_vrf_hash.clone(),
                declared_vrf: declared_vrf_hash,
            });
        }
        Ok(())
    }
}

// ------------------------------------------------------------ assert_vrf_proof

#[derive(Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum VrfProofError {
    #[error("Malformed VRF proof: {0}")]
    MalformedProof(#[from] vrf::ProofFromBytesError),

    #[error("Invalid VRF proof: {0}")]
    InvalidProof(vrf::ProofVerifyError, Slot, Nonce, Vec<u8>),

    #[error("could not convert slice to array")]
    TryFromSliceError,

    #[error(
        "Mismatch between the declared VRF proof hash in block ({}) and the computed one ({}).",
        hex::encode(&declared[0..7]),
        hex::encode(&computed[0..7]),
    )]
    ProofMismatch {
        // this is Proof Hash (sha512 hash)
        declared: Vec<u8>,
        computed: Vec<u8>,
    },

    #[error(
        "Mismatch between the declared VRF output in block ({}) and the computed one ({}).",
        hex::encode(&declared[0..7]),
        hex::encode(&computed[0..7]),
    )]
    OutputMismatch {
        declared: Vec<u8>,
        computed: Vec<u8>,
    },
}

impl From<TryFromSliceError> for VrfProofError {
    fn from(_: TryFromSliceError) -> Self {
        Self::TryFromSliceError
    }
}

impl PartialEq for VrfProofError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::MalformedProof(l0), Self::MalformedProof(r0)) => l0 == r0,
            (Self::InvalidProof(l0, l1, l2, l3), Self::InvalidProof(r0, r1, r2, r3)) => {
                l0 == r0 && l1 == r1 && l2 == r2 && l3 == r3
            }
            (Self::TryFromSliceError, Self::TryFromSliceError) => true,
            (
                Self::ProofMismatch {
                    declared: l_declared,
                    computed: l_computed,
                },
                Self::ProofMismatch {
                    declared: r_declared,
                    computed: r_computed,
                },
            ) => l_declared == r_declared && l_computed == r_computed,
            (
                Self::OutputMismatch {
                    declared: l_declared,
                    computed: l_computed,
                },
                Self::OutputMismatch {
                    declared: r_declared,
                    computed: r_computed,
                },
            ) => l_declared == r_declared && l_computed == r_computed,
            _ => false,
        }
    }
}

impl VrfProofError {
    /// Validate the VRF output from the block and its corresponding hash.
    pub fn new(
        absolute_slot: Slot,
        epoch_nonce: &Nonce,
        leader_vrf_output: &[u8],
        // Public Key from declared_vrf_key from block header
        leader_public_key: &vrf::PublicKey,
        // must be [u8; 64]
        unsafe_vrf_proof_hash: &[u8],
        // must be [u8; 80]
        unsafe_vrf_proof: &[u8],
    ) -> Result<(), Self> {
        let input = &vrf::VrfInput::new(absolute_slot, epoch_nonce);
        let block_proof_hash: [u8; vrf::Proof::HASH_SIZE] = unsafe_vrf_proof_hash.try_into()?;
        let block_proof: [u8; vrf::Proof::SIZE] = unsafe_vrf_proof.try_into()?;

        // Verify the VRF proof
        let vrf_proof = vrf::Proof::try_from(&block_proof)?;
        let proof_hash = vrf_proof.verify(leader_public_key, input).map_err(|e| {
            Self::InvalidProof(
                e,
                absolute_slot,
                epoch_nonce.clone(),
                leader_public_key.as_ref().to_vec(),
            )
        })?;
        if !proof_hash.as_slice().eq(&block_proof_hash) {
            return Err(Self::ProofMismatch {
                declared: block_proof_hash.to_vec(),
                computed: proof_hash.to_vec(),
            });
        }

        // The proof was valid. Make sure that the leader's output matches what was in the block
        let calculated_leader_vrf_output =
            derive_tagged_vrf_output(proof_hash.as_slice(), VrfDerivation::Leader);
        if calculated_leader_vrf_output.as_slice() != leader_vrf_output {
            return Err(Self::OutputMismatch {
                declared: leader_vrf_output.to_vec(),
                computed: calculated_leader_vrf_output,
            });
        }

        Ok(())
    }
}
