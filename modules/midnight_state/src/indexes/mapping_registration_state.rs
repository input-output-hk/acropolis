use imbl::{HashMap, OrdMap};

use acropolis_common::{BlockNumber, UTxOIdentifier};

use crate::types::{Deregistration, DeregistrationEvent, Registration, RegistrationEvent};

#[derive(Clone, Default, serde::Serialize)]
pub struct MappingRegistrationState {
    // Mapping registrations by block enabling range lookups.
    registrations: OrdMap<BlockNumber, Vec<UTxOIdentifier>>,
    // Mapping deregistrations by block enabling range lookups.
    deregistrations: OrdMap<BlockNumber, Vec<DeregistrationEvent>>,
    // Registration index to avoid duplicating registration data in deregistrations.
    pub registration_index: HashMap<UTxOIdentifier, RegistrationEvent>,
}

impl MappingRegistrationState {
    /// Handle all mapping registrations for a block.
    pub fn add_registrations(&mut self, block: BlockNumber, registrations: Vec<RegistrationEvent>) {
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

    /// Handle all mapping deregistrations for a block.
    pub fn add_deregistrations(
        &mut self,
        block: BlockNumber,
        deregistrations: Vec<DeregistrationEvent>,
    ) {
        self.deregistrations.insert(block, deregistrations);
    }

    /// Get the mapping registrations within a specified block range.
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

    /// Get the mapping deregistrations within a specified block range.
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

#[cfg(test)]
mod tests {
    use acropolis_common::{BlockHash, Datum, TxHash};

    use super::*;

    fn id(i: u8) -> UTxOIdentifier {
        UTxOIdentifier::new(TxHash::from([i; 32]), 0)
    }

    fn registration_event(tx_index: u32) -> RegistrationEvent {
        RegistrationEvent {
            block_number: 0,
            epoch: 0,
            slot_number: 0,
            tx_index,
            block_hash: BlockHash::default(),
            block_timestamp: 0,
            tx_hash: TxHash::default(),
            utxo_index: 0,
            datum: Datum::Inline(vec![1u8; 32]),
            tx_inputs: vec![],
        }
    }

    fn dereg_event(tx_index: u32, reg: UTxOIdentifier) -> DeregistrationEvent {
        DeregistrationEvent {
            spent_tx_index: tx_index,
            registration_utxo: reg,
            spent_block_hash: BlockHash::default(),
            spent_block_timestamp: 1,
            spent_tx_hash: TxHash::default(),
        }
    }

    #[test]
    fn registrations_returns_entries_at_or_after_start_tx_index() {
        let mut state = MappingRegistrationState::default();

        let id0 = id(0);
        let id1 = id(1);
        let id2 = id(2);

        state.registrations.insert(10, vec![id0, id1, id2]);

        state.registration_index.insert(id0, registration_event(0));
        state.registration_index.insert(id1, registration_event(1));
        state.registration_index.insert(id2, registration_event(2));

        let result = state.get_registrations(10, 1, 10);

        let txs: Vec<u32> = result.iter().map(|r| r.tx_index_in_block).collect();

        assert_eq!(txs, vec![1, 2]);
    }

    #[test]
    fn registrations_limits_to_capacity() {
        let mut state = MappingRegistrationState::default();

        let ids = [id(1), id(2), id(3), id(4)];

        state.registrations.insert(10, ids.to_vec());

        for (i, identifier) in ids.iter().enumerate() {
            state.registration_index.insert(*identifier, registration_event(i as u32));
        }

        let result = state.get_registrations(10, 0, 2);

        assert_eq!(result.len(), 2);
    }

    #[test]
    fn deregistrations_returns_entries_at_or_after_start_tx_index() {
        let mut state = MappingRegistrationState::default();

        let id0 = id(0);
        let id1 = id(1);
        let id2 = id(2);

        state.deregistrations.insert(
            10,
            vec![
                dereg_event(0, id0),
                dereg_event(1, id1),
                dereg_event(2, id2),
            ],
        );

        state.registration_index.insert(id0, registration_event(0));
        state.registration_index.insert(id1, registration_event(1));
        state.registration_index.insert(id2, registration_event(2));

        let result = state.get_deregistrations(10, 1, 10);

        let txs: Vec<u32> = result.iter().map(|r| r.tx_index_in_block).collect();

        assert_eq!(txs, vec![1, 2]);
    }

    #[test]
    fn deregistrations_limits_to_capacity() {
        let mut state = MappingRegistrationState::default();

        let id0 = id(0);
        let id1 = id(1);
        let id2 = id(2);

        state.deregistrations.insert(
            10,
            vec![
                dereg_event(0, id0),
                dereg_event(1, id1),
                dereg_event(2, id2),
            ],
        );

        state.registration_index.insert(id0, registration_event(0));
        state.registration_index.insert(id1, registration_event(1));
        state.registration_index.insert(id2, registration_event(2));

        let result = state.get_deregistrations(10, 0, 2);

        assert_eq!(result.len(), 2);
    }
}
