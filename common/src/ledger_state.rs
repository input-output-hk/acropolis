use crate::{PoolId, PoolRegistration};
use std::collections::BTreeMap;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct LedgerState {
    pub spo_state: SPOState,
}

pub struct UTxOState {}

pub struct StakeDistributionState {}

pub struct AccountState {}

pub struct ParametersState {}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default, Eq, PartialEq)]
pub struct SPOState {
    pub pools: BTreeMap<PoolId, PoolRegistration>,
    pub updates: BTreeMap<PoolId, PoolRegistration>,
    pub retiring: BTreeMap<PoolId, u64>,
}

impl SPOState {
    pub fn new() -> Self {
        Self {
            pools: BTreeMap::new(),
            updates: BTreeMap::new(),
            retiring: BTreeMap::new(),
        }
    }

    pub fn extend(&mut self, extension: &Self) {
        self.pools.extend(extension.pools.clone());
        self.updates.extend(extension.updates.clone());
        self.retiring.extend(extension.retiring.clone());
    }
}

pub struct DRepState {}

pub struct ProposalState {}

pub struct VotingState {}
