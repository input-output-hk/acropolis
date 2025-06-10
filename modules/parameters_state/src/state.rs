//! Acropolis Protocol Params: State storage

use crate::ParametersUpdater;
use acropolis_common::{
    messages::{EnactStateMessage, GenesisCompleteMessage, ProtocolParamsMessage},
    BlockInfo, Era, ProtocolParams,
};
use anyhow::{anyhow, Result};
use tracing::info;

pub struct State {
    pub genesis: ProtocolParams,

    pub current_params: ParametersUpdater,
    pub current_era: Option<Era>,
}

impl State {
    pub fn new() -> Self {
        Self {
            genesis: ProtocolParams::default(),

            current_params: ParametersUpdater::new(),
            current_era: None,
        }
    }

    pub fn apply_genesis(&mut self, new_epoch_block: &BlockInfo) {
        if let Some(ref era) = self.current_era {
            if *era == new_epoch_block.era {
                return;
            }
        }
        info!("Applying genesis for {:?}", new_epoch_block.era);

        self.current_params
            .apply_genesis(&new_epoch_block.era, &self.genesis);
        self.current_era = Some(new_epoch_block.era.clone());
    }

    pub async fn handle_genesis(&mut self, message: &GenesisCompleteMessage) -> Result<()> {
        info!(
            "Received genesis complete message; conway present = {}",
            message.conway_genesis.is_some()
        );
        self.genesis.conway = message.conway_genesis.clone();
        Ok(())
    }

    pub async fn handle_enact_state(
        &mut self,
        block: &BlockInfo,
        msg: &EnactStateMessage,
    ) -> Result<ProtocolParamsMessage> {
        if !block.new_epoch {
            return Err(anyhow!(
                "Enact state at block {:?} (not a new epoch)",
                block
            ));
        }

        self.apply_genesis(&block);
        self.current_params.apply_enact_state(msg);
        Ok(ProtocolParamsMessage {
            params: self.current_params.get_params(),
        })
    }

    #[allow(dead_code)]
    pub async fn tick(&self) -> Result<()> {
        Ok(())
    }
}
