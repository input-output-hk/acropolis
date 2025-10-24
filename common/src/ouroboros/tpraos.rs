use crate::crypto::keyhash_224;
use crate::ouroboros::overlay_shedule::OBftSlot;
use crate::ouroboros::vrf_validation::{
    TPraosBadLeaderVrfProofError, TPraosBadNonceVrfProofError, VrfValidation, VrfValidationError,
    WrongGenesisLeaderVrfKeyError, WrongLeaderVrfKeyError,
};
use crate::ouroboros::{overlay_shedule, vrf};
use crate::protocol_params::Nonce;
use crate::rational_number::RationalNumber;
use crate::PoolId;
use crate::{genesis_values::GenesisDelegs, protocol_params::PraosParams, BlockInfo};
use anyhow::Result;
use pallas::ledger::primitives::VrfCert;
use pallas::ledger::traverse::MultiEraHeader;

pub fn validate_vrf_tpraos<'a>(
    block_info: &'a BlockInfo,
    header: &'a MultiEraHeader,
    praos_params: &'a PraosParams,
    epoch_nonce: &'a Nonce,
    decentralisation_param: RationalNumber,
    genesis_delegs: &'a GenesisDelegs,
) -> Result<Vec<VrfValidation<'a>>, VrfValidationError> {
    let active_slots_coeff = praos_params.active_slots_coeff;

    // first look up for overlay slot
    let obft_slot = overlay_shedule::lookup_in_overlay_schedule(
        block_info.epoch_slot,
        genesis_delegs,
        decentralisation_param,
        active_slots_coeff,
    )
    .map_err(|e| VrfValidationError::InvalidShelleyParams(e.to_string()))?;

    match obft_slot {
        None => {
            let Some(issuer_vkey) = header.issuer_vkey() else {
                return Ok(vec![Box::new(|| Err(VrfValidationError::MissingIssuerKey))]);
            };
            let pool_id: PoolId = keyhash_224(issuer_vkey);

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
        genesis_values::GenesisValues, protocol_params::NonceHash, BlockHash, BlockStatus, Era,
    };

    use super::*;

    #[test]
    fn test_4490511_block() {
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
        let vrf_validations = validate_vrf_tpraos(
            &block_info,
            &block_header,
            &praos_params,
            &epoch_nonce,
            decentralisation_param,
            &genesis_value.genesis_delegs,
        )
        .unwrap();
        let result = vrf_validations.iter().try_for_each(|assert| assert());
        assert!(result.is_ok());
    }
}
