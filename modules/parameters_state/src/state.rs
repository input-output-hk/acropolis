//! Acropolis Protocol Params: State storage

use crate::ParametersUpdater;
use acropolis_common::{messages::{
    GovernanceOutcomesMessage,
    GovernanceProtocolParametersBootstrapMessage,
    GovernanceProtocolParametersSlice, ProtocolParamsMessage,
}, protocol_params::ProtocolVersion, snapshot::protocol_parameters::ProtocolParameters, BlockInfo, Era, EnactStateElem, GovernanceOutcome, GovernanceOutcomeVariant, ProtocolParamUpdate, VotingOutcome, AlonzoBabbageVotingOutcome};
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

    pub fn apply_genesis(&mut self, new_era: &Era) -> Result<()> {
        let to_apply = Self::genesis_era_range(self.current_era, *new_era);
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
            new_era,
            self.current_params.get_params()
        );
        self.current_era = Some(*new_era);
        Ok(())
    }

    pub fn apply_governance_outcomes(
        &mut self,
        new_era: &Era,
        alonzo_gov: &Vec<AlonzoBabbageVotingOutcome>,
        conway_gov: &Vec<GovernanceOutcomeVariant>
    ) -> Result<()> {
        info!("Current Era: {:?}", self.current_era);
        if self.current_era != Some(*new_era) {
            self.apply_genesis(new_era)?;
        }
        self.current_params.apply_enact_state(alonzo_gov, conway_gov)
    }

    pub async fn handle_enact_state(
        &mut self,
        new_era: &Era,
        msg: &GovernanceOutcomesMessage,
    ) -> Result<ProtocolParamsMessage> {
        info!("Era: {:?}, applying enact state", new_era);
        self.apply_governance_outcomes(
            new_era,
            &msg.alonzo_babbage_outcomes,
            &msg.conway_outcomes.iter().map(|o| o.action_to_perform.clone()).collect()
        )?;
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
    pub fn bootstrap(&mut self, param_msg: &GovernanceProtocolParametersBootstrapMessage) -> u64 {
        let outcomes = Self::mk_governance_outcomes(&param_msg.params);
        self.network_name = param_msg.network_name.clone();
        if let Err(e) = self.apply_governance_outcomes(&Era::Conway, &vec![], &outcomes) {
            tracing::error!("Parameters application failed: {e}");
        }

        info!(
            "Bootstrapped ParametersState to era {:?} with params: {:?}",
            self.current_era,
            self.current_params.get_params()
        );

        param_msg.epoch
    }

    /// This function transforms a `ProtocolParameters` struct (containing actual values from
    /// a snapshot) into a `ConwayParams` struct used by the parameters updater.
    ///
    /// Note: Constitution and committee are initialized as empty/placeholder values since they
    /// are not included in the ProtocolParameters from the snapshot.
    fn mk_governance_outcomes(params: &ProtocolParameters) -> Vec<GovernanceOutcomeVariant> {
        use acropolis_common::{
            protocol_params::ConwayParams, rational_number::RationalNumber, Anchor, Committee,
            Constitution, CostModel,
        };
        use std::collections::HashMap;

        let mut outcomes = Vec::new();

        // Create placeholder constitution (will be updated by governance events)
        let constitution = Constitution {
            anchor: Anchor {
                url: String::new(),
                data_hash: Vec::new(),
            },
            guardrail_script: None,
        };

        // Create empty committee (will be updated by governance events)
        let committee = Committee {
            members: HashMap::new(),
            threshold: RationalNumber::ZERO,
        };

        // TODO: I believe that we don't need to add these outcomes, if they're empty anyway; default is fine.
        //outcome.push(GovernanceOutcomeVariant::EnactStateElem(EnactStateElem::Constitution(constitution)));
        //outcome.push(GovernanceOutcomeVariant::EnactStateElem(EnactStateElem::Committee(committee)));

        outcomes.push(GovernanceOutcomeVariant::EnactStateElem(EnactStateElem::ProtVer(
            params.protocol_version.clone()
        )));

        let mut param_update = ProtocolParamUpdate::default();
        param_update.minfee_a = Some(params.min_fee_a);
        param_update.minfee_b = Some(params.min_fee_b);
        param_update.collateral_percentage = Some(params.collateral_percentage as u64);
        // TODO: fill other parameters

        outcomes.push(GovernanceOutcomeVariant::EnactStateElem(EnactStateElem::Params(Box::new(param_update))));
/*
        // Get the plutus v3 cost model, or create empty one if not present
        let plutus_v3_cost_model =
            params.cost_models.plutus_v3.clone().unwrap_or_else(|| CostModel::new(Vec::new()));

        ConwayParams {
            pool_voting_thresholds: params.pool_voting_thresholds.clone(),
            d_rep_voting_thresholds: params.drep_voting_thresholds.clone(),
            committee_min_size: params.min_committee_size as u64,
            committee_max_term_length: params.max_committee_term_length as u32,
            gov_action_lifetime: params.gov_action_lifetime as u32,
            gov_action_deposit: params.gov_action_deposit,
            d_rep_deposit: params.drep_deposit,
            d_rep_activity: params.drep_expiry as u32,
            min_fee_ref_script_cost_per_byte: RationalNumber::from(
                params.min_fee_ref_script_lovelace_per_byte.numerator,
                params.min_fee_ref_script_lovelace_per_byte.denominator,
            ),
            plutus_v3_cost_model,
            constitution,
            committee,
        }
*/
        outcomes
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
