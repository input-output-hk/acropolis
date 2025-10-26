//! Acropolis Protocol Params: State storage

use crate::ParametersUpdater;
use acropolis_common::{
    messages::{GovernanceOutcomesMessage, ProtocolParamsMessage},
    BlockInfo, Era,
};
use anyhow::Result;
use std::ops::RangeInclusive;
use tracing::info;

#[derive(Default, Clone)]
pub struct State {
    pub network_name: String,
    pub current_params: ParametersUpdater,
    pub current_era: Option<Era>,
}

impl State {
    pub fn new(network_name: String) -> Self {
        Self {
            network_name,
            current_params: ParametersUpdater::new(),
            current_era: None,
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
        self.current_era = Some(new_block.era);
        Ok(())
    }

    pub async fn handle_enact_state(
        &mut self,
        block: &BlockInfo,
        msg: &GovernanceOutcomesMessage,
    ) -> Result<ProtocolParamsMessage> {
        if self.current_era != Some(block.era) {
            self.apply_genesis(&block)?;
        }
        self.current_params.apply_enact_state(msg)?;
        let params_message = ProtocolParamsMessage {
            params: self.current_params.get_params(),
        };

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
