use crate::ouroboros::{
    overlay_schedule::{self, OBftSlot},
    vrf,
    vrf_validation::{
        validate_genesis_leader_vrf_key, validate_leader_vrf_key, validate_tpraos_leader_vrf_proof,
        validate_tpraos_nonce_vrf_proof, validate_vrf_leader_value,
    },
};
use acropolis_common::{
    crypto::keyhash_224,
    protocol_params::Nonce,
    rational_number::RationalNumber,
    validation::{VrfValidation, VrfValidationError},
    BlockInfo, GenesisDelegates, PoolId, VrfKeyHash,
};
use anyhow::Result;
use pallas::ledger::{primitives::VrfCert, traverse::MultiEraHeader};
use std::collections::HashMap;

#[allow(clippy::too_many_arguments)]
pub fn validate_vrf_tpraos<'a>(
    block_info: &'a BlockInfo,
    header: &'a MultiEraHeader,
    epoch_nonce: &'a Nonce,
    genesis_delegs: &'a GenesisDelegates,
    active_slots_coeff: RationalNumber,
    decentralisation_param: RationalNumber,
    active_spos: &'a HashMap<PoolId, VrfKeyHash>,
    active_spdd: &'a HashMap<PoolId, u64>,
    total_active_stake: u64,
) -> Result<Vec<VrfValidation<'a>>, Box<VrfValidationError>> {
    // first look up for overlay slot
    let obft_slot = overlay_schedule::lookup_in_overlay_schedule(
        block_info.epoch_slot,
        genesis_delegs,
        &decentralisation_param,
        &active_slots_coeff,
    )
    .map_err(|e| VrfValidationError::Other(e.to_string()))?;

    match obft_slot {
        None => {
            let Some(issuer_vkey) = header.issuer_vkey() else {
                return Ok(vec![Box::new(|| {
                    Err(VrfValidationError::Other(
                        "Issuer Key is not set".to_string(),
                    ))
                })]);
            };
            let pool_id = PoolId::from(keyhash_224(issuer_vkey));
            let registered_vrf_key_hash =
                active_spos.get(&pool_id).ok_or(VrfValidationError::UnknownPool { pool_id })?;

            let pool_stake = active_spdd.get(&pool_id).unwrap_or(&0);
            let relative_stake = RationalNumber::new(*pool_stake, total_active_stake);

            let Some(vrf_vkey) = header.vrf_vkey() else {
                return Ok(vec![Box::new(|| {
                    Err(VrfValidationError::Other("VRF Key is not set".to_string()))
                })]);
            };
            let declared_vrf_key: &[u8; vrf::PublicKey::SIZE] = vrf_vkey
                .try_into()
                .map_err(|_| VrfValidationError::TryFromSlice("Invalid Vrf Key".to_string()))?;
            let nonce_vrf_cert = nonce_vrf_cert(header).ok_or(VrfValidationError::Other(
                "Nonce VRF Cert is not set".to_string(),
            ))?;
            let leader_vrf_cert = leader_vrf_cert(header).ok_or(VrfValidationError::Other(
                "Leader VRF Cert is not set".to_string(),
            ))?;

            // Regular TPraos rules apply
            Ok(vec![
                Box::new(move || {
                    validate_leader_vrf_key(&pool_id, registered_vrf_key_hash, vrf_vkey)?;
                    Ok(())
                }),
                Box::new(move || {
                    validate_tpraos_nonce_vrf_proof(
                        block_info.slot,
                        epoch_nonce,
                        &vrf::PublicKey::from(declared_vrf_key),
                        &nonce_vrf_cert.0.to_vec()[..],
                        &nonce_vrf_cert.1.to_vec()[..],
                    )?;
                    Ok(())
                }),
                Box::new(move || {
                    validate_tpraos_leader_vrf_proof(
                        block_info.slot,
                        epoch_nonce,
                        &vrf::PublicKey::from(declared_vrf_key),
                        &leader_vrf_cert.0.to_vec()[..],
                        &leader_vrf_cert.1.to_vec()[..],
                    )?;
                    Ok(())
                }),
                Box::new(move || {
                    validate_vrf_leader_value(
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
                return Ok(vec![Box::new(|| {
                    Err(VrfValidationError::Other("VRF Key is not set".to_string()))
                })]);
            };
            let declared_vrf_key: &[u8; vrf::PublicKey::SIZE] = vrf_vkey
                .try_into()
                .map_err(|_| VrfValidationError::TryFromSlice("Invalid Vrf Key".to_string()))?;
            let nonce_vrf_cert = nonce_vrf_cert(header).ok_or(VrfValidationError::Other(
                "Nonce VRF Cert is not set".to_string(),
            ))?;
            let leader_vrf_cert = leader_vrf_cert(header).ok_or(VrfValidationError::Other(
                "Leader VRF Cert is not set".to_string(),
            ))?;

            Ok(vec![
                Box::new(move || {
                    validate_genesis_leader_vrf_key(&genesis_key, &gen_deleg, vrf_vkey)?;
                    Ok(())
                }),
                Box::new(move || {
                    validate_tpraos_nonce_vrf_proof(
                        block_info.slot,
                        epoch_nonce,
                        &vrf::PublicKey::from(declared_vrf_key),
                        &nonce_vrf_cert.0.to_vec()[..],
                        &nonce_vrf_cert.1.to_vec()[..],
                    )?;
                    Ok(())
                }),
                Box::new(move || {
                    validate_tpraos_leader_vrf_proof(
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
            Ok(vec![Box::new(|| {
                Err(VrfValidationError::NotActiveSlotInOverlaySchedule {
                    slot: block_info.slot,
                })
            })])
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
    use acropolis_common::{
        crypto::keyhash_256,
        genesis_values::GenesisValues,
        protocol_params::NonceHash,
        serialization::Bech32Conversion,
        validation::{VrfLeaderValueTooBigError, WrongLeaderVrfKeyError},
        BlockHash, BlockIntent, BlockStatus, Era,
    };

    use super::*;

    #[test]
    fn test_4490511_block_produced_by_genesis_key() {
        let genesis_value = GenesisValues::mainnet();
        let epoch_nonce = Nonce::from(
            NonceHash::try_from(
                hex::decode("1a3be38bcbb7911969283716ad7aa550250226b76a61fc51cc9a9a35d9276d81")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let active_slots_coeff = RationalNumber::new(1, 20);
        let decentralisation_param = RationalNumber::from(1);

        let block_header_4490511: Vec<u8> =
            hex::decode(include_str!("./data/4490511.cbor")).unwrap();
        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            intent: BlockIntent::Validate,
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
        let block_header =
            MultiEraHeader::decode(block_info.era as u8, None, &block_header_4490511).unwrap();
        let active_spos = HashMap::new();
        let active_spdd = HashMap::new();
        let result = validate_vrf_tpraos(
            &block_info,
            &block_header,
            &epoch_nonce,
            &genesis_value.genesis_delegs,
            active_slots_coeff,
            decentralisation_param,
            &active_spos,
            &active_spdd,
            1,
        )
        .and_then(|vrf_validations| {
            vrf_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_4556956_block() {
        let genesis_value = GenesisValues::mainnet();
        let epoch_nonce = Nonce::from(
            NonceHash::try_from(
                hex::decode("3fac34ac3d7d1ac6c976ba68b1509b1ee3aafdbf6de96e10789e488e13e16bd7")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let active_slots_coeff = RationalNumber::new(1, 20);
        let decentralisation_param = RationalNumber::new(9, 10);

        let block_header_4556956: Vec<u8> =
            hex::decode(include_str!("./data/4556956.cbor")).unwrap();
        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            intent: BlockIntent::Apply,
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
        let block_header =
            MultiEraHeader::decode(block_info.era as u8, None, &block_header_4556956).unwrap();
        let pool_id =
            PoolId::from_bech32("pool1pu5jlj4q9w9jlxeu370a3c9myx47md5j5m2str0naunn2q3lkdy")
                .unwrap();
        let active_spos: HashMap<PoolId, VrfKeyHash> = HashMap::from([(
            pool_id,
            VrfKeyHash::from(keyhash_256(block_header.vrf_vkey().unwrap())),
        )]);
        let active_spdd = HashMap::from([(pool_id, 75284250207839)]);
        let result = validate_vrf_tpraos(
            &block_info,
            &block_header,
            &epoch_nonce,
            &genesis_value.genesis_delegs,
            active_slots_coeff,
            decentralisation_param,
            &active_spos,
            &active_spdd,
            10177811974823000,
        )
        .and_then(|vrf_validations| {
            vrf_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_4576496_block() {
        let genesis_value = GenesisValues::mainnet();
        let epoch_nonce = Nonce::from(
            NonceHash::try_from(
                hex::decode("3fac34ac3d7d1ac6c976ba68b1509b1ee3aafdbf6de96e10789e488e13e16bd7")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let active_slots_coeff = RationalNumber::new(1, 20);
        let decentralisation_param = RationalNumber::new(9, 10);

        let block_header_4576496: Vec<u8> =
            hex::decode(include_str!("./data/4576496.cbor")).unwrap();
        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            intent: BlockIntent::Apply,
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
        let block_header =
            MultiEraHeader::decode(block_info.era as u8, None, &block_header_4576496).unwrap();
        let pool_id =
            PoolId::from_bech32("pool1pu5jlj4q9w9jlxeu370a3c9myx47md5j5m2str0naunn2q3lkdy")
                .unwrap();
        let active_spos: HashMap<PoolId, VrfKeyHash> = HashMap::from([(
            pool_id,
            VrfKeyHash::from(keyhash_256(block_header.vrf_vkey().unwrap())),
        )]);
        let active_spdd = HashMap::from([(pool_id, 75284250207839)]);
        let result = validate_vrf_tpraos(
            &block_info,
            &block_header,
            &epoch_nonce,
            &genesis_value.genesis_delegs,
            active_slots_coeff,
            decentralisation_param,
            &active_spos,
            &active_spdd,
            10177811974823000,
        )
        .and_then(|vrf_validations| {
            vrf_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_4576496_block_as_unknown_pool() {
        let genesis_value = GenesisValues::mainnet();
        let epoch_nonce = Nonce::from(
            NonceHash::try_from(
                hex::decode("3fac34ac3d7d1ac6c976ba68b1509b1ee3aafdbf6de96e10789e488e13e16bd7")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let active_slots_coeff = RationalNumber::new(1, 20);
        let decentralisation_param = RationalNumber::new(9, 10);

        let block_header_4576496: Vec<u8> =
            hex::decode(include_str!("./data/4576496.cbor")).unwrap();
        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            intent: BlockIntent::Apply,
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
        let block_header =
            MultiEraHeader::decode(block_info.era as u8, None, &block_header_4576496).unwrap();
        let pool_id =
            PoolId::from_bech32("pool1pu5jlj4q9w9jlxeu370a3c9myx47md5j5m2str0naunn2q3lkdy")
                .unwrap();
        let active_spos: HashMap<PoolId, VrfKeyHash> = HashMap::from([]);
        let active_spdd = HashMap::from([]);
        let result = validate_vrf_tpraos(
            &block_info,
            &block_header,
            &epoch_nonce,
            &genesis_value.genesis_delegs,
            active_slots_coeff,
            decentralisation_param,
            &active_spos,
            &active_spdd,
            10177811974823000,
        )
        .and_then(|vrf_validations| {
            vrf_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Box::new(VrfValidationError::UnknownPool { pool_id })
        );
    }

    #[test]
    fn test_4576496_block_as_wrong_leader_vrf_key() {
        let genesis_value = GenesisValues::mainnet();
        let epoch_nonce = Nonce::from(
            NonceHash::try_from(
                hex::decode("3fac34ac3d7d1ac6c976ba68b1509b1ee3aafdbf6de96e10789e488e13e16bd7")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let active_slots_coeff = RationalNumber::new(1, 20);
        let decentralisation_param = RationalNumber::new(9, 10);

        let block_header_4576496: Vec<u8> =
            hex::decode(include_str!("./data/4576496.cbor")).unwrap();
        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            intent: BlockIntent::Apply,
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
        let block_header =
            MultiEraHeader::decode(block_info.era as u8, None, &block_header_4576496).unwrap();
        let pool_id =
            PoolId::from_bech32("pool1pu5jlj4q9w9jlxeu370a3c9myx47md5j5m2str0naunn2q3lkdy")
                .unwrap();
        let active_spos: HashMap<PoolId, VrfKeyHash> =
            HashMap::from([(pool_id, VrfKeyHash::from(keyhash_256(&[0; 64])))]);
        let active_spdd = HashMap::from([(pool_id, 75284250207839)]);
        let result = validate_vrf_tpraos(
            &block_info,
            &block_header,
            &epoch_nonce,
            &genesis_value.genesis_delegs,
            active_slots_coeff,
            decentralisation_param,
            &active_spos,
            &active_spdd,
            10177811974823000,
        )
        .and_then(|vrf_validations| {
            vrf_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Box::new(VrfValidationError::WrongLeaderVrfKey(
                WrongLeaderVrfKeyError {
                    pool_id,
                    registered_vrf_key_hash: VrfKeyHash::from(keyhash_256(&[0; 64])),
                    header_vrf_key_hash: VrfKeyHash::from(keyhash_256(
                        block_header.vrf_vkey().unwrap()
                    )),
                }
            ))
        );
    }

    #[test]
    fn test_4576496_block_with_small_active_stake() {
        let genesis_value = GenesisValues::mainnet();
        let epoch_nonce = Nonce::from(
            NonceHash::try_from(
                hex::decode("3fac34ac3d7d1ac6c976ba68b1509b1ee3aafdbf6de96e10789e488e13e16bd7")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );
        let active_slots_coeff = RationalNumber::new(1, 20);
        let decentralisation_param = RationalNumber::new(9, 10);

        let block_header_4576496: Vec<u8> =
            hex::decode(include_str!("./data/4576496.cbor")).unwrap();
        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            intent: BlockIntent::Apply,
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
        let block_header =
            MultiEraHeader::decode(block_info.era as u8, None, &block_header_4576496).unwrap();
        let pool_id =
            PoolId::from_bech32("pool1pu5jlj4q9w9jlxeu370a3c9myx47md5j5m2str0naunn2q3lkdy")
                .unwrap();
        let active_spos: HashMap<PoolId, VrfKeyHash> = HashMap::from([(
            pool_id,
            VrfKeyHash::from(keyhash_256(block_header.vrf_vkey().unwrap())),
        )]);
        // small active stake (correct one is 75284250207839)
        let active_spdd = HashMap::from([(pool_id, 75284250207)]);
        let result = validate_vrf_tpraos(
            &block_info,
            &block_header,
            &epoch_nonce,
            &genesis_value.genesis_delegs,
            active_slots_coeff,
            decentralisation_param,
            &active_spos,
            &active_spdd,
            10177811974823000,
        )
        .and_then(|vrf_validations| {
            vrf_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Box::new(VrfValidationError::VrfLeaderValueTooBig(
                VrfLeaderValueTooBigError::VrfLeaderValueTooBig
            ))
        );
    }
}
