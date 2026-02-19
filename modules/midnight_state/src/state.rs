use anyhow::{anyhow, Result};

use acropolis_common::{messages::AddressDeltasMessage, BlockInfo};

use crate::{
    configuration::MidnightConfig,
    epoch_totals::{EpochSummary, EpochTotals},
    indexes::{
        candidate_state::CandidateState, cnight_utxo_state::CNightUTxOState,
        governance_state::GovernanceState, parameters_state::ParametersState,
    },
};

#[derive(Clone, Default)]
pub struct State {
    // Runtime-active in this PR: epoch totals observer used for logging summaries.
    epoch_totals: EpochTotals,

    // -----------------------------------------------------------------------
    // NOTE: Indexing scaffolding retained for follow-up work.
    // These fields are intentionally inactive in the runtime path for this PR.
    // -----------------------------------------------------------------------
    // CNight UTxO spends and creations indexed by block
    _utxos: CNightUTxOState,
    // Candidate (Node operator) sets by epoch and registrations/deregistrations by block
    _candidates: CandidateState,
    // Governance indexed by block
    _governance: GovernanceState,
    // Parameters indexed by epoch
    _parameters: ParametersState,
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

    pub fn start_block(&mut self, block: &BlockInfo) {
        self.epoch_totals.start_block(block);
    }

    pub fn handle_address_deltas(&mut self, address_deltas: &AddressDeltasMessage) -> Result<()> {
        let extended_deltas = address_deltas.as_extended_deltas().map_err(|e| {
            anyhow!("{e}; midnight-state requires AddressDeltasMessage::ExtendedDeltas")
        })?;
        self.epoch_totals.observe_deltas(extended_deltas);
        Ok(())
    }

    pub fn finalise_block(&mut self, block: &BlockInfo) {
        self.epoch_totals.finalise_block(block);
    }

    pub fn handle_new_epoch(&mut self, block_info: &BlockInfo) -> EpochSummary {
        let summary = self.epoch_totals.summarise_completed_epoch(block_info);
        self.epoch_totals.reset_epoch();
        summary
    }
}
