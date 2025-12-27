use std::collections::HashSet;

use acropolis_common::{
    genesis_values::GenesisValues,
    messages::{ProtocolParamsMessage, SPOStateMessage},
    validation::{KesValidationError, ValidationError},
    BlockInfo, PoolId,
};
use imbl::HashMap;
use pallas::ledger::traverse::MultiEraHeader;
use tracing::error;

use crate::ouroboros;

#[derive(Default, Debug, Clone)]
pub struct State {
    /// Tracks the latest operational certificate counter for each pool
    pub ocert_counters: HashMap<PoolId, u64>,

    pub slots_per_kes_period: Option<u64>,

    pub max_kes_evolutions: Option<u64>,

    pub active_spos: HashSet<PoolId>,
}

impl State {
    pub fn new() -> Self {
        Self {
            ocert_counters: HashMap::new(),
            slots_per_kes_period: None,
            max_kes_evolutions: None,
            active_spos: HashSet::new(),
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

    /// Initialize opcert counters from snapshot bootstrap data
    pub fn bootstrap_from_snapshot(
        &mut self,
        opcert_counters: &std::collections::HashMap<PoolId, u64>,
    ) {
        // Convert std::collections::HashMap to imbl::HashMap
        self.ocert_counters = opcert_counters.iter().map(|(k, v)| (*k, *v)).collect();

        eprintln!("Bootstrapped KES opcert counters for {} pools: {:?}", self.ocert_counters.len(), self.ocert_counters);
    }

    pub fn update_ocert_counter(&mut self, pool_id: PoolId, declared_sequence_number: u64) {
        self.ocert_counters.insert(pool_id, declared_sequence_number);
    }

    pub fn validate(
        &self,
        block_info: &BlockInfo,
        raw_header: &[u8],
        genesis: &GenesisValues,
    ) -> Result<Option<(PoolId, u64)>, Box<ValidationError>> {
        // Validation starts after Shelley Era
        if block_info.epoch < genesis.shelley_epoch {
            return Ok(None);
        }

        let header = match MultiEraHeader::decode(block_info.era as u8, None, raw_header) {
            Ok(header) => header,
            Err(e) => {
                error!("Can't decode header {}: {e}", block_info.slot);
                return Err(Box::new(ValidationError::CborDecodeError {
                    era: block_info.era,
                    slot: block_info.slot,
                    reason: e.to_string(),
                }));
            }
        };

        let Some(slots_per_kes_period) = self.slots_per_kes_period else {
            return Err(Box::new(
                KesValidationError::Other("Slots per KES period is not set".to_string()).into(),
            ));
        };
        let Some(max_kes_evolutions) = self.max_kes_evolutions else {
            return Err(Box::new(
                KesValidationError::Other("Max KES evolutions is not set".to_string()).into(),
            ));
        };

        let result = ouroboros::kes_validation::validate_block_kes(
            &header,
            &self.ocert_counters,
            &self.active_spos,
            &genesis.genesis_delegs,
            slots_per_kes_period,
            max_kes_evolutions,
        )
        .and_then(|(kes_validations, pool_id, declared_sequence_number)| {
            kes_validations.iter().try_for_each(|assert| assert().map_err(Box::new))?;
            Ok(Some((pool_id, declared_sequence_number)))
        });

        result.map_err(|e| Box::new((*e).into()))
    }
}
