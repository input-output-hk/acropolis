use anyhow::{anyhow, Result};
use std::collections::{BTreeMap, HashMap};

use acropolis_common::{messages::AddressDeltasMessage, BlockInfo, BlockNumber, Datum, Epoch};

use crate::epoch_totals::{EpochSummary, EpochTotals};
use crate::indexes::{candidate_state::CandidateState, cnight_utxo_state::CNightUTxOState};

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
}

#[derive(Clone, Default)]
pub struct GovernanceState {
    pub _technical_committee: HashMap<BlockNumber, Datum>,
    pub _council: HashMap<BlockNumber, Datum>,
}

#[allow(dead_code)]
#[derive(Clone, Default)]
pub struct ParametersState {
    pub current: Option<Datum>,
    pub permissioned_candidates: BTreeMap<Epoch, Datum>,
}

#[allow(dead_code)]
impl ParametersState {
    fn snapshot_parameters(&mut self, epoch: Epoch) {
        let Some(current) = self.current.clone() else {
            return;
        };

        if self.permissioned_candidates.values().next_back() != Some(&current) {
            self.permissioned_candidates.insert(epoch, current);
        }
    }
}

impl State {
    pub fn new() -> Self {
        Self::default()
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

    pub fn handle_new_epoch(&mut self, boundary_block: &BlockInfo) -> EpochSummary {
        let summary = self.epoch_totals.summarise_completed_epoch(boundary_block);
        self.epoch_totals.reset_epoch();
        summary
    }
}
