use anyhow::Result;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use acropolis_common::{
    messages::AddressDeltasMessage, BlockInfo, BlockNumber, BlockStatus, Datum, Epoch, Era,
    UTxOIdentifier,
};

use crate::types::{
    AssetCreate, AssetSpend, CandidateUTxO, Deregistration, DeregistrationEvent, Registration,
    RegistrationEvent, UTxOMeta,
};

/// Epoch summary emitted by midnight-state logging runtime.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EpochSummary {
    pub epoch: u64,
    pub block_number: u64,
    pub status: BlockStatus,
    pub era: Era,
    pub compact_blocks: usize,
    pub extended_blocks: usize,
    pub delta_count: usize,
    pub created_utxos: usize,
    pub spent_utxos: usize,
}

trait EpochTotalsObserver {
    fn start_block(&mut self, block: &BlockInfo);
    fn observe_deltas(&mut self, deltas: &AddressDeltasMessage);
    fn finalise_block(&mut self, block: &BlockInfo);
}

#[derive(Clone, Default)]
struct EpochTotalsAccumulator {
    compact_blocks: usize,
    extended_blocks: usize,
    delta_count: usize,
    created_utxos: usize,
    spent_utxos: usize,
    last_checkpoint: Option<EpochCheckpoint>,
}

#[derive(Clone)]
struct EpochCheckpoint {
    epoch: u64,
    block_number: u64,
    status: BlockStatus,
    era: Era,
}

impl EpochCheckpoint {
    fn from_block(block: &BlockInfo) -> Self {
        Self {
            epoch: block.epoch,
            block_number: block.number,
            status: block.status.clone(),
            era: block.era,
        }
    }
}

impl EpochTotalsAccumulator {
    fn summarise_epoch(&self) -> Option<EpochSummary> {
        self.last_checkpoint.as_ref().map(|checkpoint| EpochSummary {
            epoch: checkpoint.epoch,
            block_number: checkpoint.block_number,
            status: checkpoint.status.clone(),
            era: checkpoint.era,
            compact_blocks: self.compact_blocks,
            extended_blocks: self.extended_blocks,
            delta_count: self.delta_count,
            created_utxos: self.created_utxos,
            spent_utxos: self.spent_utxos,
        })
    }

    fn reset_epoch(&mut self) {
        self.compact_blocks = 0;
        self.extended_blocks = 0;
        self.delta_count = 0;
        self.created_utxos = 0;
        self.spent_utxos = 0;
        self.last_checkpoint = None;
    }
}

impl EpochTotalsObserver for EpochTotalsAccumulator {
    fn start_block(&mut self, _block: &BlockInfo) {}

    fn observe_deltas(&mut self, deltas: &AddressDeltasMessage) {
        match deltas {
            AddressDeltasMessage::Deltas(compact_deltas) => {
                self.compact_blocks += 1;
                self.delta_count += compact_deltas.len();
                self.created_utxos +=
                    compact_deltas.iter().map(|delta| delta.created_utxos.len()).sum::<usize>();
                self.spent_utxos +=
                    compact_deltas.iter().map(|delta| delta.spent_utxos.len()).sum::<usize>();
            }
            AddressDeltasMessage::ExtendedDeltas(extended_deltas) => {
                self.extended_blocks += 1;
                self.delta_count += extended_deltas.len();
                self.created_utxos +=
                    extended_deltas.iter().map(|delta| delta.created_utxos.len()).sum::<usize>();
                self.spent_utxos +=
                    extended_deltas.iter().map(|delta| delta.spent_utxos.len()).sum::<usize>();
            }
        }
    }

    fn finalise_block(&mut self, block: &BlockInfo) {
        self.last_checkpoint = Some(EpochCheckpoint::from_block(block));
    }
}

#[derive(Clone, Default)]
pub struct State {
    // Runtime-active in this PR: epoch totals observer used for logging summaries.
    epoch_totals: EpochTotalsAccumulator,
    pending_epoch_summary: Option<EpochSummary>,

    // -----------------------------------------------------------------------
    // NOTE: Indexing scaffolding retained for follow-up work.
    // These fields are intentionally inactive in the runtime path for this PR.
    // -----------------------------------------------------------------------
    // CNight UTxO spends and creations indexed by block
    asset_utxos: AssetUTxOState,
    // Candidate (Node operator) registrations/deregistrations indexed by block
    candidate_registrations: CandidateRegistrationState,
    // Candidate (Node operator) sets indexed by the last block of each epoch
    _candidate_utxos: CandidateUTxOState,
    // Governance indexed by block
    _governance: GovernanceState,
    // Parameters indexed by epoch
    _parameters: ParametersState,
}

#[derive(Clone, Default)]
pub struct AssetUTxOState {
    pub created_utxos: BTreeMap<BlockNumber, Vec<UTxOIdentifier>>,
    pub spent_utxos: BTreeMap<BlockNumber, Vec<UTxOIdentifier>>,
    pub utxo_index: HashMap<UTxOIdentifier, UTxOMeta>,
}

#[derive(Clone, Default)]
pub struct CandidateRegistrationState {
    pub registrations: BTreeMap<BlockNumber, Vec<Arc<RegistrationEvent>>>,
    pub deregistrations: BTreeMap<BlockNumber, Vec<Arc<DeregistrationEvent>>>,
}

#[derive(Clone, Default)]
pub struct CandidateUTxOState {
    pub _current: BTreeMap<UTxOIdentifier, CandidateUTxO>,
    pub _history: BTreeMap<BlockNumber, Vec<CandidateUTxO>>,
}

#[derive(Clone, Default)]
pub struct GovernanceState {
    pub _technical_committee: HashMap<BlockNumber, Datum>,
    pub _council: HashMap<BlockNumber, Datum>,
}

#[derive(Clone, Default)]
pub struct ParametersState {
    pub _permissioned_candidates: BTreeMap<Epoch, Option<Datum>>,
}

impl State {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start_block(&mut self, block: &BlockInfo) {
        self.epoch_totals.start_block(block);
    }

    pub fn handle_address_deltas(&mut self, address_deltas: &AddressDeltasMessage) -> Result<()> {
        self.epoch_totals.observe_deltas(address_deltas);
        Ok(())
    }

    pub fn finalise_block(&mut self, block: &BlockInfo) {
        self.epoch_totals.finalise_block(block);
    }

    pub fn handle_new_epoch(&mut self) -> Result<()> {
        self.pending_epoch_summary = self.epoch_totals.summarise_epoch();
        self.epoch_totals.reset_epoch();
        Ok(())
    }

    pub fn take_epoch_summary_if_ready(&mut self) -> Option<EpochSummary> {
        self.pending_epoch_summary.take()
    }

    #[allow(dead_code)]
    /// Get the CNight UTxO creations within a specified block range
    pub fn get_asset_creates(&self, start: BlockNumber, end: BlockNumber) -> Vec<AssetCreate> {
        self.asset_utxos
            .created_utxos
            .range(start..=end)
            .flat_map(|(_, utxos)| {
                utxos.iter().map(|utxo_id| {
                    let meta = self
                        .asset_utxos
                        .utxo_index
                        .get(utxo_id)
                        .expect("UTxO index out of sync with created_utxos");

                    AssetCreate {
                        block_number: meta.created_in,
                        block_hash: meta.created_tx,
                        block_timestamp: meta.created_block_timestamp,
                        tx_index_in_block: meta.created_tx_index,
                        quantity: meta.asset_quantity,
                        holder_address: meta.holder_address.clone(),
                        tx_hash: meta.created_tx,
                        utxo_index: meta.created_utxo_index,
                    }
                })
            })
            .collect()
    }

    #[allow(dead_code)]
    /// Get the CNight UTxO spends within a specified block range
    pub fn get_asset_spends(&self, start: BlockNumber, end: BlockNumber) -> Vec<AssetSpend> {
        self.asset_utxos
            .spent_utxos
            .range(start..=end)
            .flat_map(|(_, utxos)| {
                utxos.iter().map(|utxo_id| {
                    let meta = self
                        .asset_utxos
                        .utxo_index
                        .get(utxo_id)
                        .expect("UTxO index out of sync with spent_utxos");

                    AssetSpend {
                        block_number: meta
                            .spent_in
                            .expect("UTxO index out of sync with spent_utxos"),
                        block_hash: meta.spend_tx.expect("UTxO index out of sync with spent_utxos"),
                        block_timestamp: meta
                            .spent_block_timestamp
                            .expect("UTxO index out of sync with spent_utxos"),
                        tx_index_in_block: meta
                            .spent_tx_index
                            .expect("UTxO index out of sync with spent_utxos"),
                        quantity: meta.asset_quantity,
                        holder_address: meta.holder_address.clone(),
                        utxo_tx_hash: meta.created_tx,
                        utxo_index: meta.created_utxo_index,
                        spending_tx_hash: meta
                            .spend_tx
                            .expect("UTxO index out of sync with spent_utxos"),
                    }
                })
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn get_registrations(&self, start: BlockNumber, end: BlockNumber) -> Vec<Registration> {
        self.candidate_registrations
            .registrations
            .range(start..=end)
            .flat_map(|(block_number, events)| {
                events.iter().map(|event| Registration {
                    full_datum: event.datum.clone(),
                    block_number: *block_number,
                    block_hash: event.header.block_hash,
                    block_timestamp: event.header.block_timestamp,
                    tx_index_in_block: event.header.tx_index,
                    tx_hash: event.header.tx_hash,
                    utxo_index: event.header.utxo_index,
                })
            })
            .collect()
    }

    #[allow(dead_code)]
    pub fn get_deregistrations(&self, start: BlockNumber, end: BlockNumber) -> Vec<Deregistration> {
        self.candidate_registrations
            .deregistrations
            .range(start..=end)
            .flat_map(|(block_number, events)| {
                events.iter().map(|event| Deregistration {
                    full_datum: event.datum.clone(),
                    block_number: *block_number,
                    block_hash: event.spent_block_hash,
                    block_timestamp: event.spent_block_timestamp,
                    tx_index_in_block: event.spent_tx_index,
                    tx_hash: event.spent_tx_hash,
                    utxo_tx_hash: event.header.tx_hash,
                    utxo_index: event.header.utxo_index,
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::state_history::{StateHistory, StateHistoryStore};
    use acropolis_common::{
        messages::AddressDeltasMessage, Address, BlockHash, BlockIntent, CreatedUTxOExtended,
        ExtendedAddressDelta, TxHash, TxIdentifier, UTxOIdentifier, Value,
    };

    fn make_block(epoch: u64, number: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            intent: BlockIntent::Apply,
            slot: number,
            number,
            hash: BlockHash::default(),
            epoch,
            epoch_slot: number,
            new_epoch: false,
            is_new_era: false,
            timestamp: number,
            tip_slot: None,
            era: Era::Mary,
        }
    }

    fn make_utxo(seed: u8, index: u16) -> UTxOIdentifier {
        UTxOIdentifier::new(TxHash::from([seed; 32]), index)
    }

    fn compact_message(
        deltas: usize,
        created_each: usize,
        spent_each: usize,
    ) -> AddressDeltasMessage {
        AddressDeltasMessage::Deltas(
            (0..deltas)
                .map(|i| acropolis_common::AddressDelta {
                    address: Address::None,
                    tx_identifier: TxIdentifier::new(1, i as u16),
                    spent_utxos: (0..spent_each)
                        .map(|j| make_utxo(1 + i as u8, j as u16))
                        .collect(),
                    created_utxos: (0..created_each)
                        .map(|j| make_utxo(21 + i as u8, j as u16))
                        .collect(),
                    sent: Value::new(spent_each as u64, vec![]),
                    received: Value::new(created_each as u64, vec![]),
                })
                .collect(),
        )
    }

    fn extended_message(
        deltas: usize,
        created_each: usize,
        spent_each: usize,
    ) -> AddressDeltasMessage {
        AddressDeltasMessage::ExtendedDeltas(
            (0..deltas)
                .map(|i| ExtendedAddressDelta {
                    address: Address::None,
                    tx_identifier: TxIdentifier::new(2, i as u16),
                    spent_utxos: (0..spent_each)
                        .map(|j| acropolis_common::SpentUTxOExtended {
                            utxo: make_utxo(41 + i as u8, j as u16),
                            value: Value::new(1, vec![]),
                            spent_by: TxHash::from([61 + i as u8; 32]),
                            datum: None,
                        })
                        .collect(),
                    created_utxos: (0..created_each)
                        .map(|j| CreatedUTxOExtended {
                            utxo: make_utxo(81 + i as u8, j as u16),
                            value: Value::new(1, vec![]),
                            datum: None,
                        })
                        .collect(),
                    sent: Value::new(spent_each as u64, vec![]),
                    received: Value::new(created_each as u64, vec![]),
                })
                .collect(),
        )
    }

    #[test]
    fn accumulates_within_epoch_and_summarises_on_transition() {
        let mut state = State::new();

        let block_1 = make_block(10, 100);
        state.start_block(&block_1);
        state.handle_address_deltas(&compact_message(2, 1, 2)).unwrap();
        state.finalise_block(&block_1);

        let block_2 = make_block(10, 101);
        state.start_block(&block_2);
        state.handle_address_deltas(&extended_message(1, 3, 1)).unwrap();
        state.finalise_block(&block_2);

        state.handle_new_epoch().unwrap();
        let summary = state.take_epoch_summary_if_ready().unwrap();

        assert_eq!(summary.epoch, 10);
        assert_eq!(summary.block_number, 101);
        assert_eq!(summary.era, Era::Mary);
        assert_eq!(summary.status, BlockStatus::Immutable);
        assert_eq!(summary.compact_blocks, 1);
        assert_eq!(summary.extended_blocks, 1);
        assert_eq!(summary.delta_count, 3);
        assert_eq!(summary.created_utxos, 5);
        assert_eq!(summary.spent_utxos, 5);
    }

    #[test]
    fn epoch_transition_resets_totals() {
        let mut state = State::new();

        let block_1 = make_block(20, 200);
        state.start_block(&block_1);
        state.handle_address_deltas(&compact_message(1, 2, 0)).unwrap();
        state.finalise_block(&block_1);

        state.handle_new_epoch().unwrap();
        let summary_epoch_20 = state.take_epoch_summary_if_ready().unwrap();
        assert_eq!(summary_epoch_20.epoch, 20);
        assert_eq!(summary_epoch_20.delta_count, 1);

        let block_2 = make_block(21, 201);
        state.start_block(&block_2);
        state.handle_address_deltas(&extended_message(1, 0, 4)).unwrap();
        state.finalise_block(&block_2);

        state.handle_new_epoch().unwrap();
        let summary_epoch_21 = state.take_epoch_summary_if_ready().unwrap();
        assert_eq!(summary_epoch_21.epoch, 21);
        assert_eq!(summary_epoch_21.compact_blocks, 0);
        assert_eq!(summary_epoch_21.extended_blocks, 1);
        assert_eq!(summary_epoch_21.delta_count, 1);
        assert_eq!(summary_epoch_21.created_utxos, 0);
        assert_eq!(summary_epoch_21.spent_utxos, 4);
    }

    #[test]
    fn rollback_replay_uses_rolled_back_state() {
        let mut history =
            StateHistory::<State>::new("midnight_state_test", StateHistoryStore::Unbounded);

        let block_1 = make_block(30, 300);
        let mut state = history.get_or_init_with(State::new);
        state.start_block(&block_1);
        state.handle_address_deltas(&compact_message(1, 1, 0)).unwrap();
        state.finalise_block(&block_1);
        history.commit(block_1.number, state);

        let block_2 = make_block(30, 301);
        let mut state = history.get_or_init_with(State::new);
        state.start_block(&block_2);
        state.handle_address_deltas(&extended_message(2, 1, 1)).unwrap();
        state.finalise_block(&block_2);
        history.commit(block_2.number, state);

        let mut state = history.get_rolled_back_state(block_2.number);
        state.start_block(&block_2);
        state.handle_address_deltas(&compact_message(0, 0, 0)).unwrap();
        state.finalise_block(&block_2);
        history.commit(block_2.number, state);

        let mut state = history.get_or_init_with(State::new);
        state.handle_new_epoch().unwrap();
        let summary = state.take_epoch_summary_if_ready().unwrap();

        assert_eq!(summary.epoch, 30);
        assert_eq!(summary.compact_blocks, 2);
        assert_eq!(summary.extended_blocks, 0);
        assert_eq!(summary.delta_count, 1);
        assert_eq!(summary.created_utxos, 1);
        assert_eq!(summary.spent_utxos, 0);
    }

    #[test]
    fn zero_delta_epoch_boundary_produces_zero_summary() {
        let mut state = State::new();

        let block = make_block(40, 400);
        state.start_block(&block);
        state.handle_address_deltas(&extended_message(0, 0, 0)).unwrap();
        state.finalise_block(&block);

        state.handle_new_epoch().unwrap();
        let summary = state.take_epoch_summary_if_ready().unwrap();

        assert_eq!(summary.epoch, 40);
        assert_eq!(summary.compact_blocks, 0);
        assert_eq!(summary.extended_blocks, 1);
        assert_eq!(summary.delta_count, 0);
        assert_eq!(summary.created_utxos, 0);
        assert_eq!(summary.spent_utxos, 0);
    }
}
