//! Acropolis Protocol Params: State storage

use crate::ParametersUpdater;
use acropolis_common::{
    messages::{GovernanceOutcomesMessage, ProtocolParamsMessage},
    BlockInfo, Era,
};
use anyhow::{bail, Result};
use tracing::info;

pub struct State {
    pub current_params: ParametersUpdater,
    pub current_era: Option<Era>,
}

impl State {
    pub fn new() -> Self {
        Self {
            current_params: ParametersUpdater::new(),
            current_era: None,
        }
    }

    pub fn apply_genesis(&mut self, new_block: &BlockInfo) -> Result<()> {
        if let Some(ref era) = self.current_era {
            if *era == new_block.era {
                return Ok(());
            }
        }

        info!("Applying genesis for {}", new_block.era);

        self.current_era = Some(new_block.era.clone());
        self.current_params.apply_genesis(&new_block.era)?;

        info!("Applied genesis for {}, resulting params {:?}", 
            new_block.era, self.current_params.get_params()
        );

        Ok(())
    }

    pub async fn handle_enact_state(
        &mut self,
        block: &BlockInfo,
        msg: &GovernanceOutcomesMessage,
    ) -> Result<ProtocolParamsMessage> {
        if !block.new_epoch {
            bail!("Enact state for block {block:?} (not a new epoch)");
        }

        self.apply_genesis(&block)?;
        self.current_params.apply_enact_state(msg)?;
        Ok(ProtocolParamsMessage {
            params: self.current_params.get_params(),
        })
    }

    #[allow(dead_code)]
    pub async fn tick(&self) -> Result<()> {
        Ok(())
    }
}
