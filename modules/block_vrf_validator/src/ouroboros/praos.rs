use crate::ouroboros::{
    vrf,
    vrf_validation::{
        validate_leader_vrf_key, validate_praos_vrf_proof, validate_vrf_leader_value,
    },
};
use acropolis_common::{
    crypto::keyhash_224,
    protocol_params::{Nonce, PraosParams},
    rational_number::RationalNumber,
    validation::{VrfValidation, VrfValidationError},
    BlockInfo, PoolId, VrfKeyHash,
};
use anyhow::Result;
use pallas::ledger::{primitives::VrfCert, traverse::MultiEraHeader};
use std::collections::HashMap;

pub fn validate_vrf_praos<'a>(
    block_info: &'a BlockInfo,
    header: &'a MultiEraHeader,
    epoch_nonce: &'a Nonce,
    praos_params: &'a PraosParams,
    active_spos: &'a HashMap<PoolId, VrfKeyHash>,
    active_spdd: &'a HashMap<PoolId, u64>,
    total_active_stake: u64,
) -> Result<Vec<VrfValidation<'a>>, Box<VrfValidationError>> {
    let active_slots_coeff = praos_params.active_slots_coeff;

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
    let declared_vrf_key: &[u8; vrf::PublicKey::HASH_SIZE] = vrf_vkey
        .try_into()
        .map_err(|_| VrfValidationError::TryFromSlice("Invalid Vrf Key".to_string()))?;
    let vrf_cert =
        vrf_result(header).ok_or(VrfValidationError::Other("VRF Cert is not set".to_string()))?;

    // Regular TPraos rules apply
    Ok(vec![
        Box::new(move || {
            validate_leader_vrf_key(&pool_id, registered_vrf_key_hash, vrf_vkey)?;
            Ok(())
        }),
        Box::new(move || {
            validate_praos_vrf_proof(
                block_info.slot,
                epoch_nonce,
                &header.leader_vrf_output().map_err(|_| {
                    VrfValidationError::Other("Leader VRF Output is not set".to_string())
                })?[..],
                &vrf::PublicKey::from(declared_vrf_key),
                &vrf_cert.0.to_vec()[..],
                &vrf_cert.1.to_vec()[..],
            )?;
            Ok(())
        }),
        Box::new(move || {
            validate_vrf_leader_value(
                &header.leader_vrf_output().map_err(|_| {
                    VrfValidationError::Other("Leader VRF Output is not set".to_string())
                })?[..],
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

#[cfg(test)]
mod tests {
    use acropolis_common::{
        crypto::keyhash_256, protocol_params::NonceHash, serialization::Bech32Conversion,
        BlockHash, BlockStatus, Era,
    };

    use super::*;

    #[test]
    fn test_7854823_block() {
        let praos_params = PraosParams::mainnet();
        let epoch_nonce = Nonce::from(
            NonceHash::try_from(
                hex::decode("8dad163edf4607452fec9c5955d593fb598ca728bae162138f88da6667bba79b")
                    .unwrap()
                    .as_slice(),
            )
            .unwrap(),
        );

        let block_header_7854823: Vec<u8> =
            hex::decode(include_str!("./data/7854823.cbor")).unwrap();
        let block_info = BlockInfo {
            status: BlockStatus::Immutable,
            slot: 73614529,
            hash: BlockHash::try_from(
                hex::decode("4884996cff870563ffddab5d1255a82a58482ba9351536f5b72c882f883c8947")
                    .unwrap(),
            )
            .unwrap(),
            timestamp: 1665180820,
            number: 7854823,
            epoch: 368,
            epoch_slot: 1729,
            new_epoch: false,
            era: Era::Babbage,
        };
        let block_header =
            MultiEraHeader::decode(block_info.era as u8, None, &block_header_7854823).unwrap();
        let pool_id =
            PoolId::from_bech32("pool195gdnmj6smzuakm4etxsxw3fgh8asqc4awtcskpyfnkpcvh2v8t")
                .unwrap();
        let active_spos = HashMap::from([(
            pool_id,
            VrfKeyHash::from(keyhash_256(block_header.vrf_vkey().unwrap())),
        )]);
        let active_spdd = HashMap::from([(pool_id, 64590523391239)]);
        let result = validate_vrf_praos(
            &block_info,
            &block_header,
            &epoch_nonce,
            &praos_params,
            &active_spos,
            &active_spdd,
            25069171797357766,
        )
        .and_then(|vrf_validations| {
            vrf_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_ok());
    }
}
