use acropolis_common::{
    crypto::keyhash_224,
    validation::{
        KesSignatureError, KesValidation, KesValidationError, OperationalCertificateError,
    },
    GenesisDelegates, PoolId,
};
use imbl::HashMap;
use pallas::{crypto::key::ed25519, ledger::traverse::MultiEraHeader};

use crate::ouroboros::{kes, praos, tpraos};

#[derive(Copy, Clone)]
pub struct OperationalCertificate<'a> {
    pub operational_cert_hot_vkey: &'a [u8],
    pub operational_cert_sequence_number: u64,
    pub operational_cert_kes_period: u64,
    pub operational_cert_sigma: &'a [u8],
}

pub fn validate_kes_signature(
    slot_kes_period: u64,
    opcert_kes_period: u64,
    header_body: &[u8],
    public_key: &kes::PublicKey,
    signature: &kes::Signature,
    max_kes_evolutions: u64,
) -> Result<(), KesSignatureError> {
    if opcert_kes_period > slot_kes_period {
        return Err(KesSignatureError::KesBeforeStartOcert {
            ocert_start_period: opcert_kes_period,
            current_period: slot_kes_period,
        });
    }

    if slot_kes_period >= opcert_kes_period + max_kes_evolutions {
        return Err(KesSignatureError::KesAfterEndOcert {
            current_period: slot_kes_period,
            ocert_start_period: opcert_kes_period,
            max_kes_evolutions,
        });
    }

    let kes_period = (slot_kes_period - opcert_kes_period) as u32;

    signature.verify(kes_period, public_key, header_body).map_err(|error| {
        KesSignatureError::InvalidKesSignatureOcert {
            current_period: slot_kes_period,
            ocert_start_period: opcert_kes_period,
            reason: error.to_string(),
        }
    })?;

    Ok(())
}

pub fn validate_operational_certificate<'a>(
    certificate: OperationalCertificate<'a>,
    issuer: &ed25519::PublicKey,
    latest_sequence_number: u64,
    is_praos: bool,
) -> Result<(), OperationalCertificateError> {
    // Verify the Operational Certificate signature
    let signature =
        ed25519::Signature::try_from(certificate.operational_cert_sigma).map_err(|error| {
            OperationalCertificateError::MalformedSignatureOcert {
                reason: error.to_string(),
            }
        })?;

    let declared_sequence_number = certificate.operational_cert_sequence_number;

    // Check the sequence number of the operational certificate. It should either be the same
    // as the latest known sequence number for the issuer or one greater.
    if declared_sequence_number < latest_sequence_number {
        return Err(OperationalCertificateError::CounterTooSmallOcert {
            latest_counter: latest_sequence_number,
            declared_counter: declared_sequence_number,
        });
    }

    // this is only for praos protocol
    if is_praos && (declared_sequence_number - latest_sequence_number) > 1 {
        return Err(OperationalCertificateError::CounterOverIncrementedOcert {
            latest_counter: latest_sequence_number,
            declared_counter: declared_sequence_number,
        });
    }

    // The opcert message is a concatenation of the KES vkey, the sequence number, and the kes period
    let mut message = Vec::new();
    message.extend_from_slice(certificate.operational_cert_hot_vkey);
    message.extend_from_slice(&certificate.operational_cert_sequence_number.to_be_bytes());
    message.extend_from_slice(&certificate.operational_cert_kes_period.to_be_bytes());
    if !issuer.verify(&message, &signature) {
        return Err(OperationalCertificateError::InvalidSignatureOcert {
            issuer: issuer.as_ref().to_vec(),
        });
    }

    Ok(())
}

pub fn validate_block_kes<'a>(
    header: &'a MultiEraHeader,
    ocert_counters: &'a HashMap<PoolId, u64>,
    active_spos: &'a [PoolId],
    genesis_delegs: &'a GenesisDelegates,
    slots_per_kes_period: u64,
    max_kes_evolutions: u64,
) -> Result<Vec<KesValidation<'a>>, Box<KesValidationError>> {
    let is_praos = matches!(header, MultiEraHeader::BabbageCompatible(_));

    let issuer_vkey = header.issuer_vkey().ok_or(Box::new(KesValidationError::Other(
        "Issuer Key is not set".to_string(),
    )))?;
    let issuer = ed25519::PublicKey::from(
        <[u8; ed25519::PublicKey::SIZE]>::try_from(issuer_vkey)
            .map_err(|_| Box::new(KesValidationError::Other("Invalid issuer key".to_string())))?,
    );
    let pool_id = PoolId::from(keyhash_224(issuer_vkey));

    let slot_kes_period = header.slot() / slots_per_kes_period;
    let cert = operational_cert(header).ok_or(Box::new(KesValidationError::Other(
        "Operational certificate is not set".to_string(),
    )))?;
    let body_sig = body_signature(header).ok_or(Box::new(KesValidationError::Other(
        "Body signature is not set".to_string(),
    )))?;
    let raw_header_body = header.header_body_cbor().ok_or(Box::new(KesValidationError::Other(
        "Header body is not set".to_string(),
    )))?;

    let latest_sequence_number = if is_praos {
        praos::latest_issue_no_praos(ocert_counters, active_spos, &pool_id)
    } else {
        tpraos::latest_issue_no_tpraos(ocert_counters, active_spos, genesis_delegs, &pool_id)
    }
    .ok_or(Box::new(KesValidationError::NoOCertCounter { pool_id }))?;

    Ok(vec![
        Box::new(move || {
            validate_kes_signature(
                slot_kes_period,
                cert.operational_cert_kes_period,
                raw_header_body,
                &kes::PublicKey::try_from(cert.operational_cert_hot_vkey).map_err(|_| {
                    KesValidationError::Other(
                        "Invalid operational certificate hot vkey".to_string(),
                    )
                })?,
                &kes::Signature::try_from(body_sig)
                    .map_err(|_| KesValidationError::Other("Invalid body signature".to_string()))?,
                max_kes_evolutions,
            )?;
            Ok(())
        }),
        Box::new(move || {
            validate_operational_certificate(cert, &issuer, latest_sequence_number, is_praos)?;
            Ok(())
        }),
    ])
}

fn operational_cert<'a>(header: &'a MultiEraHeader) -> Option<OperationalCertificate<'a>> {
    match header {
        MultiEraHeader::ShelleyCompatible(x) => {
            let cert = OperationalCertificate {
                operational_cert_hot_vkey: &x.header_body.operational_cert_hot_vkey,
                operational_cert_sequence_number: x.header_body.operational_cert_sequence_number,
                operational_cert_kes_period: x.header_body.operational_cert_kes_period,
                operational_cert_sigma: &x.header_body.operational_cert_sigma,
            };
            Some(cert)
        }
        MultiEraHeader::BabbageCompatible(x) => Some(OperationalCertificate {
            operational_cert_hot_vkey: &x.header_body.operational_cert.operational_cert_hot_vkey,
            operational_cert_sequence_number: x
                .header_body
                .operational_cert
                .operational_cert_sequence_number,
            operational_cert_kes_period: x.header_body.operational_cert.operational_cert_kes_period,
            operational_cert_sigma: &x.header_body.operational_cert.operational_cert_sigma,
        }),
        _ => None,
    }
}

fn body_signature<'a>(header: &'a MultiEraHeader) -> Option<&'a [u8]> {
    match header {
        MultiEraHeader::ShelleyCompatible(x) => Some(&x.body_signature),
        MultiEraHeader::BabbageCompatible(x) => Some(&x.body_signature),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use acropolis_common::{genesis_values::GenesisValues, serialization::Bech32Conversion, Era};
    use pallas::ledger::traverse::MultiEraHeader;

    use super::*;

    #[test]
    fn test_4490511_block_produced_by_genesis_key() {
        let slots_per_kes_period = 129600;
        let max_kes_evolutions = 62;
        let genesis_values = GenesisValues::mainnet();

        let block_header_4490511: Vec<u8> =
            hex::decode(include_str!("./data/4490511.cbor")).unwrap();
        let block_header =
            MultiEraHeader::decode(Era::Shelley as u8, None, &block_header_4490511).unwrap();

        let ocert_counters = HashMap::new();
        let active_spos = vec![];

        let result = validate_block_kes(
            &block_header,
            &ocert_counters,
            &active_spos,
            &genesis_values.genesis_delegs,
            slots_per_kes_period,
            max_kes_evolutions,
        )
        .and_then(|kes_validations| {
            kes_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_4556956_block() {
        let slots_per_kes_period = 129600;
        let max_kes_evolutions = 62;
        let genesis_values = GenesisValues::mainnet();

        let block_header_4556956: Vec<u8> =
            hex::decode(include_str!("./data/4556956.cbor")).unwrap();
        let block_header =
            MultiEraHeader::decode(Era::Shelley as u8, None, &block_header_4556956).unwrap();

        let ocert_counters = HashMap::from_iter([(
            PoolId::from_bech32("pool1pu5jlj4q9w9jlxeu370a3c9myx47md5j5m2str0naunn2q3lkdy")
                .unwrap(),
            1,
        )]);
        let active_spos =
            vec![
                PoolId::from_bech32("pool1pu5jlj4q9w9jlxeu370a3c9myx47md5j5m2str0naunn2q3lkdy")
                    .unwrap(),
            ];

        let result = validate_block_kes(
            &block_header,
            &ocert_counters,
            &active_spos,
            &genesis_values.genesis_delegs,
            slots_per_kes_period,
            max_kes_evolutions,
        )
        .and_then(|kes_validations| {
            kes_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_4556956_block_with_wrong_ocert_counter() {
        let slots_per_kes_period = 129600;
        let max_kes_evolutions = 62;
        let genesis_values = GenesisValues::mainnet();

        let block_header_4556956: Vec<u8> =
            hex::decode(include_str!("./data/4556956.cbor")).unwrap();
        let block_header =
            MultiEraHeader::decode(Era::Shelley as u8, None, &block_header_4556956).unwrap();

        let ocert_counters = HashMap::from_iter([(
            PoolId::from_bech32("pool1pu5jlj4q9w9jlxeu370a3c9myx47md5j5m2str0naunn2q3lkdy")
                .unwrap(),
            2,
        )]);
        let active_spos =
            vec![
                PoolId::from_bech32("pool1pu5jlj4q9w9jlxeu370a3c9myx47md5j5m2str0naunn2q3lkdy")
                    .unwrap(),
            ];

        let result = validate_block_kes(
            &block_header,
            &ocert_counters,
            &active_spos,
            &genesis_values.genesis_delegs,
            slots_per_kes_period,
            max_kes_evolutions,
        )
        .and_then(|kes_validations| {
            kes_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Box::new(KesValidationError::OperationalCertificateError(
                OperationalCertificateError::CounterTooSmallOcert {
                    latest_counter: 2,
                    declared_counter: 1
                }
            ))
        );
    }

    #[test]
    fn test_4556956_block_with_missing_ocert_counter_and_active_spos() {
        let slots_per_kes_period = 129600;
        let max_kes_evolutions = 62;
        let genesis_values = GenesisValues::mainnet();

        let block_header_4556956: Vec<u8> =
            hex::decode(include_str!("./data/4556956.cbor")).unwrap();
        let block_header =
            MultiEraHeader::decode(Era::Shelley as u8, None, &block_header_4556956).unwrap();

        let ocert_counters = HashMap::new();
        let active_spos = vec![];

        let result = validate_block_kes(
            &block_header,
            &ocert_counters,
            &active_spos,
            &genesis_values.genesis_delegs,
            slots_per_kes_period,
            max_kes_evolutions,
        )
        .and_then(|kes_validations| {
            kes_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Box::new(KesValidationError::NoOCertCounter {
                pool_id: PoolId::from_bech32(
                    "pool1pu5jlj4q9w9jlxeu370a3c9myx47md5j5m2str0naunn2q3lkdy"
                )
                .unwrap(),
            })
        );
    }

    #[test]
    fn test_7854823_praos_block() {
        let slots_per_kes_period = 129600;
        let max_kes_evolutions = 62;
        let genesis_values = GenesisValues::mainnet();

        let block_header_7854823: Vec<u8> =
            hex::decode(include_str!("./data/7854823.cbor")).unwrap();
        let block_header =
            MultiEraHeader::decode(Era::Babbage as u8, None, &block_header_7854823).unwrap();

        let ocert_counters = HashMap::from_iter([(
            PoolId::from_bech32("pool195gdnmj6smzuakm4etxsxw3fgh8asqc4awtcskpyfnkpcvh2v8t")
                .unwrap(),
            11,
        )]);
        let active_spos =
            vec![
                PoolId::from_bech32("pool195gdnmj6smzuakm4etxsxw3fgh8asqc4awtcskpyfnkpcvh2v8t")
                    .unwrap(),
            ];

        let result = validate_block_kes(
            &block_header,
            &ocert_counters,
            &active_spos,
            &genesis_values.genesis_delegs,
            slots_per_kes_period,
            max_kes_evolutions,
        )
        .and_then(|kes_validations| {
            kes_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_ok());
    }

    #[test]
    fn test_7854823_praos_block_with_overincremented_ocert_counter() {
        let slots_per_kes_period = 129600;
        let max_kes_evolutions = 62;
        let genesis_values = GenesisValues::mainnet();

        let block_header_7854823: Vec<u8> =
            hex::decode(include_str!("./data/7854823.cbor")).unwrap();
        let block_header =
            MultiEraHeader::decode(Era::Babbage as u8, None, &block_header_7854823).unwrap();

        let ocert_counters = HashMap::from_iter([(
            PoolId::from_bech32("pool195gdnmj6smzuakm4etxsxw3fgh8asqc4awtcskpyfnkpcvh2v8t")
                .unwrap(),
            // This is just for test case
            // actual on-chain value is 11
            // now ocert counter is incremented by 2
            9,
        )]);
        let active_spos =
            vec![
                PoolId::from_bech32("pool195gdnmj6smzuakm4etxsxw3fgh8asqc4awtcskpyfnkpcvh2v8t")
                    .unwrap(),
            ];

        let result = validate_block_kes(
            &block_header,
            &ocert_counters,
            &active_spos,
            &genesis_values.genesis_delegs,
            slots_per_kes_period,
            max_kes_evolutions,
        )
        .and_then(|kes_validations| {
            kes_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            Box::new(KesValidationError::OperationalCertificateError(
                OperationalCertificateError::CounterOverIncrementedOcert {
                    latest_counter: 9,
                    declared_counter: 11,
                }
            ))
        );
    }
}
