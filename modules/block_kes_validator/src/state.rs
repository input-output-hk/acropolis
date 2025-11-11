use crate::ouroboros::praos::OperationalCertificate;
use acropolis_common::{
    genesis_values::GenesisValues, messages::ProtocolParamsMessage, validation::KesValidationError,
    BlockInfo, PoolId,
};
use imbl::HashMap;
use pallas::ledger::{primitives::babbage::OperationalCert, traverse::MultiEraHeader};
use tracing::error;

#[derive(Default, Debug, Clone)]
pub struct State {
    pub ocert_counters: HashMap<PoolId, u64>,

    pub slots_per_kes_period: Option<u64>,

    pub max_kes_evolutions: Option<u64>,
}

impl State {
    pub fn new() -> Self {
        Self {
            ocert_counters: HashMap::new(),
            slots_per_kes_period: None,
            max_kes_evolutions: None,
        }
    }

    pub fn handle_protocol_parameters(&mut self, msg: &ProtocolParamsMessage) {
        if let Some(shelley_params) = msg.params.shelley.as_ref() {
            self.slots_per_kes_period = Some(shelley_params.slots_per_kes_period as u64);
            self.max_kes_evolutions = Some(shelley_params.max_kes_evolutions as u64);
        }
    }

    pub fn validate_block_kes(
        &self,
        block_info: &BlockInfo,
        raw_header: &[u8],
        genesis: &GenesisValues,
    ) -> Result<(), Box<KesValidationError>> {
        // Validation starts after Shelley Era
        if block_info.epoch < genesis.shelley_epoch {
            return Ok(());
        }

        let header = match MultiEraHeader::decode(block_info.era as u8, None, raw_header) {
            Ok(header) => header,
            Err(e) => {
                error!("Can't decode header {}: {e}", block_info.slot);
                return Err(Box::new(KesValidationError::Other(format!(
                    "Can't decode header {}: {e}",
                    block_info.slot
                ))));
            }
        };

        let Some(slots_per_kes_period) = self.slots_per_kes_period else {
            return Err(Box::new(KesValidationError::Other(
                "Slots per KES period is not set".to_string(),
            )));
        };
        let Some(max_kes_evolutions) = self.max_kes_evolutions else {
            return Err(Box::new(KesValidationError::Other(
                "Max KES evolutions is not set".to_string(),
            )));
        };

        let cert = operational_cert(&header).ok_or(Box::new(KesValidationError::Other(
            "Can't get operational certificate".to_string(),
        )))?;

        Ok(())
    }
}

fn operational_cert<'a>(header: &'a MultiEraHeader) -> Option<OperationalCertificate<'a>> {
    match header {
        MultiEraHeader::BabbageCompatible(x) => Some(OperationalCertificate {
            operational_cert_hot_vkey: &x.header_body.operational_cert.operational_cert_hot_vkey,
            operational_cert_sequence_number: x
                .header_body
                .operational_cert
                .operational_cert_sequence_number,
            operational_cert_kes_period: x.header_body.operational_cert.operational_cert_kes_period,
            operational_cert_sigma: &x.header_body.operational_cert.operational_cert_sigma,
        }),
        MultiEraHeader::ShelleyCompatible(x) => {
            let cert = OperationalCertificate {
                operational_cert_hot_vkey: &x.header_body.operational_cert_hot_vkey,
                operational_cert_sequence_number: x.header_body.operational_cert_sequence_number,
                operational_cert_kes_period: x.header_body.operational_cert_kes_period,
                operational_cert_sigma: &x.header_body.operational_cert_sigma,
            };
            Some(cert)
        }
        _ => None,
    }
}
