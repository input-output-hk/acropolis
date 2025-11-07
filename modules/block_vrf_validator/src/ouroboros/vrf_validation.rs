use crate::ouroboros::vrf;
use acropolis_common::{
    crypto::keyhash_256,
    protocol_params::Nonce,
    rational_number::RationalNumber,
    validation::{
        BadVrfProofError, PraosBadVrfProofError, TPraosBadLeaderVrfProofError,
        TPraosBadNonceVrfProofError, VrfLeaderValueTooBigError, WrongGenesisLeaderVrfKeyError,
        WrongLeaderVrfKeyError,
    },
    GenesisDelegate, GenesisKeyhash, PoolId, Slot, VrfKeyHash,
};
use anyhow::Result;
use dashu_int::UBig;
use pallas::ledger::primitives::babbage::{derive_tagged_vrf_output, VrfDerivation};
use pallas_math::math::{ExpOrdering, FixedDecimal, FixedPrecision};

pub fn validate_genesis_leader_vrf_key(
    genesis_key: &GenesisKeyhash,
    genesis_deleg: &GenesisDelegate,
    vrf_vkey: &[u8],
) -> Result<(), WrongGenesisLeaderVrfKeyError> {
    let header_vrf_hash = VrfKeyHash::from(keyhash_256(vrf_vkey));
    let registered_vrf_hash = &genesis_deleg.vrf;
    if !registered_vrf_hash.eq(&header_vrf_hash) {
        return Err(WrongGenesisLeaderVrfKeyError {
            genesis_key: *genesis_key,
            registered_vrf_hash: *registered_vrf_hash,
            header_vrf_hash,
        });
    }
    Ok(())
}

pub fn validate_leader_vrf_key(
    pool_id: &PoolId,
    registered_vrf_key_hash: &VrfKeyHash,
    vrf_vkey: &[u8],
) -> Result<(), WrongLeaderVrfKeyError> {
    let header_vrf_key_hash = VrfKeyHash::from(keyhash_256(vrf_vkey));
    if !registered_vrf_key_hash.eq(&header_vrf_key_hash) {
        return Err(WrongLeaderVrfKeyError {
            pool_id: *pool_id,
            registered_vrf_key_hash: *registered_vrf_key_hash,
            header_vrf_key_hash,
        });
    }
    Ok(())
}

/// Validate the VRF output from the block and its corresponding hash.
/// in TPraos Protocol for Nonce
pub fn validate_tpraos_nonce_vrf_proof(
    absolute_slot: Slot,
    epoch_nonce: &Nonce,
    leader_public_key: &vrf::PublicKey,
    unsafe_vrf_proof_hash: &[u8],
    unsafe_vrf_proof: &[u8],
) -> Result<(), TPraosBadNonceVrfProofError> {
    // For nonce proof validation
    let seed_eta = Nonce::seed_eta();
    // https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/TPraos.hs#L365
    let rho_seed = vrf::VrfInput::mk_seed(absolute_slot, epoch_nonce, &seed_eta);

    // Verify the Nonce VRF proof
    validate_vrf_proof(
        &rho_seed,
        leader_public_key,
        unsafe_vrf_proof_hash,
        unsafe_vrf_proof,
    )
    .map_err(|e| TPraosBadNonceVrfProofError::BadVrfProof(absolute_slot, epoch_nonce.clone(), e))?;
    Ok(())
}

/// Validate the VRF output from the block and its corresponding hash.
/// in TPraos Protocol for Leader
pub fn validate_tpraos_leader_vrf_proof(
    absolute_slot: Slot,
    epoch_nonce: &Nonce,
    // Declared VRF Public Key from block header
    leader_public_key: &vrf::PublicKey,
    // Declared VRF Proof Hash from block header (sha512 hash)
    unsafe_vrf_proof_hash: &[u8],
    // Declared VRF Proof from block header (80 bytes)
    unsafe_vrf_proof: &[u8],
) -> Result<(), TPraosBadLeaderVrfProofError> {
    // For leader proof validation
    let seed_l = Nonce::seed_l();
    // https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/TPraos.hs#L366
    let y_seed = vrf::VrfInput::mk_seed(absolute_slot, epoch_nonce, &seed_l);

    // Verify the Leader VRF proof
    validate_vrf_proof(
        &y_seed,
        leader_public_key,
        unsafe_vrf_proof_hash,
        unsafe_vrf_proof,
    )
    .map_err(|e| {
        TPraosBadLeaderVrfProofError::BadVrfProof(absolute_slot, epoch_nonce.clone(), e)
    })?;
    Ok(())
}

/// Validate the VRF output from the block and its corresponding hash.
/// in Praos Protocol
pub fn validate_praos_vrf_proof(
    absolute_slot: Slot,
    epoch_nonce: &Nonce,
    leader_vrf_output: &[u8],
    // Declared VRF Public Key from block header
    leader_public_key: &vrf::PublicKey,
    // Declared VRF Proof Hash from block header (sha512 hash)
    unsafe_vrf_proof_hash: &[u8],
    // Declared VRF Proof from block header (80 bytes)
    unsafe_vrf_proof: &[u8],
) -> Result<(), PraosBadVrfProofError> {
    let input = vrf::VrfInput::mk_vrf_input(absolute_slot, epoch_nonce);

    // Verify the VRF proof
    validate_vrf_proof(
        &input,
        leader_public_key,
        unsafe_vrf_proof_hash,
        unsafe_vrf_proof,
    )
    .map_err(|e| PraosBadVrfProofError::BadVrfProof(absolute_slot, epoch_nonce.clone(), e))?;

    // The proof was valid. Make sure that the leader's output matches what was in the block
    let calculated_leader_vrf_output =
        derive_tagged_vrf_output(unsafe_vrf_proof_hash, VrfDerivation::Leader);
    if calculated_leader_vrf_output.as_slice() != leader_vrf_output {
        return Err(PraosBadVrfProofError::OutputMismatch {
            declared: leader_vrf_output.to_vec(),
            computed: calculated_leader_vrf_output,
        });
    }

    Ok(())
}

/// Reference
/// https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/TPraos.hs#L430
/// https://github.com/IntersectMBO/ouroboros-consensus/blob/e3c52b7c583bdb6708fac4fdaa8bf0b9588f5a88/ouroboros-consensus-protocol/src/ouroboros-consensus-protocol/Ouroboros/Consensus/Protocol/Praos.hs#L527
///
/// Check that the certified input natural is valid for being slot leader. This means we check that
/// p < 1 - (1 - f)^σ
/// **Variables**
/// `p` = `certNat` / `certNatMax`. (`certNat` is 64bytes for TPraos and 32bytes for Praos)
/// `σ` (sigma) = pool's relative stake (pools active stake / total active stake)
/// `f` = active slot coefficient (e.g., 0.05 = 5%)
/// let q = 1 - p and c = ln(1 - f)
/// then p < 1 - (1 - f)^σ => 1 / (1 - p) < exp(-σ * c) => 1 / q < exp(-σ * c)
/// Reference
/// https://github.com/IntersectMBO/cardano-ledger/blob/24ef1741c5e0109e4d73685a24d8e753e225656d/libs/cardano-protocol-tpraos/src/Cardano/Protocol/TPraos/BHeader.hs#L331
///
/// NOTE:
/// We are using Pallas Math Library
///
pub fn validate_vrf_leader_value(
    leader_vrf_output: &[u8],
    leader_relative_stake: &RationalNumber,
    active_slot_coeff: &RationalNumber,
) -> Result<(), VrfLeaderValueTooBigError> {
    let certified_leader_vrf = &FixedDecimal::from(leader_vrf_output);
    let output_size_bits = leader_vrf_output.len() * 8;
    let cert_nat_max = FixedDecimal::from(UBig::ONE << output_size_bits);
    let leader_relative_stake = FixedDecimal::from(UBig::from(*leader_relative_stake.numer()))
        / FixedDecimal::from(UBig::from(*leader_relative_stake.denom()));
    let active_slot_coeff = FixedDecimal::from(UBig::from(*active_slot_coeff.numer()))
        / FixedDecimal::from(UBig::from(*active_slot_coeff.denom()));

    let denominator = &cert_nat_max - certified_leader_vrf;
    let recip_q = &cert_nat_max / &denominator;
    let c = (&FixedDecimal::from(1u64) - &active_slot_coeff).ln();
    let x = -(leader_relative_stake * c);
    let ordering = x.exp_cmp(1000, 3, &recip_q);
    match ordering.estimation {
        ExpOrdering::LT => Ok(()),
        ExpOrdering::GT | ExpOrdering::UNKNOWN => {
            Err(VrfLeaderValueTooBigError::VrfLeaderValueTooBig)
        }
    }
}

/// Validate the VRF proof
pub fn validate_vrf_proof(
    vrf_input: &vrf::VrfInput,
    // Declared VRF Public Key from block header
    vrf_public_key: &vrf::PublicKey,
    // Declared VRF Proof Hash from block header (sha512 hash)
    unsafe_vrf_proof_hash: &[u8],
    // Declared VRF Proof from block header (80 bytes)
    unsafe_vrf_proof: &[u8],
) -> Result<(), BadVrfProofError> {
    let vrf_proof: [u8; vrf::Proof::SIZE] = unsafe_vrf_proof.try_into()?;
    let vrf_proof_hash: [u8; vrf::Proof::HASH_SIZE] = unsafe_vrf_proof_hash.try_into()?;
    let vrf_proof = vrf::Proof::try_from(&vrf_proof)
        .map_err(|e| BadVrfProofError::MalformedProof(e.to_string()))?;

    // Verify the VRF proof
    let proof_hash = vrf_proof.verify(vrf_public_key, vrf_input).map_err(|e| {
        BadVrfProofError::InvalidProof(
            e.to_string(),
            vrf_input.as_ref().to_vec(),
            vrf_public_key.as_ref().to_vec(),
        )
    })?;
    if !proof_hash.as_slice().eq(&vrf_proof_hash) {
        return Err(BadVrfProofError::ProofMismatch {
            declared: vrf_proof_hash.to_vec(),
            computed: proof_hash.to_vec(),
        });
    }

    Ok(())
}
