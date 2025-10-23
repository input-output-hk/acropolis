use crate::ouroboros::{overlay_shedule, vrf};
use crate::PoolId;
use crate::{
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

/// Reference
/// https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/Praos.hs#L342
#[derive(Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum VrfValidationError {
    /// **Cause:** The Shelley protocol parameters used to validate the block,
    #[error("{0}")]
    InvalidShelleyParams(String),
    /// **Cause:** Block issuer's pool ID is not registered in current stake distribution
    #[error("Unknown Pool: {}", hex::encode(&pool_id))]
    UnknownPool { pool_id: PoolId },
    /// **Cause:** The VRF key hash in the block header doesn't match the VRF key
    /// registered with this stake pool in the ledger state
    #[error("{0}")]
    WrongLeaderVrfKey(#[from] WrongLeaderVrfKeyError),
    /// VRF nonce proof verification failed (TPraos rho - nonce proof)
    /// **Cause:** The (rho - nonce) VRF proof failed verification
    #[error("{0}")]
    TPraosBadNonceVrfProof(#[from] TPraosBadNonceVrfProofError),
    /// VRF leader proof verification failed (TPraos y - leader proof)
    /// **Cause:** The (y - leader) VRF proof failed verification
    #[error("{0}")]
    TPraosBadLeaderVrfProof(#[from] TPraosBadLeaderVrfProofError),
    /// VRF proof cryptographic verification failed (Praos single proof)
    /// **Cause:** The cryptographic VRF proof is invalid
    #[error("{0}")]
    PraosBadVrfProof(#[from] PraosBadVrfProofError),
    /// **Cause:** The VRF output is too large for this pool's stake.
    /// The pool lost the slot lottery in TPraos Protocol
    #[error("TPraos VRF Leader Value Too Big")]
    TPraosVrfLeaderValueTooBig,
    /// **Cause:** The VRF output is too large for this pool's stake.
    /// The pool lost the slot lottery in Praos Protocol
    #[error("Praos VRF Leader Value Too Big")]
    PraosVrfLeaderValueTooBig,
}

// ------------------------------------------------------------ WrongLeaderVrfKeyError

#[derive(Error, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[error(
    "Wrong Leader VRF Key: Pool ID={}, Registered VRF Hash={}, Header VRF Hash={}",
    hex::encode(&pool_id),
    hex::encode(&registered_vrf_hash),
    hex::encode(&header_vrf_hash),
)]
pub struct WrongLeaderVrfKeyError {
    pool_id: PoolId,
    registered_vrf_hash: KeyHash,
    header_vrf_hash: KeyHash,
}

impl WrongLeaderVrfKeyError {
    pub fn new(
        pool_id: &PoolId,
        registered_vrf_hash: &KeyHash,
        vrf_vkey: &[u8],
    ) -> Result<(), Self> {
        let header_vrf_hash = keyhash_256(vrf_vkey);
        if !registered_vrf_hash.eq(&header_vrf_hash) {
            return Err(Self {
                pool_id: pool_id.clone(),
                registered_vrf_hash: registered_vrf_hash.clone(),
                header_vrf_hash: header_vrf_hash,
            });
        }
        Ok(())
    }
}

// ------------------------------------------------------------ TPraosBadNonceVrfProofError

#[derive(Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum TPraosBadNonceVrfProofError {
    #[error("Bad Nonce VRF Proof: Slot={0}, Epoch Nonce={1}, Bad VRF Proof={2}")]
    BadVrfProof(Slot, Nonce, BadVrfProofError),
}

impl TPraosBadNonceVrfProofError {
    /// Validate the VRF output from the block and its corresponding hash.
    /// in TPraos Protocol for Nonce
    pub fn new(
        absolute_slot: Slot,
        epoch_nonce: &Nonce,
        // Declared VRF Public Key from block header
        leader_public_key: &vrf::PublicKey,
        // Declared VRF Proof Hash from block header (sha512 hash)
        unsafe_vrf_proof_hash: &[u8],
        // Declared VRF Proof from block header (80 bytes)
        unsafe_vrf_proof: &[u8],
    ) -> Result<(), Self> {
        // For nonce proof validation
        let seed_eta = Nonce::seed_eta();
        // https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/TPraos.hs#L365
        let rho_seed = vrf::VrfInput::mk_seed(absolute_slot, epoch_nonce, &seed_eta);

        // Verify the Nonce VRF proof
        BadVrfProofError::new(
            &rho_seed,
            leader_public_key,
            unsafe_vrf_proof_hash,
            unsafe_vrf_proof,
        )
        .map_err(|e| Self::BadVrfProof(absolute_slot, epoch_nonce.clone(), e))?;
        Ok(())
    }
}

// ------------------------------------------------------------ TPraosBadLeaderVrfProofError

#[derive(Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum TPraosBadLeaderVrfProofError {
    #[error("Bad Leader VRF Proof: Slot={0}, Epoch Nonce={1}, Bad VRF Proof={2}")]
    BadVrfProof(Slot, Nonce, BadVrfProofError),
}

impl TPraosBadLeaderVrfProofError {
    /// Validate the VRF output from the block and its corresponding hash.
    /// in TPraos Protocol for Leader
    pub fn new(
        absolute_slot: Slot,
        epoch_nonce: &Nonce,
        // Declared VRF Public Key from block header
        leader_public_key: &vrf::PublicKey,
        // Declared VRF Proof Hash from block header (sha512 hash)
        unsafe_vrf_proof_hash: &[u8],
        // Declared VRF Proof from block header (80 bytes)
        unsafe_vrf_proof: &[u8],
    ) -> Result<(), Self> {
        // For leader proof validation
        let seed_l = Nonce::seed_l();
        // https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/TPraos.hs#L366
        let y_seed = vrf::VrfInput::mk_seed(absolute_slot, epoch_nonce, &seed_l);

        // Verify the Leader VRF proof
        BadVrfProofError::new(
            &y_seed,
            leader_public_key,
            unsafe_vrf_proof_hash,
            unsafe_vrf_proof,
        )
        .map_err(|e| Self::BadVrfProof(absolute_slot, epoch_nonce.clone(), e))?;
        Ok(())
    }
}

// ------------------------------------------------------------ PraosBadVrfProofError

#[derive(Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum PraosBadVrfProofError {
    #[error("Bad VRF proof: Slot={0}, Epoch Nonce={1}, Bad VRF Proof={2}")]
    BadVrfProof(Slot, Nonce, BadVrfProofError),

    #[error(
        "Mismatch between the declared VRF output in block ({}) and the computed one ({}).",
        hex::encode(&declared),
        hex::encode(&computed),
    )]
    OutputMismatch {
        declared: Vec<u8>,
        computed: Vec<u8>,
    },
}

impl PartialEq for PraosBadVrfProofError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::BadVrfProof(l0, l1, l2), Self::BadVrfProof(r0, r1, r2)) => {
                l0 == r0 && l1 == r1 && l2 == r2
            }
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

impl PraosBadVrfProofError {
    /// Validate the VRF output from the block and its corresponding hash.
    /// in Praos Protocol
    pub fn new(
        absolute_slot: Slot,
        epoch_nonce: &Nonce,
        leader_vrf_output: &[u8],
        // Declared VRF Public Key from block header
        leader_public_key: &vrf::PublicKey,
        // Declared VRF Proof Hash from block header (sha512 hash)
        unsafe_vrf_proof_hash: &[u8],
        // Declared VRF Proof from block header (80 bytes)
        unsafe_vrf_proof: &[u8],
    ) -> Result<(), Self> {
        let input = vrf::VrfInput::mk_vrf_input(absolute_slot, epoch_nonce);

        // Verify the VRF proof
        BadVrfProofError::new(
            &input,
            leader_public_key,
            unsafe_vrf_proof_hash,
            unsafe_vrf_proof,
        )
        .map_err(|e| Self::BadVrfProof(absolute_slot, epoch_nonce.clone(), e))?;

        // The proof was valid. Make sure that the leader's output matches what was in the block
        let calculated_leader_vrf_output =
            derive_tagged_vrf_output(unsafe_vrf_proof_hash, VrfDerivation::Leader);
        if calculated_leader_vrf_output.as_slice() != leader_vrf_output {
            return Err(Self::OutputMismatch {
                declared: leader_vrf_output.to_vec(),
                computed: calculated_leader_vrf_output,
            });
        }

        Ok(())
    }
}

// ------------------------------------------------------------ BadVrfProofError

#[derive(Error, Debug, serde::Serialize, serde::Deserialize)]
pub enum BadVrfProofError {
    #[error("Malformed VRF proof: {0}")]
    MalformedProof(#[from] vrf::ProofFromBytesError),

    #[error("Invalid VRF proof: {0}")]
    /// (error, vrf_input_hash, vrf_public_key_hash)
    InvalidProof(vrf::ProofVerifyError, Vec<u8>, Vec<u8>),

    #[error("could not convert slice to array")]
    TryFromSliceError,

    #[error(
        "Mismatch between the declared VRF proof hash ({}) and the computed one ({}).",
        hex::encode(&declared),
        hex::encode(&computed),
    )]
    ProofMismatch {
        // this is Proof Hash (sha512 hash)
        declared: Vec<u8>,
        computed: Vec<u8>,
    },
}

impl From<TryFromSliceError> for BadVrfProofError {
    fn from(_: TryFromSliceError) -> Self {
        Self::TryFromSliceError
    }
}

impl PartialEq for BadVrfProofError {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::MalformedProof(l0), Self::MalformedProof(r0)) => l0 == r0,
            (Self::InvalidProof(l0, l1, l2), Self::InvalidProof(r0, r1, r2)) => {
                l0 == r0 && l1 == r1 && l2 == r2
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
            _ => false,
        }
    }
}

impl BadVrfProofError {
    /// Validate the VRF proof
    pub fn new(
        vrf_input: &vrf::VrfInput,
        // Declared VRF Public Key from block header
        vrf_public_key: &vrf::PublicKey,
        // Declared VRF Proof Hash from block header (sha512 hash)
        unsafe_vrf_proof_hash: &[u8],
        // Declared VRF Proof from block header (80 bytes)
        unsafe_vrf_proof: &[u8],
    ) -> Result<(), Self> {
        let vrf_proof: [u8; vrf::Proof::SIZE] = unsafe_vrf_proof.try_into()?;
        let vrf_proof_hash: [u8; vrf::Proof::HASH_SIZE] = unsafe_vrf_proof_hash.try_into()?;
        let vrf_proof = vrf::Proof::try_from(&vrf_proof)?;

        // Verify the VRF proof
        let proof_hash = vrf_proof.verify(vrf_public_key, vrf_input).map_err(|e| {
            Self::InvalidProof(
                e,
                vrf_input.as_ref().to_vec(),
                vrf_public_key.as_ref().to_vec(),
            )
        })?;
        if !proof_hash.as_slice().eq(&vrf_proof_hash) {
            return Err(Self::ProofMismatch {
                declared: vrf_proof_hash.to_vec(),
                computed: proof_hash.to_vec(),
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
    )
    .map_err(|e| VrfValidationError::InvalidShelleyParams(e.to_string()))?;

    Ok(())
}
