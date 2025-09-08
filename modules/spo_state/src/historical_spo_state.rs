use std::collections::HashSet;

use acropolis_common::{
    queries::governance::VoteRecord, KeyHash, PoolRegistration, PoolUpdateEvent, StakeCredential,
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
    pub fn new(store_config: StoreConfig) -> Self {
        Self {
            registration: store_config.store_registration.then(PoolRegistration::default),
            updates: store_config.store_updates.then(Vec::new),
            delegators: store_config.store_delegators.then(HashSet::new),
            votes: store_config.store_votes.then(Vec::new),
        }
    }

    pub fn handle_stake_delegation(&mut self, credential: &StakeCredential, spo: &KeyHash) {
        let hash = credential.get_hash();
    }
}
