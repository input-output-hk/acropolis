use acropolis_common::validation::{KesSignatureError, OperationalCertificateError};
use pallas::crypto::key::ed25519;

use crate::ouroboros::kes;

pub struct OperationalCertificate<'a> {
    pub operational_cert_hot_vkey: &'a [u8],
    pub operational_cert_sequence_number: u64,
    pub operational_cert_kes_period: u64,
    pub operational_cert_sigma: &'a [u8],
}

pub fn validate_kes_signature<'a>(
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
