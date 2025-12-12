//! Acropolis Protocol Params: State storage

use crate::{ParametersUpdater, DEFAULT_NETWORK_NAME};
use acropolis_common::{
    BlockInfo, Era, ProtocolParamUpdate, messages::{
        GovernanceOutcomesMessage, GovernanceProtocolParametersBootstrapMessage, GovernanceProtocolParametersSlice, ProtocolParamsMessage
    }, snapshot::protocol_parameters::ProtocolParameters
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
        let to_apply = Self::genesis_era_range(self.current_era, new_block.era);
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
            self.apply_genesis(block)?;
        }
        self.current_params.apply_enact_state(msg)?;
        let params_message = ProtocolParamsMessage {
            params: self.current_params.get_params(),
        };

        Ok(params_message)
    }

    /// Initialize state from Conway snapshot data
    ///
    /// This method bootstraps the protocol parameters state from a snapshot message.
    /// It converts the protocol parameters from the snapshot into the internal representation
    /// used for tracking parameter changes.
    ///
    /// # Arguments
    ///
    /// * `param_msg` - The bootstrap message containing protocol parameters from the snapshot
    ///
    /// # Behavior
    ///
    /// - Assumes Conway era as the current era
    pub fn bootstrap(&mut self, param_msg: &GovernanceProtocolParametersBootstrapMessage) {
        // we only bootstrap from the present protocol parameters
        if param_msg.slice != GovernanceProtocolParametersSlice::Current {
            return;
        }
        // convert param_msg into current_params
        let network_name = DEFAULT_NETWORK_NAME.0.to_string();
        let era = Era::Conway;
        let mut updater = ParametersUpdater::new();
        let update: ProtocolParamUpdate =
            Self::update_from_protocol_parameters(&network_name, &era, &param_msg.params);
        updater.update_conway_params(&update).unwrap_or_else(|e| {
            tracing::error!("Failed to update Conway params during bootstrap: {}", e);
        });
        self.current_params = updater;
    }

    /// This function transforms a `ProtocolParameters` struct (containing actual values from
    /// a snapshot) into a `ProtocolParamUpdate` struct (where all fields are `Option<T>` for
    /// representing parameter changes, most of them as Some(x)).
    ///
    /// Deprecated fields are set to `None` (lovelace_per_utxo_word, decentralisation_constant, extra_enthropy)
    fn update_from_protocol_parameters(
        _network_name: &str,
        _era: &Era,
        params: &ProtocolParameters,
    ) -> ProtocolParamUpdate {
        // convert params into ProtocolParamUpdate
        ProtocolParamUpdate {
            minfee_a: Some(params.min_fee_a),
            minfee_b: Some(params.min_fee_b),
            max_block_body_size: Some(params.max_block_body_size),
            max_transaction_size: Some(params.max_transaction_size),
            max_block_header_size: Some(params.max_block_header_size as u64),
            key_deposit: Some(params.stake_credential_deposit),
            pool_deposit: Some(params.stake_pool_deposit),
            maximum_epoch: Some(params.stake_pool_max_retirement_epoch),
            desired_number_of_stake_pools: Some(params.optimal_stake_pools_count as u64),
            pool_pledge_influence: Some(acropolis_common::rational_number::RationalNumber::from(
                params.pledge_influence.numerator,
                params.pledge_influence.denominator,
            )),
            expansion_rate: Some(acropolis_common::rational_number::RationalNumber::from(
                params.monetary_expansion_rate.numerator,
                params.monetary_expansion_rate.denominator,
            )),
            treasury_growth_rate: Some(acropolis_common::rational_number::RationalNumber::from(
                params.treasury_expansion_rate.numerator,
                params.treasury_expansion_rate.denominator,
            )),
            min_pool_cost: Some(params.min_pool_cost),
            lovelace_per_utxo_word: None,
            cost_models_for_script_languages: Some(params.cost_models.clone()),
            execution_costs: Some(params.prices.clone()),
            max_tx_ex_units: Some(params.max_tx_ex_units),
            max_block_ex_units: Some(params.max_block_ex_units),
            max_value_size: Some(params.max_value_size),
            collateral_percentage: Some(params.collateral_percentage as u64),
            max_collateral_inputs: Some(params.max_collateral_inputs as u64),
            coins_per_utxo_byte: Some(params.lovelace_per_utxo_byte),
            pool_voting_thresholds: Some(params.pool_voting_thresholds.clone()),
            drep_voting_thresholds: Some(params.drep_voting_thresholds.clone()),
            min_committee_size: Some(params.min_committee_size as u64),
            committee_term_limit: Some(params.max_committee_term_length),
            governance_action_validity_period: Some(params.gov_action_lifetime),
            governance_action_deposit: Some(params.gov_action_deposit),
            drep_deposit: Some(params.drep_deposit),
            drep_inactivity_period: Some(params.drep_expiry),
            minfee_refscript_cost_per_byte: Some(
                acropolis_common::rational_number::RationalNumber::from(
                    params.min_fee_ref_script_lovelace_per_byte.numerator,
                    params.min_fee_ref_script_lovelace_per_byte.denominator,
                ),
            ),
            decentralisation_constant: None,
            extra_enthropy: None,
            protocol_version: Some(params.protocol_version.clone()),
        }
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
