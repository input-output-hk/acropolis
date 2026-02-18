use anyhow::{anyhow, Result};
use std::collections::{BTreeMap, HashMap};

use acropolis_common::{BlockNumber, UTxOIdentifier};

use crate::types::{
    Deregistration, DeregistrationEvent, Registration, RegistrationEvent, TxOutput,
};

#[derive(Clone, Default)]
pub struct CandidateState {
    // Candidate registrations by block enabling range lookups
    pub registrations: BTreeMap<BlockNumber, Vec<RegistrationEvent>>,
    // Candidate deregistrations by block enabling range lookups
    pub deregistrations: BTreeMap<BlockNumber, Vec<DeregistrationEvent>>,
    // Current candidate set
    pub current: HashMap<UTxOIdentifier, TxOutput>,
    // Candidate set snapshots at the last block of each epoch
    pub history: HashMap<BlockNumber, Vec<TxOutput>>,
}

impl CandidateState {
    #[allow(dead_code)]
    /// Handle all candidate registrations for a block
    pub fn register_candidates(
        &mut self,
        block: BlockNumber,
        candidates: Vec<(TxOutput, RegistrationEvent)>,
    ) {
        let mut registrations = Vec::with_capacity(candidates.len());
        for (candidate, event) in candidates {
            self.current.insert(candidate.utxo, candidate);
            registrations.push(event);
        }
        self.registrations.insert(block, registrations);
    }

    #[allow(dead_code)]
    /// Handle all candidate deregistrations for a block
    pub fn deregister_candidates(&mut self, block: BlockNumber, events: Vec<DeregistrationEvent>) {
        for event in &events {
            self.current.remove(&UTxOIdentifier::new(
                event.registration.tx_hash,
                event.registration.utxo_index,
            ));
        }

        self.deregistrations.insert(block, events);
    }

    /// Snapshot the current candidate set and insert into history
    pub fn snapshot_candidate_set(&mut self, block: BlockNumber) {
        if !self.current.is_empty() {
            self.history.insert(block, self.current.values().cloned().collect());
        }
    }

    #[allow(dead_code)]
    /// Get the candidate registrations within a specified block range
    pub fn get_registrations(&self, start: BlockNumber, end: BlockNumber) -> Vec<Registration> {
        self.registrations
            .range(start..=end)
            .flat_map(|(block_number, events)| {
                events.iter().map(|event| Registration::from((*block_number, event)))
            })
            .collect()
    }

    #[allow(dead_code)]
    /// Get the candidate deregistrations within a specified block range
    pub fn get_deregistrations(&self, start: BlockNumber, end: BlockNumber) -> Vec<Deregistration> {
        self.deregistrations
            .range(start..=end)
            .flat_map(|(block_number, events)| {
                events.iter().map(|event| Deregistration::from((*block_number, event)))
            })
            .collect()
    }

    #[allow(dead_code)]
    /// Get the registered candidate set at a specified last block in an epoch
    pub fn get_registered_candidates(
        &self,
        last_block_in_epoch: BlockNumber,
    ) -> Result<Vec<TxOutput>> {
        match self.history.get(&last_block_in_epoch) {
            Some(tx_outputs) => Ok(tx_outputs.to_vec()),
            None => Err(anyhow!("Requested block not indexed")),
        }
    }
}
