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
    pub fn get_registrations(
        &self,
        start: BlockNumber,
        start_tx_index: u32,
        utxo_capacity: usize,
    ) -> Vec<Registration> {
        self.registrations
            .range(start..)
            .flat_map(|(block_number, identifiers)| {
                identifiers.iter().filter_map(move |identifier| {
                    let event = self.registration_index.get(identifier)?;

                    if *block_number == start && event.tx_index < start_tx_index {
                        return None;
                    }

                    Some(Registration::from((*block_number, event)))
                })
            })
            .take(utxo_capacity)
            .collect()
    }

    /// Get the candidate deregistrations within a specified block range
    pub fn get_deregistrations(
        &self,
        start: BlockNumber,
        start_tx_index: u32,
        utxo_capacity: usize,
    ) -> Vec<Deregistration> {
        self.deregistrations
            .range(start..)
            .flat_map(|(block_number, events)| {
                events.iter().filter_map(move |event| {
                    if *block_number == start && event.spent_tx_index < start_tx_index {
                        return None;
                    }

                    let registration = self.registration_index.get(&event.registration_utxo)?;

                    Some(Deregistration::from((*block_number, registration, event)))
                })
            })
            .take(utxo_capacity)
            .collect()
    }
}
