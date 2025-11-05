//! Acropolis block_vrf_validator state storage

use std::sync::Arc;

use acropolis_common::{
    genesis_values::GenesisValues,
    messages::{
        EpochNonceMessage, ProtocolParamsMessage, SPOStakeDistributionMessage, SPOStateMessage,
    },
    ouroboros::{self, vrf_validation::VrfValidationError},
    protocol_params::{Nonce, PraosParams, ShelleyParams},
    BlockInfo, Era,
};
use anyhow::Result;
use pallas::ledger::traverse::MultiEraHeader;

use crate::snapshot::Snapshot;

#[derive(Default, Debug, Clone)]
pub struct EpochSnapshots {
    pub mark: Arc<Snapshot>,
    pub set: Arc<Snapshot>,
    pub go: Arc<Snapshot>,
}

impl EpochSnapshots {
    /// Push a new snapshot
    pub fn push(&mut self, latest: Snapshot) {
        self.go = self.set.clone();
        self.set = self.mark.clone();
        self.mark = Arc::new(latest);
    }
}

#[derive(Default, Debug, Clone)]
pub struct State {
    /// shelley params
    pub shelley_params: Option<ShelleyParams>,

    /// protocol parameter for Praos and TPraos
    pub praos_params: Option<PraosParams>,

    /// epoch nonce
    pub epoch_nonce: Option<Nonce>,

    /// epoch snapshots
    pub epoch_snapshots: EpochSnapshots,
}

impl State {
    pub fn new() -> Self {
        Self {
            praos_params: None,
            shelley_params: None,
            epoch_nonce: None,
            epoch_snapshots: EpochSnapshots::default(),
        }
    }

    pub fn handle_protocol_parameters(&mut self, msg: &ProtocolParamsMessage) {
        if let Some(shelley_params) = msg.params.shelley.as_ref() {
            self.shelley_params = Some(shelley_params.clone());
            self.praos_params = Some(shelley_params.into());
        }
    }

    pub fn handle_epoch_nonce(&mut self, msg: &EpochNonceMessage) {
        self.epoch_nonce = msg.nonce.clone();
    }

    pub fn handle_new_snapshot(
        &mut self,
        spo_state_msg: &SPOStateMessage,
        spdd_msg: &SPOStakeDistributionMessage,
    ) {
        let new_snapshot = Snapshot::from((spo_state_msg, spdd_msg));
        self.epoch_snapshots.push(new_snapshot);
    }

    pub fn validate_block_vrf(
        &self,
        block_info: &BlockInfo,
        header: &MultiEraHeader,
        genesis: &GenesisValues,
    ) -> Result<(), VrfValidationError> {
        // Validation starts after Shelley Era
        if block_info.epoch < genesis.shelley_epoch {
            return Ok(());
        }

        let Some(shelley_params) = self.shelley_params.as_ref() else {
            return Err(VrfValidationError::InvalidShelleyParams(
                "Shelley Params are not set".to_string(),
            ));
        };
        let Some(praos_params) = self.praos_params.as_ref() else {
            return Err(VrfValidationError::InvalidShelleyParams(
                "Praos Params are not set".to_string(),
            ));
        };
        let Some(epoch_nonce) = self.epoch_nonce.as_ref() else {
            return Err(VrfValidationError::MissingEpochNonce);
        };
        let decentralisation_param = shelley_params.protocol_params.decentralisation_param;

        let is_tpraos = matches!(
            block_info.era,
            Era::Shelley | Era::Allegra | Era::Mary | Era::Alonzo
        );

        if is_tpraos {
            let result = ouroboros::tpraos::validate_vrf_tpraos(
                block_info,
                header,
                epoch_nonce,
                &genesis.genesis_delegs,
                praos_params,
                &self.epoch_snapshots.set.active_spos,
                &self.epoch_snapshots.set.active_stakes,
                self.epoch_snapshots.set.total_active_stakes,
                decentralisation_param,
            )
            .and_then(|vrf_validations| vrf_validations.iter().try_for_each(|assert| assert()));
            result
        } else {
            let result = ouroboros::praos::validate_vrf_praos(
                block_info,
                header,
                epoch_nonce,
                praos_params,
                &self.epoch_snapshots.set.active_spos,
                &self.epoch_snapshots.set.active_stakes,
                self.epoch_snapshots.set.total_active_stakes,
            )
            .and_then(|vrf_validations| vrf_validations.iter().try_for_each(|assert| assert()));
            result
        }
    }
}
