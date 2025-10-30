use std::collections::HashMap;

use crate::crypto::keyhash_224;
use crate::ouroboros::vrf;
use crate::ouroboros::vrf_validation::{
    PraosBadVrfProofError, VrfLeaderValueTooBigError, VrfValidation, VrfValidationError,
    WrongLeaderVrfKeyError,
};
use crate::protocol_params::Nonce;
use crate::rational_number::RationalNumber;
use crate::{protocol_params::PraosParams, BlockInfo};
use crate::{KeyHash, PoolId};
use anyhow::Result;
use pallas::ledger::primitives::VrfCert;
use pallas::ledger::traverse::MultiEraHeader;

pub fn validate_vrf_praos<'a>(
    block_info: &'a BlockInfo,
    header: &'a MultiEraHeader,
    epoch_nonce: &'a Nonce,
    praos_params: &'a PraosParams,
    active_spos: &'a HashMap<PoolId, KeyHash>,
    active_spdd: &'a HashMap<PoolId, u64>,
    total_active_stake: u64,
) -> Result<Vec<VrfValidation<'a>>, VrfValidationError> {
    let active_slots_coeff = praos_params.active_slots_coeff;

    let Some(issuer_vkey) = header.issuer_vkey() else {
        return Ok(vec![Box::new(|| Err(VrfValidationError::MissingIssuerKey))]);
    };
    let pool_id: PoolId = keyhash_224(issuer_vkey);
    let registered_vrf_key_hash =
        active_spos.get(&pool_id).ok_or(VrfValidationError::UnknownPool {
            pool_id: pool_id.clone(),
        })?;

    let pool_stake = active_spdd.get(&pool_id).unwrap_or(&0);
    let relative_stake = RationalNumber::new(*pool_stake, total_active_stake);

    let Some(vrf_vkey) = header.vrf_vkey() else {
        return Ok(vec![Box::new(|| Err(VrfValidationError::MissingVrfVkey))]);
    };
    let declared_vrf_key: &[u8; vrf::PublicKey::HASH_SIZE] = vrf_vkey
        .try_into()
        .map_err(|_| VrfValidationError::TryFromSlice("Invalid Vrf Key".to_string()))?;
    let vrf_cert = vrf_result(header).ok_or(VrfValidationError::PraosMissingVrfCert)?;

    // Regular TPraos rules apply
    Ok(vec![
        Box::new(move || {
            WrongLeaderVrfKeyError::new(&pool_id, registered_vrf_key_hash, vrf_vkey)?;
            Ok(())
        }),
        Box::new(move || {
            PraosBadVrfProofError::new(
                block_info.slot,
                epoch_nonce,
                &header
                    .leader_vrf_output()
                    .map_err(|_| VrfValidationError::PraosMissingLeaderVrfOutput)?[..],
                &vrf::PublicKey::from(declared_vrf_key),
                &vrf_cert.0.to_vec()[..],
                &vrf_cert.1.to_vec()[..],
            )?;
            Ok(())
        }),
        Box::new(move || {
            VrfLeaderValueTooBigError::new(
                &header
                    .leader_vrf_output()
                    .map_err(|_| VrfValidationError::PraosMissingLeaderVrfOutput)?[..],
                &relative_stake,
                &active_slots_coeff,
            )?;
            Ok(())
        }),
    ])
}

fn vrf_result<'a>(header: &'a MultiEraHeader) -> Option<&'a VrfCert> {
    match header {
        MultiEraHeader::BabbageCompatible(x) => Some(&x.header_body.vrf_result),
        _ => None,
    }
}
