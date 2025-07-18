//! Acropolis Protocol Params: State storage

use crate::ParametersUpdater;
use acropolis_common::{
    messages::{GovernanceOutcomesMessage, ProtocolParamsMessage},
    BlockInfo, Era,
};
use anyhow::{bail, Result};
use std::{collections::BTreeMap, ops::RangeInclusive};
use tracing::info;

pub struct State {
    pub network_name: String,
    pub active_epoch: u64,
    pub current_params: ParametersUpdater,
    pub current_era: Option<Era>,
    pub parameter_history: Option<BTreeMap<u64, ProtocolParamsMessage>>,
}

impl State {
    pub fn new(network_name: String, store_history: bool) -> Self {
        Self {
            network_name,
            active_epoch: 0,
            current_params: ParametersUpdater::new(),
            current_era: None,
            parameter_history: if store_history {
                Some(BTreeMap::new())
            } else {
                None
            },
        }
    }

    fn genesis_era_range(from_era: Option<Era>, to_era: Era) -> RangeInclusive<u8> {
        match from_era {
            None => Era::default() as u8..=to_era as u8,
            Some(e) => e as u8 + 1..=to_era as u8,
        }
    }

    pub fn apply_genesis(&mut self, new_block: &BlockInfo) -> Result<()> {
        let to_apply = Self::genesis_era_range(self.current_era.clone(), new_block.era.clone());
        if to_apply.is_empty() {
            return Ok(());
        }

        for mid_era_u8 in to_apply {
            let mid_era = Era::try_from(mid_era_u8)?;
            info!("Applying genesis {} for {}", self.network_name, mid_era);

            self.current_params.apply_genesis(&self.network_name, &mid_era)?;
        }

        info!(
            "Applied genesis up to {}, resulting params {:?}",
            new_block.era,
            self.current_params.get_params()
        );
        self.current_era = Some(new_block.era.clone());
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
        let params_message = ProtocolParamsMessage {
            params: self.current_params.get_params(),
        };

        if let Some(history) = self.parameter_history.as_mut() {
            let last = history.range(..block.epoch).next_back();

            let should_store = match last {
                Some((_, last_params)) => last_params.params != params_message.params,
                None => true,
            };

            if should_store {
                history.insert(block.epoch, params_message.clone());
                self.active_epoch = block.epoch;
            }
        }

        Ok(params_message)
    }

    #[allow(dead_code)]
    pub async fn tick(&self) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::State;
    use acropolis_common::Era;
    use anyhow::Result;

    #[test]
    fn test_genesis_era_range() -> Result<()> {
        assert_eq!(State::genesis_era_range(None, Era::Byron), 0..=0);

        assert!(State::genesis_era_range(Some(Era::Byron), Era::Byron).is_empty());
        assert_eq!(State::genesis_era_range(None, Era::Conway), 0..=6);
        assert_eq!(
            State::genesis_era_range(Some(Era::Byron), Era::Conway),
            1..=6
        );
        assert_eq!(
            State::genesis_era_range(Some(Era::Byron), Era::Shelley),
            1..=1
        );
        assert!(State::genesis_era_range(Some(Era::Conway), Era::Conway).is_empty());

        // Assert that empty range does not lead to impossible conversions.
        // Stupid test, but follows a pattern: "if you ever have a doubt about
        // some impossible behaviour, then write a test/assert about it".
        for x in State::genesis_era_range(Some(Era::Conway), Era::Conway) {
            println!("{x} => {}", Era::try_from(x)?);
        }
        Ok(())
    }
}
