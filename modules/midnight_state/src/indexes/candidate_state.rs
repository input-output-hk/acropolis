use std::collections::BTreeMap;

use acropolis_common::BlockNumber;

use crate::types::{Deregistration, DeregistrationEvent, Registration, RegistrationEvent};

#[derive(Clone, Default)]
pub struct CandidateState {
    // Candidate registrations by block enabling range lookups
    registrations: BTreeMap<BlockNumber, Vec<RegistrationEvent>>,
    // Candidate deregistrations by block enabling range lookups
    deregistrations: BTreeMap<BlockNumber, Vec<DeregistrationEvent>>,
}

impl CandidateState {
    #[allow(dead_code)]
    /// Handle all candidate registrations for a block
    pub fn register_candidates(
        &mut self,
        block: BlockNumber,
        registrations: Vec<RegistrationEvent>,
    ) {
        self.registrations.insert(block, registrations);
    }

    #[allow(dead_code)]
    /// Handle all candidate deregistrations for a block
    pub fn deregister_candidates(
        &mut self,
        block: BlockNumber,
        deregistrations: Vec<DeregistrationEvent>,
    ) {
        self.deregistrations.insert(block, deregistrations);
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
}
