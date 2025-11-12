use acropolis_common::{
    genesis_values::GenesisValues,
    messages::{ProtocolParamsMessage, SPOStateMessage},
    validation::KesValidationError,
    BlockInfo, PoolId,
};
use imbl::HashMap;
use pallas::ledger::traverse::MultiEraHeader;
use tracing::error;

use crate::ouroboros;

#[derive(Default, Debug, Clone)]
pub struct State {
    pub ocert_counters: HashMap<PoolId, u64>,

    pub slots_per_kes_period: Option<u64>,

    pub max_kes_evolutions: Option<u64>,

    pub active_spos: Vec<PoolId>,
}

impl State {
    pub fn new() -> Self {
        Self {
            ocert_counters: HashMap::new(),
            slots_per_kes_period: None,
            max_kes_evolutions: None,
            active_spos: Vec::new(),
        }
    }

    pub fn handle_protocol_parameters(&mut self, msg: &ProtocolParamsMessage) {
        if let Some(shelley_params) = msg.params.shelley.as_ref() {
            self.slots_per_kes_period = Some(shelley_params.slots_per_kes_period as u64);
            self.max_kes_evolutions = Some(shelley_params.max_kes_evolutions as u64);
        }
    }

    pub fn handle_spo_state(&mut self, msg: &SPOStateMessage) {
        self.active_spos = msg.spos.iter().map(|spo| spo.operator).collect();
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

        let result = ouroboros::kes_validation::validate_block_kes(
            &header,
            &self.ocert_counters,
            &self.active_spos,
            &genesis.genesis_delegs,
            slots_per_kes_period,
            max_kes_evolutions,
        )
        .and_then(|kes_validations| {
            kes_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
        });

        result
    }
}
