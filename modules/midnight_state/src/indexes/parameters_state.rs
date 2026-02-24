use std::collections::BTreeMap;

use acropolis_common::{Datum, Epoch};

#[derive(Clone, Default)]
pub struct ParametersState {
    /// Ariadne parameters keyed by epoch
    pub permissioned_candidates: BTreeMap<Epoch, Datum>,
}

impl ParametersState {
    #[allow(dead_code)]
    /// Insert the parameters for an epoch on change, overwriting existing entry if multiple
    /// updates in the same epoch
    pub fn add_parameter_datum(&mut self, epoch: Epoch, datum: Datum) -> bool {
        if self.permissioned_candidates.last_key_value().map(|(_, v)| v) != Some(&datum) {
            self.permissioned_candidates.insert(epoch, datum);
            true
        } else {
            false
        }
    }

    #[allow(dead_code)]
    /// Get the Ariadne parameters valid at a specific epoch
    pub fn get_ariadne_parameters(&self, epoch: Epoch) -> Option<(Epoch, Datum)> {
        self.permissioned_candidates
            .range(..=epoch)
            .next_back()
            .map(|(epoch, datum)| (*epoch, datum.clone()))
    }
}
