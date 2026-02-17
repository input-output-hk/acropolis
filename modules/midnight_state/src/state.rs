use anyhow::Result;
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use acropolis_common::{messages::AddressDeltasMessage, BlockNumber, Datum, Epoch, UTxOIdentifier};

use crate::types::{CandidateUTxO, DeregistrationEvent, RegistrationEvent, UTxOMeta};

#[derive(Clone, Default)]
pub struct State {
    // CNight UTxO spends and creations indexed by block
    _asset_utxos: AssetUTxOState,
    // Candidate (Node operator) registrations/deregistrations indexed by block
    _candidate_registrations: CandidateRegistrationState,
    // Candidate (Node operator) sets indexed by the last block of each epoch
    _candidate_utxos: CandidateUTxOState,
    // Governance indexed by block
    _governance: GovernanceState,
    // Parameters indexed by epoch
    _parameters: ParametersState,
}

#[derive(Clone, Default)]
pub struct AssetUTxOState {
    pub _created_utxos: BTreeMap<BlockNumber, Vec<UTxOIdentifier>>,
    pub _spent_utxos: BTreeMap<BlockNumber, Vec<UTxOIdentifier>>,
    pub _utxo_index: HashMap<UTxOIdentifier, UTxOMeta>,
}

#[derive(Clone, Default)]
pub struct CandidateRegistrationState {
    pub _registrations: BTreeMap<BlockNumber, Vec<Arc<RegistrationEvent>>>,
    pub _deregistrations: BTreeMap<BlockNumber, Vec<Arc<DeregistrationEvent>>>,
    pub _datum_index: HashMap<UTxOIdentifier, Datum>,
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
}
