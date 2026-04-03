use imbl::{HashMap, HashSet};

use acropolis_common::{Epoch, UTxOIdentifier};

use crate::{
    grpc::midnight_state_proto::EpochCandidate,
    types::{DeregistrationEvent, RegistrationEvent},
};

#[derive(Clone, Default, serde::Serialize)]
pub struct CommitteeCandidateState {
    // Registration lookup retained for deregistration detection and epoch snapshots.
    pub registration_index: HashMap<UTxOIdentifier, RegistrationEvent>,
    // The active committee candidate set.
    active_candidates: HashSet<UTxOIdentifier>,
    // The committee candidate set at a given epoch.
    epoch_index: HashMap<Epoch, Vec<UTxOIdentifier>>,
}

impl CommitteeCandidateState {
    pub fn register_candidates(&mut self, registrations: Vec<RegistrationEvent>) {
        for registration in registrations {
            let identifier = UTxOIdentifier {
                tx_hash: registration.tx_hash,
                output_index: registration.utxo_index,
            };
            self.active_candidates.insert(identifier);
            self.registration_index.insert(identifier, registration);
        }
    }

    pub fn deregister_candidates(&mut self, deregistrations: Vec<DeregistrationEvent>) {
        for event in &deregistrations {
            self.active_candidates.remove(&event.registration_utxo);
        }
    }

    pub fn snapshot_epoch(&mut self, epoch: Epoch) {
        self.epoch_index.insert(epoch, self.active_candidates.iter().cloned().collect());
    }

    pub fn get_epoch_candidates(&self, epoch: Epoch) -> Vec<EpochCandidate> {
        let Some(identifiers) = self.epoch_index.get(&epoch) else {
            return Vec::new();
        };

        identifiers
            .iter()
            .filter_map(|id| {
                let registration = self.registration_index.get(id)?;

                Some(EpochCandidate::from(registration))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use acropolis_common::{BlockHash, Datum, TxHash};

    use super::*;

    fn registration_event(tx: u8, tx_index: u32) -> RegistrationEvent {
        RegistrationEvent {
            block_number: 0,
            epoch: 0,
            slot_number: 0,
            tx_index,
            block_hash: BlockHash::default(),
            block_timestamp: 0,
            tx_hash: TxHash::from([tx; 32]),
            utxo_index: 0,
            datum: Datum::Inline(vec![tx]),
            tx_inputs: vec![],
        }
    }

    fn registration_id(tx: u8) -> UTxOIdentifier {
        UTxOIdentifier::new(TxHash::from([tx; 32]), 0)
    }

    #[test]
    fn snapshots_active_candidates_by_epoch() {
        let mut state = CommitteeCandidateState::default();
        state.register_candidates(vec![registration_event(1, 0), registration_event(2, 1)]);

        state.snapshot_epoch(42);

        let mut ids: Vec<_> = state
            .get_epoch_candidates(42)
            .into_iter()
            .map(|candidate| candidate.utxo_tx_hash)
            .collect();
        ids.sort();

        assert_eq!(ids, vec![vec![1; 32], vec![2; 32]]);
    }

    #[test]
    fn deregistration_removes_candidate_from_future_snapshots() {
        let mut state = CommitteeCandidateState::default();
        let registration = registration_event(7, 0);
        let registration_utxo = registration_id(7);

        state.register_candidates(vec![registration]);
        state.snapshot_epoch(1);
        assert_eq!(state.get_epoch_candidates(1).len(), 1);

        state.deregister_candidates(vec![DeregistrationEvent {
            spent_tx_index: 1,
            registration_utxo,
            spent_block_hash: BlockHash::default(),
            spent_block_timestamp: 0,
            spent_tx_hash: TxHash::from([8; 32]),
        }]);
        state.snapshot_epoch(2);

        assert!(state.get_epoch_candidates(2).is_empty());
    }
}
