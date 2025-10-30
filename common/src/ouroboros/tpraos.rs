use std::collections::HashMap;

use crate::crypto::keyhash_224;
use crate::ouroboros::overlay_shedule::OBftSlot;
use crate::ouroboros::vrf_validation::{
    TPraosBadLeaderVrfProofError, TPraosBadNonceVrfProofError, VrfLeaderValueTooBigError,
    VrfValidation, VrfValidationError, WrongGenesisLeaderVrfKeyError, WrongLeaderVrfKeyError,
};
use crate::ouroboros::{overlay_shedule, vrf};
use crate::protocol_params::Nonce;
use crate::rational_number::RationalNumber;
use crate::{genesis_values::GenesisDelegs, protocol_params::PraosParams, BlockInfo};
use crate::{KeyHash, PoolId};
use anyhow::Result;
use pallas::ledger::primitives::VrfCert;
use pallas::ledger::traverse::MultiEraHeader;

pub fn validate_vrf_tpraos<'a>(
    block_info: &'a BlockInfo,
    header: &'a MultiEraHeader,
    epoch_nonce: &'a Nonce,
    genesis_delegs: &'a GenesisDelegs,
    praos_params: &'a PraosParams,
    active_spos: &'a HashMap<PoolId, KeyHash>,
    active_spdd: &'a HashMap<PoolId, u64>,
    total_active_stake: u64,
    decentralisation_param: RationalNumber,
) -> Result<Vec<VrfValidation<'a>>, VrfValidationError> {
    let active_slots_coeff = praos_params.active_slots_coeff;

    // first look up for overlay slot
    let obft_slot = overlay_shedule::lookup_in_overlay_schedule(
        block_info.epoch_slot,
        genesis_delegs,
        &decentralisation_param,
        &active_slots_coeff,
    )
    .map_err(|e| VrfValidationError::InvalidShelleyParams(e.to_string()))?;

    match obft_slot {
        None => {
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
            let nonce_vrf_cert =
                nonce_vrf_cert(header).ok_or(VrfValidationError::TPraosMissingNonceVrfCert)?;
            let leader_vrf_cert =
                leader_vrf_cert(header).ok_or(VrfValidationError::TPraosMissingLeaderVrfCert)?;

            // Regular TPraos rules apply
            Ok(vec![
                Box::new(move || {
                    WrongLeaderVrfKeyError::new(&pool_id, registered_vrf_key_hash, vrf_vkey)?;
                    Ok(())
                }),
                Box::new(move || {
                    TPraosBadNonceVrfProofError::new(
                        block_info.slot,
                        epoch_nonce,
                        &vrf::PublicKey::from(declared_vrf_key),
                        &nonce_vrf_cert.0.to_vec()[..],
                        &nonce_vrf_cert.1.to_vec()[..],
                    )?;
                    Ok(())
                }),
                Box::new(move || {
                    TPraosBadLeaderVrfProofError::new(
                        block_info.slot,
                        epoch_nonce,
                        &vrf::PublicKey::from(declared_vrf_key),
                        &leader_vrf_cert.0.to_vec()[..],
                        &leader_vrf_cert.1.to_vec()[..],
                    )?;
                    Ok(())
                }),
                Box::new(move || {
                    VrfLeaderValueTooBigError::new(
                        &leader_vrf_cert.0.to_vec()[..],
                        &relative_stake,
                        &active_slots_coeff,
                    )?;
                    Ok(())
                }),
            ])
        }
        Some(OBftSlot::ActiveSlot(genesis_key, gen_deleg)) => {
            // The given genesis key has authority to produce a block in this
            // slot. Check whether we're its delegate.
            let Some(vrf_vkey) = header.vrf_vkey() else {
                return Ok(vec![Box::new(|| Err(VrfValidationError::MissingVrfVkey))]);
            };
            let declared_vrf_key: &[u8; vrf::PublicKey::HASH_SIZE] = vrf_vkey
                .try_into()
                .map_err(|_| VrfValidationError::TryFromSlice("Invalid Vrf Key".to_string()))?;
            let nonce_vrf_cert =
                nonce_vrf_cert(header).ok_or(VrfValidationError::TPraosMissingNonceVrfCert)?;
            let leader_vrf_cert =
                leader_vrf_cert(header).ok_or(VrfValidationError::TPraosMissingLeaderVrfCert)?;

            Ok(vec![
                Box::new(move || {
                    WrongGenesisLeaderVrfKeyError::new(&genesis_key, &gen_deleg, vrf_vkey)?;
                    Ok(())
                }),
                Box::new(move || {
                    TPraosBadNonceVrfProofError::new(
                        block_info.slot,
                        epoch_nonce,
                        &vrf::PublicKey::from(declared_vrf_key),
                        &nonce_vrf_cert.0.to_vec()[..],
                        &nonce_vrf_cert.1.to_vec()[..],
                    )?;
                    Ok(())
                }),
                Box::new(move || {
                    TPraosBadLeaderVrfProofError::new(
                        block_info.slot,
                        epoch_nonce,
                        &vrf::PublicKey::from(declared_vrf_key),
                        &leader_vrf_cert.0.to_vec()[..],
                        &leader_vrf_cert.1.to_vec()[..],
                    )?;
                    Ok(())
                }),
            ])
        }
        Some(OBftSlot::NonActiveSlot) => {
            // This is a non-active slot; nobody may produce a block
            Ok(vec![])
        }
    }
}

fn nonce_vrf_cert<'a>(header: &'a MultiEraHeader) -> Option<&'a VrfCert> {
    match header {
        MultiEraHeader::ShelleyCompatible(x) => Some(&x.header_body.nonce_vrf),
        _ => None,
    }
}

fn leader_vrf_cert<'a>(header: &'a MultiEraHeader) -> Option<&'a VrfCert> {
    match header {
        MultiEraHeader::ShelleyCompatible(x) => Some(&x.header_body.leader_vrf),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        crypto::keyhash_256, genesis_values::GenesisValues, protocol_params::NonceHash,
        serialization::Bech32WithHrp, BlockHash, BlockStatus, Era,
    };

    use super::*;

    #[test]
    fn test_4490511_block_produced_by_genesis_key() {
        let genesis_value = GenesisValues::mainnet();
        let praos_params = PraosParams::mainnet();
        let epoch_nonce = Nonce::from(
            NonceHash::try_from(
                hex::decode("1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let decentralisation_param = RationalNumber::from(1);

        let block_header_4490511: Vec<u8> =
            hex::decode(include_str!("./data/4490511.cbor")).unwrap();
        let block_header = MultiEraHeader::decode(1, None, &block_header_4490511).unwrap();
        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 4492800,
            hash: BlockHash::try_from(
                hex::decode("aa83acbf5904c0edfe4d79b3689d3d00fcfc553cf360fd2229b98d464c28e9de")
                    .unwrap(),
            )
            .unwrap(),
            timestamp: 1596059091,
            number: 4490511,
            epoch: 208,
            epoch_slot: 0,
            new_epoch: true,
            era: Era::Shelley,
        };
        let active_spos = HashMap::new();
        let active_spdd = HashMap::new();
        let vrf_validations = validate_vrf_tpraos(
            &block_info,
            &block_header,
            &epoch_nonce,
            &genesis_value.genesis_delegs,
            &praos_params,
            &active_spos,
            &active_spdd,
            1,
            decentralisation_param,
        )
        .unwrap();
        let result: Result<(), VrfValidationError> =
            vrf_validations.iter().try_for_each(|assert| assert());
        assert!(result.is_ok());
    }

    #[test]
    fn test_4556956_block() {
        let genesis_value = GenesisValues::mainnet();
        let praos_params = PraosParams::mainnet();
        let epoch_nonce = Nonce::from(
            NonceHash::try_from(
                hex::decode("3fac34ac3d7d1ac6c976ba68b1509b1ee3aafdbf6de96e10789e488e13e16bd7")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let decentralisation_param = RationalNumber::new(9, 10);

        let block_header_4556956: Vec<u8> =
            hex::decode(include_str!("./data/4556956.cbor")).unwrap();
        let block_header = MultiEraHeader::decode(1, None, &block_header_4556956).unwrap();
        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 5824849,
            hash: BlockHash::try_from(
                hex::decode("1038b2c76a23ea7d89cbd84d7744c97560eb3412661beed6959d748e24ff8229")
                    .unwrap(),
            )
            .unwrap(),
            timestamp: 1597391140,
            number: 4556956,
            epoch: 211,
            epoch_slot: 36049,
            new_epoch: false,
            era: Era::Shelley,
        };
        let pool_id = Vec::<u8>::from_bech32_with_hrp(
            "pool1pu5jlj4q9w9jlxeu370a3c9myx47md5j5m2str0naunn2q3lkdy",
            "pool",
        )
        .unwrap();
        let active_spos: HashMap<PoolId, KeyHash> = HashMap::from([(
            pool_id.clone(),
            keyhash_256(block_header.vrf_vkey().unwrap()),
        )]);
        let active_spdd = HashMap::from([(pool_id.clone(), 75284250207839)]);
        let vrf_validations = validate_vrf_tpraos(
            &block_info,
            &block_header,
            &epoch_nonce,
            &genesis_value.genesis_delegs,
            &praos_params,
            &active_spos,
            &active_spdd,
            10177811974823000,
            decentralisation_param,
        )
        .unwrap();
        let result: Result<(), VrfValidationError> =
            vrf_validations.iter().try_for_each(|assert| assert());
        assert!(result.is_ok());
    }

    #[test]
    fn test_4576496_block() {
        let genesis_value = GenesisValues::mainnet();
        let praos_params = PraosParams::mainnet();
        let epoch_nonce = Nonce::from(
            NonceHash::try_from(
                hex::decode("3fac34ac3d7d1ac6c976ba68b1509b1ee3aafdbf6de96e10789e488e13e16bd7")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let decentralisation_param = RationalNumber::new(9, 10);

        let block_header_4576496: Vec<u8> =
            hex::decode(include_str!("./data/4576496.cbor")).unwrap();
        let block_header = MultiEraHeader::decode(1, None, &block_header_4576496).unwrap();
        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 6220749,
            hash: BlockHash::try_from(
                hex::decode("d78e446b6540612e161ebdda32ee1715ef0f9fc68e890c7e3aae167b0354f998")
                    .unwrap(),
            )
            .unwrap(),
            timestamp: 1597787040,
            number: 4576496,
            epoch: 211,
            epoch_slot: 431949,
            new_epoch: false,
            era: Era::Shelley,
        };
        let pool_id = Vec::<u8>::from_bech32_with_hrp(
            "pool1pu5jlj4q9w9jlxeu370a3c9myx47md5j5m2str0naunn2q3lkdy",
            "pool",
        )
        .unwrap();
        let active_spos: HashMap<PoolId, KeyHash> = HashMap::from([(
            pool_id.clone(),
            keyhash_256(block_header.vrf_vkey().unwrap()),
        )]);
        let active_spdd = HashMap::from([(pool_id.clone(), 75284250207839)]);
        let vrf_validations = validate_vrf_tpraos(
            &block_info,
            &block_header,
            &epoch_nonce,
            &genesis_value.genesis_delegs,
            &praos_params,
            &active_spos,
            &active_spdd,
            10177811974823000,
            decentralisation_param,
        )
        .unwrap();
        let result: Result<(), VrfValidationError> =
            vrf_validations.iter().try_for_each(|assert| assert());
        assert!(result.is_ok());
    }
}
