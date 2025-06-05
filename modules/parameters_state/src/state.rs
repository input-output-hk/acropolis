//! Acropolis Protocol Params: State storage

use anyhow::{anyhow, Result};
use tracing::info;
use acropolis_common::{
    messages::{EnactStateMessage, GenesisCompleteMessage, ProtocolParamsMessage}, 
    BlockInfo, Era, ProtocolParams
};
use crate::ParametersUpdater;

pub struct State {
    pub genesis: ProtocolParams,

    pub current_params: ParametersUpdater,
    pub current_epoch: u64,
    pub current_era: Era
}

impl State {
    pub fn new() -> Self {
        Self {
            genesis: ProtocolParams::default(),

            current_params: ParametersUpdater::new(),
            current_era: Era::default(),
            current_epoch: 0
        }
    }

    pub fn process_new_epoch(&mut self, new_epoch_block: &BlockInfo) {
        if self.current_era < new_epoch_block.era {
            self.current_params.apply_genesis (&new_epoch_block.era, &self.genesis);
            self.current_era = new_epoch_block.era.clone();
        }
    }

    pub async fn handle_genesis(&mut self, 
        message: &GenesisCompleteMessage
    ) -> Result<()> {
        info!("Received genesis complete message; conway present = {}", 
            message.conway_genesis.is_some()
        );
        self.genesis.conway = message.conway_genesis.clone();
        Ok(())
    }

    pub async fn handle_enact_state(&mut self,
        block: &BlockInfo, 
        msg: &EnactStateMessage
    ) -> Result<ProtocolParamsMessage> {
        if !block.new_epoch {
            return Err(anyhow!("Enact state at block {:?} (not a new epoch)", block));
        }

        self.process_new_epoch(&block);
        self.current_params.apply_enact_state(msg);
        Ok(ProtocolParamsMessage { params: self.current_params.get_params() })
    }

    fn log_stats(&self) {
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats();
        Ok(())
    }
}
