use crate::ouroboros::{overlay_shedule, vrf};
use acropolis_common::{
    crypto::keyhash_256,
    genesis_values::GenesisDelegs,
    protocol_params::{Nonce, PraosParams, ShelleyParams},
    BlockInfo, KeyHash, Slot,
};
use anyhow::Result;
use pallas::ledger::{
    primitives::babbage::{derive_tagged_vrf_output, VrfDerivation},
    traverse::MultiEraHeader,
};
use std::array::TryFromSliceError;
use thiserror::Error;

#[derive(Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum VrfValidationError {
    #[error("{0}")]
    ShelleyParams(String),
    #[error("{0}")]
    KnownLeaderVrf(#[from] KnownLeaderVrfError),
    #[error("{0}")]
    TPraosVrfProof(#[from] TPraosVrfProofError),
    #[error("{0}")]
    PraosVrfProof(#[from] PraosVrfProofError),
}

// ------------------------------------------------------------ assert_known_leader_vrf

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

// ------------------------------------------------------------ assert_tpraos_vrf_proof

#[derive(Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum TPraosVrfProofError {
    #[error("Malformed Nonce VRF proof: {0}")]
    MalformedNonceProof(vrf::ProofFromBytesError),

    /// (error, absolute_slot, nonce, leader_public_key)
    #[error("Invalid Nonce VRF proof: {0}")]
    InvalidNonceProof(vrf::ProofVerifyError, Slot, Nonce, Vec<u8>),

    #[error("Malformed Leader VRF proof: {0}")]
    MalformedLeaderProof(vrf::ProofFromBytesError),

    #[error("Invalid Leader VRF proof: {0}")]
    InvalidLeaderProof(vrf::ProofVerifyError, Slot, Nonce, Vec<u8>),

    #[error("could not convert slice to array")]
    TryFromSliceError,

    #[error(
        "Mismatch between the declared Nonce VRF proof hash in block ({}) and the computed one ({}).",
        hex::encode(&declared[0..7]),
        hex::encode(&computed[0..7]),
    )]
    NonceProofMismatch {
        // this is Proof Hash (sha512 hash)
        declared: Vec<u8>,
        computed: Vec<u8>,
    },

    #[error(
        "Mismatch between the declared Leader VRF proof hash in block ({}) and the computed one ({}).",
        hex::encode(&declared[0..7]),
        hex::encode(&computed[0..7]),
    )]
    LeaderProofMismatch {
        declared: Vec<u8>,
        computed: Vec<u8>,
    },
}

impl From<TryFromSliceError> for TPraosVrfProofError {
    fn from(_: TryFromSliceError) -> Self {
        Self::TryFromSliceError
    }
}

impl PartialEq for TPraosVrfProofError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::MalformedNonceProof(l0), Self::MalformedNonceProof(r0)) => l0 == r0,
            (Self::InvalidNonceProof(l0, l1, l2, l3), Self::InvalidNonceProof(r0, r1, r2, r3)) => {
                l0 == r0 && l1 == r1 && l2 == r2 && l3 == r3
            }
            (Self::MalformedLeaderProof(l0), Self::MalformedLeaderProof(r0)) => l0 == r0,
            (
                Self::InvalidLeaderProof(l0, l1, l2, l3),
                Self::InvalidLeaderProof(r0, r1, r2, r3),
            ) => l0 == r0 && l1 == r1 && l2 == r2 && l3 == r3,
            (Self::TryFromSliceError, Self::TryFromSliceError) => true,
            (
                Self::NonceProofMismatch {
                    declared: l_declared,
                    computed: l_computed,
                },
                Self::NonceProofMismatch {
                    declared: r_declared,
                    computed: r_computed,
                },
            ) => l_declared == r_declared && l_computed == r_computed,
            (
                Self::LeaderProofMismatch {
                    declared: l_declared,
                    computed: l_computed,
                },
                Self::LeaderProofMismatch {
                    declared: r_declared,
                    computed: r_computed,
                },
            ) => l_declared == r_declared && l_computed == r_computed,
            _ => false,
        }
    }
}

/// This is VRF Validation for TPraos Protocol
/// In TPraos, they use different validation flow than Praos Protocol.
/// Main difference is in TPraos, there are 2 VRF Outputs and Proofs when in Praos, those are combined into one using derived Tag.
/// So need to validate 2 VRF Proofs - one for Nonce and one for Leader
///
impl TPraosVrfProofError {
    /// Validate the VRF output from the block and its corresponding hash.
    /// in TPraos Protocol
    pub fn new(
        absolute_slot: Slot,
        epoch_nonce: &Nonce,
        // Public Key from declared_vrf_key from block header
        leader_public_key: &vrf::PublicKey,
        unsafe_nonce_vrf_proof_hash: &[u8],  // must be [u8; 64]
        unsafe_nonce_vrf_proof: &[u8],       // must be [u8; 80]
        unsafe_leader_vrf_proof_hash: &[u8], // must be [u8; 64]
        unsafe_leader_vrf_proof: &[u8],      // must be [u8; 80]
    ) -> Result<(), Self> {
        // For nonce proof validation
        let seed_eta = Nonce::seed_eta();
        // For leader proof validation
        let seed_l = Nonce::seed_l();

        // https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/TPraos.hs#L365
        let rho_seed = vrf::VrfInput::mk_seed(absolute_slot, epoch_nonce, &seed_eta);
        // https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/TPraos.hs#L366
        let y_seed = vrf::VrfInput::mk_seed(absolute_slot, epoch_nonce, &seed_l);

        let nonce_vrf_proof_hash: [u8; vrf::Proof::HASH_SIZE] =
            unsafe_nonce_vrf_proof_hash.try_into()?;
        let nonce_vrf_proof: [u8; vrf::Proof::SIZE] = unsafe_nonce_vrf_proof.try_into()?;
        let leader_vrf_proof_hash: [u8; vrf::Proof::HASH_SIZE] =
            unsafe_leader_vrf_proof_hash.try_into()?;
        let leader_vrf_proof: [u8; vrf::Proof::SIZE] = unsafe_leader_vrf_proof.try_into()?;

        // Verify Nonce VRF proof
        let nonce_vrf_proof =
            vrf::Proof::try_from(&nonce_vrf_proof).map_err(|e| Self::MalformedNonceProof(e))?;
        let nonce_proof_hash =
            nonce_vrf_proof.verify(leader_public_key, &rho_seed).map_err(|e| {
                Self::InvalidNonceProof(
                    e,
                    absolute_slot,
                    epoch_nonce.clone(),
                    leader_public_key.as_ref().to_vec(),
                )
            })?;
        if !nonce_proof_hash.as_slice().eq(&nonce_vrf_proof_hash) {
            return Err(Self::NonceProofMismatch {
                declared: nonce_vrf_proof_hash.to_vec(),
                computed: nonce_proof_hash.to_vec(),
            });
        }

        // Verify Leader VRF proof
        let leader_vrf_proof =
            vrf::Proof::try_from(&leader_vrf_proof).map_err(|e| Self::MalformedLeaderProof(e))?;
        let leader_proof_hash =
            leader_vrf_proof.verify(leader_public_key, &y_seed).map_err(|e| {
                Self::InvalidLeaderProof(
                    e,
                    absolute_slot,
                    epoch_nonce.clone(),
                    leader_public_key.as_ref().to_vec(),
                )
            })?;
        if !leader_proof_hash.as_slice().eq(&leader_vrf_proof_hash) {
            return Err(Self::LeaderProofMismatch {
                declared: leader_vrf_proof_hash.to_vec(),
                computed: leader_proof_hash.to_vec(),
            });
        }

        Ok(())
    }
}

// ------------------------------------------------------------ assert_praos_vrf_proof

#[derive(Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum PraosVrfProofError {
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

impl From<TryFromSliceError> for PraosVrfProofError {
    fn from(_: TryFromSliceError) -> Self {
        Self::TryFromSliceError
    }
}

impl PartialEq for PraosVrfProofError {
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

impl PraosVrfProofError {
    /// Validate the VRF output from the block and its corresponding hash.
    /// in TPraos Protocol
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
        let input = &vrf::VrfInput::mk_vrf_input(absolute_slot, epoch_nonce);
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

pub fn validate_vrf(
    block_info: &BlockInfo,
    header: &MultiEraHeader,
    shelley_params: &ShelleyParams,
    praos_params: &PraosParams,
    genesis_delegs: &GenesisDelegs,
) -> Result<(), VrfValidationError> {
    let decentralisation_param = shelley_params.protocol_params.decentralisation_param;
    let active_slots_coeff = praos_params.active_slots_coeff;

    // first look up for overlay slot
    let obft_slot = overlay_shedule::lookup_in_overlay_schedule(
        block_info.epoch_slot,
        genesis_delegs,
        decentralisation_param,
        active_slots_coeff,
    );

    Ok(())
}
