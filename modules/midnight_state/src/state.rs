use anyhow::Result;
use std::collections::{BTreeMap, HashMap};

use acropolis_common::{messages::AddressDeltasMessage, BlockInfo, BlockNumber, Datum, Epoch};

use crate::indexes::{candidate_state::CandidateState, cnight_utxo_state::CNightUTxOState};

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
}

#[derive(Clone, Default)]
pub struct GovernanceState {
    pub _technical_committee: HashMap<BlockNumber, Datum>,
    pub _council: HashMap<BlockNumber, Datum>,
}

#[derive(Clone, Default)]
pub struct ParametersState {
    pub current: Option<Datum>,
    pub permissioned_candidates: BTreeMap<Epoch, Datum>,
}

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

    pub fn handle_address_deltas(&mut self, _address_deltas: &AddressDeltasMessage) -> Result<()> {
        Ok(())
    }

    pub fn handle_new_epoch(&mut self, block_info: &BlockInfo) -> Result<()> {
        // Snapshot the candidate set and parameters at epoch boundary
        self.candidates.snapshot_candidate_set(block_info.number);
        self.parameters.snapshot_parameters(block_info.epoch);
        Ok(())
    }
}
