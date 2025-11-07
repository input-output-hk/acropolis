use std::sync::Arc;
use acropolis_common::BlockInfo;
use acropolis_common::messages::{ProtocolParamsMessage, RawTxsMessage};
use acropolis_common::validation::ValidationStatus;
use crate::TxValidatorPhase1StateConfig;

pub struct State {
    pub config: Arc<TxValidatorPhase1StateConfig>,
    params: Option<ProtocolParamsMessage>
}


impl State {
    pub fn new(config: Arc<TxValidatorPhase1StateConfig>) -> Self {
        Self { config, params: None }
    }

    pub async fn process_params(
        &mut self, _blk: BlockInfo, prm: ProtocolParamsMessage
    ) -> anyhow::Result<()> {
        self.params = Some(prm);
        Ok(())
    }

    pub async fn process_transactions(
        &mut self, _blk: &BlockInfo, _trx: &RawTxsMessage
    ) -> anyhow::Result<ValidationStatus> {
        Ok(ValidationStatus::Go)
    }
}

