use acropolis_common::{
    queries::governance::VoteRecord, PoolRegistration, PoolUpdateEvent, StakeAddress,
};
use imbl::{HashSet, OrdMap, Vector};
use serde::{Deserialize, Serialize};

use crate::store_config::StoreConfig;

// Historical SPO State
// each field can be optional (according to configurations)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoricalSPOState {
    pub registration: Option<PoolRegistration>,
    pub updates: Option<Vec<PoolUpdateEvent>>,

    // SPO's delegators
    pub delegators: Option<HashSet<StakeAddress>>,
    // SPO's votes
    pub votes: Option<Vec<VoteRecord>>,

    // blocks
    // <Epoch Number, Block Heights>
    pub blocks: Option<OrdMap<u64, Vector<u64>>>,
}

impl HistoricalSPOState {
    #[allow(dead_code)]
    pub fn new(store_config: &StoreConfig) -> Self {
        Self {
            registration: store_config.store_registration.then(PoolRegistration::default),
            updates: store_config.store_updates.then(Vec::new),
            delegators: store_config.store_delegators.then(HashSet::new),
            votes: store_config.store_votes.then(Vec::new),
            blocks: store_config.store_blocks.then(OrdMap::new),
        }
    }

    pub fn add_pool_registration(&mut self, reg: &PoolRegistration) -> Option<bool> {
        // update registration if enabled
        self.registration.as_mut().map(|registration| {
            *registration = reg.clone();
            true
        })
    }

    pub fn add_pool_updates(&mut self, update: PoolUpdateEvent) -> Option<bool> {
        // update updates if enabled
        self.updates.as_mut().map(|updates| {
            updates.push(update);
            true
        })
    }

    pub fn add_delegator(&mut self, delegator: &StakeAddress) -> Option<bool> {
        self.delegators.as_mut().map(|delegators| delegators.insert(delegator.clone()).is_some())
    }

    pub fn remove_delegator(&mut self, delegator: &StakeAddress) -> Option<bool> {
        self.delegators.as_mut().map(|delegators| delegators.remove(delegator).is_some())
    }

    pub fn get_all_blocks(&self) -> Option<Vec<u64>> {
        self.blocks.as_ref().map(|blocks| blocks.values().flatten().cloned().collect())
    }

    pub fn get_blocks_by_epoch(&self, epoch: u64) -> Option<Vec<u64>> {
        self.blocks
            .as_ref()
            .and_then(|blocks| blocks.get(&epoch).map(|blocks| blocks.iter().cloned().collect()))
    }

    pub fn add_block(&mut self, epoch: u64, block_number: u64) -> Option<()> {
        self.blocks.as_mut().map(|blocks| {
            blocks.entry(epoch).or_insert_with(Vector::new).push_back(block_number);
        })
    }
}
