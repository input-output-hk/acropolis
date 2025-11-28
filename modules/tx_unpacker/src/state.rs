use crate::{utxo_registry::UTxORegistry, validations};
use acropolis_common::{
    messages::ProtocolParamsMessage, protocol_params::ProtocolParams,
    validation::TransactionValidationError, BlockInfo, Era,
};
use anyhow::Result;
use pallas::ledger::traverse::MultiEraTx;

#[derive(Default, Clone)]
pub struct State {
    pub protocol_params: ProtocolParams,
}

impl State {
    pub fn new() -> Self {
        Self {
            protocol_params: ProtocolParams::default(),
        }
    }

    pub fn handle_protocol_params(&mut self, msg: &ProtocolParamsMessage) {
        self.protocol_params = msg.params.clone();
    }

    pub fn validate_transaction(
        &self,
        block_info: &BlockInfo,
        tx: &MultiEraTx,
        utxo_registry: &UTxORegistry,
    ) -> Result<(), TransactionValidationError> {
        match block_info.era {
            Era::Shelley => {
                let Some(shelley_params) = self.protocol_params.shelley.as_ref() else {
                    return Err(TransactionValidationError::Other(
                        "Shelley params are not set".to_string(),
                    ));
                };
                validations::validate_shelley_tx(tx, shelley_params, block_info.slot, |tx_ref| {
                    utxo_registry.lookup_by_hash(tx_ref)
                })
            }
            _ => Ok(()),
        }
    }
}
