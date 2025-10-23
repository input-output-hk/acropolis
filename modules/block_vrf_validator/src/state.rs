//! Acropolis block_vrf_validator state storage

use acropolis_common::{
    genesis_values::{GenesisDelegs, GenesisValues},
    messages::ProtocolParamsMessage,
    protocol_params::{PraosParams, ShelleyParams},
    BlockInfo,
};
use anyhow::Result;
use pallas::ledger::traverse::MultiEraHeader;

use crate::ouroboros::vrf_validation::{self, VrfValidationError};

#[derive(Default, Debug, Clone)]
pub struct State {
    // shelley params
    pub shelly_params: Option<ShelleyParams>,

    // protocol parameter for Praos and TPraos
    pub praos_params: Option<PraosParams>,
}

impl State {
    pub fn new() -> Self {
        Self {
            praos_params: None,
            shelly_params: None,
        }
    }

    /// Handle protocol parameters updates
    pub fn handle_protocol_parameters(&mut self, msg: &ProtocolParamsMessage) {
        if let Some(shelly_params) = msg.params.shelley.as_ref() {
            self.shelly_params = Some(shelly_params.clone());
            self.praos_params = Some(shelly_params.into());
        }
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

        let Some(shelley_params) = self.shelly_params.as_ref() else {
            return Err(VrfValidationError::ShelleyParams(
                "Shelley Params are not set".to_string(),
            ));
        };
        let Some(praos_params) = self.praos_params.as_ref() else {
            return Err(VrfValidationError::ShelleyParams(
                "Praos Params are not set".to_string(),
            ));
        };

        vrf_validation::validate_vrf(
            block_info,
            header,
            shelley_params,
            praos_params,
            &genesis.genesis_delegs,
        )?;

        Ok(())
    }
}
