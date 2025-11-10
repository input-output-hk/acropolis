use std::borrow::Cow;
use std::sync::Arc;
use pallas::ledger::primitives::{alonzo, byron};
use pallas::ledger::traverse::MultiEraTx;
use acropolis_common::BlockInfo;
use acropolis_common::messages::{ProtocolParamsMessage, RawTxsMessage};
use acropolis_common::validation::{ValidationError, ValidationStatus};
use crate::TxValidatorPhase1StateConfig;
use anyhow::Result;

pub struct State {
    pub config: Arc<TxValidatorPhase1StateConfig>,
    params: Option<ProtocolParamsMessage>
}

fn convert_from_pallas_alonzo<'b> (tx: &alonzo::MintedTx<'b>) {
    return Transaction{tx}
}

impl State {
    pub fn new(config: Arc<TxValidatorPhase1StateConfig>) -> Self {
        Self { config, params: None }
    }

    pub async fn process_params(
        &mut self, _blk: BlockInfo, prm: ProtocolParamsMessage
    ) -> Result<()> {
        self.params = Some(prm);
        Ok(())
    }

    /// Byron validation is not acutally performed, so it's always returns 'Go'
    fn validate_byron<'b>(&self, _tx: Box<Cow<'b, byron::MintedTxPayload<'b>>>)
        -> Result<ValidationStatus>
    {
        Ok(ValidationStatus::Go)
    }

    pub fn process_transactions(
        &mut self, _blk: &BlockInfo, txs_msg: &RawTxsMessage
    ) -> Result<ValidationStatus> {
        for (tx_index , raw_tx) in txs_msg.txs.iter().enumerate() {
            // Parse the tx
            let res = match MultiEraTx::decode(raw_tx) {
                Err(e) =>
                    ValidationStatus::NoGo(
                        ValidationError::CborDecodeError(tx_index, e.to_string())
                    ),
                Ok(MultiEraTx::Byron(byron_tx)) => self.validate_byron(byron_tx)?,

                Ok(MultiEraTx::AlonzoCompatible(tx, _)) =>
                    self.validate_tx(convert_from_pallas_alonzo(tx))?,
                Ok(MultiEraTx::Babbage(tx)) =>
                    self.validate_tx(convert_from_pallas_babbage(tx))?,
                Ok(MultiEraTx::Conway(tx)) =>
                    self.validate_tx(convert_from_conway_babbage(tx))?,
                _ => ValidationStatus::NoGo(ValidationError::CborDecodeError(0, "".to_string()))
            };

            if let ValidationStatus::NoGo(_) = &res {
                return Ok(res);
            }
        }
        Ok(ValidationStatus::Go)
    }
}

