use std::collections::HashSet;

use acropolis_common::{
    queries::governance::VoteRecord, KeyHash, PoolRegistration, PoolUpdateEvent,
};
use serde::{Deserialize, Serialize};

use crate::store_config::StoreConfig;

// Historical SPO State
// each field can be optional (according to configurations)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalSPOState {
    pub registration: Option<PoolRegistration>,
    pub updates: Option<Vec<PoolUpdateEvent>>,

    // SPO's delegators
    pub delegators: Option<HashSet<KeyHash>>,
    // SPO's votes
    pub votes: Option<Vec<VoteRecord>>,
}

impl HistoricalSPOState {
    #[allow(dead_code)]
    pub fn new(store_config: &StoreConfig) -> Self {
        Self {
            registration: store_config.store_registration.then(PoolRegistration::default),
            updates: store_config.store_updates.then(Vec::new),
            delegators: store_config.store_delegators.then(HashSet::new),
            votes: store_config.store_votes.then(Vec::new),
        }
    }

    pub fn add_delegator(&mut self, delegator: &KeyHash) -> Option<bool> {
        let Some(delegators) = self.delegators.as_mut() else {
            return None;
        };
        Some(delegators.insert(delegator.clone()))
    }

    pub fn remove_delegator(&mut self, delegator: &KeyHash) -> Option<bool> {
        let Some(delegators) = self.delegators.as_mut() else {
            return None;
        };
        Some(delegators.remove(delegator))
    }
}
