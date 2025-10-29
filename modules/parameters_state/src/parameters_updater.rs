use crate::genesis_params;
use acropolis_common::protocol_params::{
    AlonzoParams, BabbageParams, ConwayParams, ProtocolParams, ShelleyProtocolParams,
};
use acropolis_common::{
    messages::GovernanceOutcomesMessage, AlonzoBabbageVotingOutcome, Committee, CommitteeChange,
    EnactStateElem, Era, GovernanceOutcomeVariant, ProtocolParamUpdate,
};
use anyhow::{anyhow, bail, Result};
use tracing::error;

#[derive(Default, Clone)]
pub struct ParametersUpdater {
    params: ProtocolParams,
}

impl ParametersUpdater {
    pub fn new() -> Self {
        Self {
            params: ProtocolParams::default(),
        }
    }

    //
    // Conway parameters update
    //

    fn cw_upd<T: Clone>(
        &mut self,
        f: impl Fn(&mut ConwayParams) -> &mut T,
        u: &Option<T>,
    ) -> Result<()> {
        if let Some(u) = u {
            match &mut self.params.conway {
                Some(dst) => *f(dst) = (*u).clone(),
                None => bail!("Conway parameter file must be set in genesis before updating"),
            }
        }
        Ok(())
    }

    fn cw_u32(&mut self, f: impl Fn(&mut ConwayParams) -> &mut u32, u: &Option<u64>) -> Result<()> {
        self.cw_upd(f, &u.map(u32::try_from).transpose()?)
    }

    fn update_conway_params(&mut self, p: &ProtocolParamUpdate) -> Result<()> {
        self.cw_upd(|c| &mut c.pool_voting_thresholds, &p.pool_voting_thresholds)?;
        self.cw_upd(
            |c| &mut c.d_rep_voting_thresholds,
            &p.drep_voting_thresholds,
        )?;
        self.cw_upd(|c| &mut c.committee_min_size, &p.min_committee_size)?;
        self.cw_u32(
            |c| &mut c.committee_max_term_length,
            &p.committee_term_limit,
        )?;
        self.cw_u32(|c| &mut c.d_rep_activity, &p.drep_inactivity_period)?;
        self.cw_upd(|c| &mut c.d_rep_deposit, &p.drep_deposit)?;
        self.cw_upd(|c| &mut c.gov_action_deposit, &p.governance_action_deposit)?;
        self.cw_u32(
            |c| &mut c.gov_action_lifetime,
            &p.governance_action_validity_period,
        )?;
        self.cw_upd(
            |c| &mut c.min_fee_ref_script_cost_per_byte,
            &p.minfee_refscript_cost_per_byte,
        )?;
        self.cw_upd(
            |c| &mut c.plutus_v3_cost_model,
            &p.cost_models_for_script_languages.as_ref().and_then(|x| x.plutus_v3.clone()),
        )
    }

    //
    // Babbage parameters update
    //

    fn bab_upd<T: Clone>(
        &mut self,
        f: impl Fn(&mut BabbageParams) -> &mut T,
        u: &Option<T>,
    ) -> Result<()> {
        if let Some(u) = u {
            match &mut self.params.babbage {
                Some(dst) => *f(dst) = u.clone(),
                None => bail!("Babbage must be initalized before updating"),
            }
        }
        Ok(())
    }

    fn bab_opt<T: Clone>(
        &mut self,
        f: impl Fn(&mut BabbageParams) -> &mut Option<T>,
        u: &Option<T>,
    ) -> Result<()> {
        self.bab_upd(f, &u.as_ref().map(|x| Some(x.clone())))
    }

    fn update_babbage_params(&mut self, p: &ProtocolParamUpdate) -> Result<()> {
        self.bab_upd(|b| &mut b.coins_per_utxo_byte, &p.coins_per_utxo_byte)?;
        self.bab_opt(
            |b| &mut b.plutus_v2_cost_model,
            &p.cost_models_for_script_languages.as_ref().and_then(|x| x.plutus_v2.clone()),
        )?;
        Ok(())
    }

    //
    // Shelley parameters update
    //

    fn sh_upd<T: Clone>(
        &mut self,
        f: impl Fn(&mut ShelleyProtocolParams) -> &mut T,
        u: &Option<T>,
    ) -> Result<()> {
        if let Some(u) = u {
            match &mut self.params.shelley {
                Some(dst) => *f(&mut dst.protocol_params) = (*u).clone(),
                None => bail!("Shelley parameter file must be set in genesis before updating"),
            }
        }
        Ok(())
    }

    fn sh_u32(
        &mut self,
        f: impl Fn(&mut ShelleyProtocolParams) -> &mut u32,
        u: &Option<u64>,
    ) -> Result<()> {
        self.sh_upd(f, &u.map(u32::try_from).transpose()?)
    }

    fn update_shelley_params(&mut self, p: &ProtocolParamUpdate) -> Result<()> {
        self.sh_upd(|sp| &mut sp.pool_pledge_influence, &p.pool_pledge_influence)?;
        self.sh_upd(|sp| &mut sp.monetary_expansion, &p.expansion_rate)?;
        self.sh_upd(|sp| &mut sp.min_pool_cost, &p.min_pool_cost)?;
        self.sh_upd(|sp| &mut sp.pool_retire_max_epoch, &p.maximum_epoch)?;
        self.sh_upd(|sp| &mut sp.key_deposit, &p.key_deposit)?;
        self.sh_upd(|sp| &mut sp.pool_deposit, &p.pool_deposit)?;
        self.sh_upd(|sp| &mut sp.treasury_cut, &p.treasury_growth_rate)?;
        self.sh_u32(|sp| &mut sp.max_block_body_size, &p.max_block_body_size)?;
        self.sh_u32(|sp| &mut sp.max_tx_size, &p.max_transaction_size)?;
        self.sh_u32(|sp| &mut sp.max_block_header_size, &p.max_block_header_size)?;
        self.sh_u32(|sp| &mut sp.minfee_a, &p.minfee_a)?;
        self.sh_u32(|sp| &mut sp.minfee_b, &p.minfee_b)?;
        self.sh_u32(
            |sp| &mut sp.stake_pool_target_num,
            &p.desired_number_of_stake_pools,
        )?;
        self.sh_upd(
            |sp| &mut sp.decentralisation_param,
            &p.decentralisation_constant,
        )?;
        self.sh_upd(|sp| &mut sp.protocol_version, &p.protocol_version)?;
        self.sh_upd(|sp| &mut sp.extra_entropy, &p.extra_enthropy)?;
        Ok(())
    }

    //
    // Alonzo parameters update
    //

    fn a_upd<T: Clone>(
        &mut self,
        f: impl Fn(&mut AlonzoParams) -> &mut T,
        u: &Option<T>,
    ) -> Result<()> {
        if let Some(u) = u {
            match &mut self.params.alonzo {
                Some(dst) => *f(dst) = (*u).clone(),
                None => bail!("Alonzo parameter file must be set in genesis before updating"),
            }
        }
        Ok(())
    }

    fn a_opt<T: Clone>(
        &mut self,
        f: impl Fn(&mut AlonzoParams) -> &mut Option<T>,
        u: &Option<T>,
    ) -> Result<()> {
        self.a_upd(f, &u.as_ref().map(|x| Some(x.clone())))
    }

    fn a_u32(&mut self, f: impl Fn(&mut AlonzoParams) -> &mut u32, u: &Option<u64>) -> Result<()> {
        self.a_upd(f, &u.map(u32::try_from).transpose()?)
    }

    fn update_alonzo_params(&mut self, p: &ProtocolParamUpdate) -> Result<()> {
        self.a_u32(|a| &mut a.max_collateral_inputs, &p.max_collateral_inputs)?;
        self.a_u32(|a| &mut a.collateral_percentage, &p.collateral_percentage)?;
        self.a_u32(|a| &mut a.max_value_size, &p.max_value_size)?;
        self.a_upd(|a| &mut a.execution_prices, &p.execution_costs)?;
        self.a_upd(|a| &mut a.max_tx_ex_units, &p.max_tx_ex_units)?;
        self.a_upd(|a| &mut a.max_block_ex_units, &p.max_block_ex_units)?;
        self.a_upd(|a| &mut a.lovelace_per_utxo_word, &p.lovelace_per_utxo_word)?;
        self.a_opt(
            |a| &mut a.plutus_v1_cost_model,
            &p.cost_models_for_script_languages.as_ref().and_then(|x| x.plutus_v1.clone()),
        )
    }

    //
    // General update procs
    //

    fn update_params(&mut self, pu: &ProtocolParamUpdate) -> Result<()> {
        self.update_alonzo_params(pu)?;
        self.update_shelley_params(pu)?;
        self.update_babbage_params(pu)?;
        self.update_conway_params(pu)
    }

    fn update_committee(c: &mut Committee, cu: &CommitteeChange) {
        for removed_member in cu.removed_committee_members.iter() {
            if c.members.remove(removed_member).is_none() {
                error!(
                    "Removing {:?}, which is not a part of the committee",
                    removed_member
                );
            }
        }
        for (new_member, v) in cu.new_committee_members.iter() {
            if let Some(old) = c.members.insert(new_member.clone(), *v) {
                error!(
                    "New committee member {:?} replaces the old committee member {:?}",
                    (new_member, v),
                    old
                );
            }
        }
        c.threshold = cu.terms;
    }

    fn apply_alonzo_babbage_outcome_elem(&mut self, u: &AlonzoBabbageVotingOutcome) -> Result<()> {
        if u.accepted {
            self.update_params(&u.parameter_update)?;
        }
        Ok(())
    }

    fn apply_enact_state_elem(&mut self, u: &EnactStateElem) -> Result<()> {
        let c = &mut (self
            .params
            .conway
            .as_mut()
            .ok_or_else(|| anyhow!("Conway must present for enact state"))?);

        match &u {
            EnactStateElem::Params(pu) => self.update_params(pu)?,
            EnactStateElem::Constitution(cu) => c.constitution = cu.clone(),
            EnactStateElem::Committee(cu) => Self::update_committee(&mut c.committee, cu),
            EnactStateElem::NoConfidence => c.committee.members.clear(),
            EnactStateElem::ProtVer(pv) => {
                self.sh_upd(|sp| &mut sp.protocol_version, &Some(pv.clone()))?
            }
        }

        Ok(())
    }

    pub fn apply_enact_state(&mut self, u: &GovernanceOutcomesMessage) -> Result<()> {
        for outcome in u.alonzo_babbage_outcomes.iter() {
            tracing::info!("Updating alonzo/babbage outcome {:?}", outcome);
            self.apply_alonzo_babbage_outcome_elem(outcome)?;
        }

        for outcome in u.conway_outcomes.iter() {
            if let GovernanceOutcomeVariant::EnactStateElem(elem) = &outcome.action_to_perform {
                self.apply_enact_state_elem(elem)?;
            }
        }
        Ok(())
    }

    fn upgen<T: Clone>(dst: &mut Option<T>, src: &T) -> Result<()> {
        if dst.is_some() {
            bail!("Destination parameter is not None, skipping applying genesis");
        }
        *dst = Some(src.clone());
        Ok(())
    }

    pub fn apply_genesis(&mut self, network: &str, era: &Era) -> Result<()> {
        match era {
            Era::Byron => Self::upgen(
                &mut self.params.byron,
                &genesis_params::read_byron_genesis(network)?,
            ),
            Era::Shelley => Self::upgen(
                &mut self.params.shelley,
                &genesis_params::read_shelley_genesis(network)?,
            ),
            Era::Alonzo => Self::upgen(
                &mut self.params.alonzo,
                &genesis_params::read_alonzo_genesis(network)?,
            ),
            Era::Babbage => Self::upgen(
                &mut self.params.babbage,
                &genesis_params::apply_babbage_transition(self.params.alonzo.as_ref())?,
            ),
            Era::Conway => Self::upgen(
                &mut self.params.conway,
                &genesis_params::read_conway_genesis(network)?,
            ),
            _ => {
                tracing::info!("Applying genesis: skipping, no genesis exist for {network} {era}");
                Ok(())
            }
        }
    }

    pub fn get_params(&self) -> ProtocolParams {
        self.params.clone()
    }
}
