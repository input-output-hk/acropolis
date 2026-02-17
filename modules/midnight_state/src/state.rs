use anyhow::Result;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use acropolis_common::{messages::AddressDeltasMessage, BlockNumber, Datum, Epoch, UTxOIdentifier};

use crate::types::{
    AssetCreate, AssetSpend, CandidateUTxO, Deregistration, DeregistrationEvent, Registration,
    RegistrationEvent, UTxOMeta,
};

#[derive(Clone, Default)]
pub struct State {
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
    pub datum_index: HashMap<UTxOIdentifier, Datum>,
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

    pub fn handle_address_deltas(&mut self, _address_deltas: &AddressDeltasMessage) -> Result<()> {
        Ok(())
    }

    pub fn handle_new_epoch(&mut self) -> Result<()> {
        Ok(())
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
}
