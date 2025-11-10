use acropolis_common::{
    genesis_values::GenesisValues, messages::ProtocolParamsMessage, validation::KesValidationError,
    BlockInfo, PoolId,
};
use imbl::HashMap;

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
        Ok(())
    }
}
