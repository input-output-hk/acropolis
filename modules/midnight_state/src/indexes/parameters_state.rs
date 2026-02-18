use std::collections::BTreeMap;

use acropolis_common::{Datum, Epoch};

#[derive(Clone, Default)]
pub struct ParametersState {
    /// The current Ariadne parameters
    pub current: Option<Datum>,
    /// Ariadne parameters keyed by epoch
    pub permissioned_candidates: BTreeMap<Epoch, Datum>,
}

impl ParametersState {
    #[allow(dead_code)]
    /// Update the current parameters
    pub fn update_current_parameters(&mut self, datum: Datum) {
        self.current = Some(datum);
    }

    /// Snapshot the current paramters and store in `permissioned_candidates`
    pub fn snapshot_parameters(&mut self, epoch: Epoch) {
        let Some(current) = self.current.clone() else {
            return;
        };

        if self.permissioned_candidates.last_key_value().map(|(_, v)| v) != Some(&current) {
            self.permissioned_candidates.insert(epoch, current);
        }
    }

    #[allow(dead_code)]
    /// Get the Ariadne parameters valid at a specific epoch
    pub fn get_ariadne_parameters(&self, epoch: Epoch) -> Option<Datum> {
        self.permissioned_candidates.range(..=epoch).next_back().map(|(_, datum)| datum.clone())
    }
}
