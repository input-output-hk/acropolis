use imbl::{HashMap, OrdMap};

use acropolis_common::{BlockNumber, UTxOIdentifier};

use crate::types::{Deregistration, DeregistrationEvent, Registration, RegistrationEvent};

#[derive(Clone, Default)]
pub struct CandidateState {
    // Candidate registrations by block enabling range lookups
    registrations: OrdMap<BlockNumber, Vec<UTxOIdentifier>>,
    // Candidate deregistrations by block enabling range lookups
    deregistrations: OrdMap<BlockNumber, Vec<DeregistrationEvent>>,
    // Registration index to avoid duplicating in deregistrations
    pub registration_index: HashMap<UTxOIdentifier, RegistrationEvent>,
}

impl CandidateState {
    /// Handle all candidate registrations for a block
    pub fn register_candidates(
        &mut self,
        block: BlockNumber,
        registrations: Vec<RegistrationEvent>,
    ) {
        let mut identifiers = Vec::new();
        for registration in registrations {
            let identifier = UTxOIdentifier {
                tx_hash: registration.tx_hash,
                output_index: registration.utxo_index,
            };
            identifiers.push(identifier);
            self.registration_index.insert(identifier, registration);
        }
        self.registrations.insert(block, identifiers);
    }

    /// Handle all candidate deregistrations for a block
    pub fn deregister_candidates(
        &mut self,
        block: BlockNumber,
        deregistrations: Vec<DeregistrationEvent>,
    ) {
        self.deregistrations.insert(block, deregistrations);
    }

    /// Get the candidate registrations within a specified block range
    pub fn get_registrations(&self, start: BlockNumber, end: BlockNumber) -> Vec<Registration> {
        self.registrations
            .range(start..=end)
            .flat_map(|(block_number, identifiers)| {
                identifiers.iter().filter_map(|identifier| {
                    self.registration_index
                        .get(identifier)
                        .map(|event| Registration::from((*block_number, event)))
                })
            })
            .collect()
    }

    /// Get the candidate deregistrations within a specified block range
    pub fn get_deregistrations(&self, start: BlockNumber, end: BlockNumber) -> Vec<Deregistration> {
        self.deregistrations
            .range(start..=end)
            .flat_map(|(block_number, events)| {
                events.iter().filter_map(|event| {
                    self.registration_index.get(&event.registration_utxo).map(|registration| {
                        Deregistration::from((*block_number, registration, event))
                    })
                })
            })
            .collect()
    }
}
