use anyhow::Result;

use acropolis_common::{messages::AddressDeltasMessage, BlockInfo};

use crate::{
    configuration::MidnightConfig,
    indexes::{
        candidate_state::CandidateState, cnight_utxo_state::CNightUTxOState,
        governance_state::GovernanceState, parameters_state::ParametersState,
    },
};

#[derive(Clone, Default)]
pub struct State {
    // CNight UTxO spends and creations indexed by block
    _utxos: CNightUTxOState,
    // Candidate (Node operator) sets by epoch and registrations/deregistrations by block
    candidates: CandidateState,
    // Governance indexed by block
    _governance: GovernanceState,
    // Parameters indexed by epoch
    parameters: ParametersState,
    // Midnight configuration
    _config: MidnightConfig,
}

impl State {
    pub fn new(_config: MidnightConfig) -> Self {
        Self {
            _config,
            ..Self::default()
        }
    }

    /// Snapshot the candidate set and Ariadne parameters at epoch boundary
    pub fn handle_new_epoch(&mut self, block_info: &BlockInfo) -> Result<()> {
        self.candidates.snapshot_candidate_set(block_info.number);
        self.parameters.snapshot_parameters(block_info.epoch);
        Ok(())
    }

    pub fn handle_address_deltas(&mut self, _address_deltas: &AddressDeltasMessage) -> Result<()> {
        Ok(())
    }
}
