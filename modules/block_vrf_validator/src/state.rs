//! Acropolis block_vrf_validator state storage

use std::sync::Arc;

use crate::{ouroboros, snapshot::Snapshot};
use acropolis_common::{
    genesis_values::GenesisValues,
    messages::{
        EpochActivityMessage, ProtocolParamsMessage, SPOStakeDistributionMessage, SPOStateMessage,
    },
    protocol_params::Nonce,
    rational_number::RationalNumber,
    validation::VrfValidationError,
    BlockInfo, Era,
};
use anyhow::Result;
use pallas::ledger::traverse::MultiEraHeader;
use tracing::error;

#[derive(Default, Debug, Clone)]
pub struct EpochSnapshots {
    pub mark: Arc<Snapshot>,
    pub set: Arc<Snapshot>,
}

impl EpochSnapshots {
    /// Push a new snapshot
    pub fn push(&mut self, latest: Snapshot) {
        self.set = self.mark.clone();
        self.mark = Arc::new(latest);
    }
}

#[derive(Default, Debug, Clone)]
pub struct State {
    pub decentralisation_param: Option<RationalNumber>,

    pub active_slots_coeff: Option<RationalNumber>,

    /// epoch nonce
    pub epoch_nonce: Option<Nonce>,

    /// epoch snapshots
    pub epoch_snapshots: EpochSnapshots,
}

impl State {
    pub fn new() -> Self {
        Self {
            active_slots_coeff: None,
            decentralisation_param: None,
            epoch_nonce: None,
            epoch_snapshots: EpochSnapshots::default(),
        }
    }

    pub fn handle_protocol_parameters(&mut self, msg: &ProtocolParamsMessage) {
        if let Some(shelley_params) = msg.params.shelley.as_ref() {
            self.decentralisation_param =
                Some(shelley_params.protocol_params.decentralisation_param);
            self.active_slots_coeff = Some(shelley_params.active_slots_coeff);
        }
    }

    pub fn handle_epoch_activity(&mut self, msg: &EpochActivityMessage) {
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
        raw_header: &[u8],
        genesis: &GenesisValues,
    ) -> Result<(), Box<VrfValidationError>> {
        let header = match MultiEraHeader::decode(block_info.era as u8, None, raw_header) {
            Ok(header) => header,
            Err(e) => {
                error!("Can't decode header {}: {e}", block_info.slot);
                return Err(Box::new(VrfValidationError::Other(format!(
                    "Can't decode header {}: {e}",
                    block_info.slot
                ))));
            }
        };

        // Validation starts after Shelley Era
        if block_info.epoch < genesis.shelley_epoch {
            return Ok(());
        }

        let Some(decentralisation_param) = self.decentralisation_param else {
            return Err(Box::new(VrfValidationError::Other(
                "Decentralisation Param is not set".to_string(),
            )));
        };
        let Some(active_slots_coeff) = self.active_slots_coeff else {
            return Err(Box::new(VrfValidationError::Other(
                "Active Slots Coeff is not set".to_string(),
            )));
        };
        let Some(epoch_nonce) = self.epoch_nonce.as_ref() else {
            return Err(Box::new(VrfValidationError::Other(
                "Epoch Nonce is not set".to_string(),
            )));
        };

        let is_tpraos = matches!(
            block_info.era,
            Era::Shelley | Era::Allegra | Era::Mary | Era::Alonzo
        );

        if is_tpraos {
            let result = ouroboros::tpraos::validate_vrf_tpraos(
                block_info,
                &header,
                epoch_nonce,
                &genesis.genesis_delegs,
                active_slots_coeff,
                decentralisation_param,
                &self.epoch_snapshots.set.active_spos,
                &self.epoch_snapshots.set.active_stakes,
                self.epoch_snapshots.set.total_active_stakes,
            )
            .and_then(|vrf_validations| {
                vrf_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
            });
            result
        } else {
            let result = ouroboros::praos::validate_vrf_praos(
                block_info,
                &header,
                epoch_nonce,
                active_slots_coeff,
                &self.epoch_snapshots.set.active_spos,
                &self.epoch_snapshots.set.active_stakes,
                self.epoch_snapshots.set.total_active_stakes,
            )
            .and_then(|vrf_validations| {
                vrf_validations.iter().try_for_each(|assert| assert().map_err(Box::new))
            });
            result
        }
    }
}
