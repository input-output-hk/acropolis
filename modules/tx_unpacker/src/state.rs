use crate::validations;
use acropolis_common::{
    messages::{ProtocolParamsMessage, RawTxsMessage},
    protocol_params::ProtocolParams,
    validation::{TransactionValidationError, ValidationError},
    BlockInfo, GenesisDelegates,
};
use anyhow::Result;

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

    fn validate_transaction(
        &self,
        block_info: &BlockInfo,
        raw_tx: &[u8],
        genesis_delegs: &GenesisDelegates,
    ) -> Result<(), Box<TransactionValidationError>> {
        validations::validate_tx(
            raw_tx,
            genesis_delegs,
            &self.protocol_params.shelley,
            block_info,
        )
    }

    pub fn validate(
        &self,
        block_info: &BlockInfo,
        txs_msg: &RawTxsMessage,
        genesis_delegs: &GenesisDelegates,
    ) -> Result<(), Box<ValidationError>> {
        let mut bad_transactions = Vec::new();
        for (tx_index, raw_tx) in txs_msg.txs.iter().enumerate() {
            let tx_index = tx_index as u16;

            // Validate transaction
            if let Err(e) = self.validate_transaction(block_info, raw_tx, genesis_delegs) {
                bad_transactions.push((tx_index, *e));
            }
        }

        if bad_transactions.is_empty() {
            Ok(())
        } else {
            Err(Box::new(ValidationError::BadTransactions {
                bad_transactions,
            }))
        }
    }
}
