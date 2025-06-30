use anyhow::{anyhow, bail, Result};
use acropolis_common::{
    messages::GovernanceOutcomesMessage, Committee, CommitteeChange,
    ConwayParams, AlonzoParams, ShelleyParams,
    EnactStateElem, Era, ProtocolParamUpdate, ProtocolParams, VotingOutcome
};
use tracing::error;
use crate::genesis_params;

pub struct ParametersUpdater {
    params: ProtocolParams,
}

impl ParametersUpdater {
    pub fn new() -> Self {
        Self {
            params: ProtocolParams::default(),
        }
    }

    fn upd<T: Clone>(dst: &mut T, u: &Option<T>) {
        if let Some(u) = u {
            *dst = (*u).clone();
        }
    }

    fn upd_u32(dst: &mut u32, u: &Option<u64>) -> Result<()> {
        if let Some(u) = u {
            *dst = u32::try_from(*u).or_else(
                |e| Err(anyhow!("Does not fit into u32: {e}"))
            )?
        }
        Ok(())
    }

    fn upd_opt<T: Clone>(dst: &mut Option<T>, u: &Option<T>) {
        if u.is_some() {
            *dst = (*u).clone();
        }
    }

    fn update_conway_params(c: &mut ConwayParams, p: &ProtocolParamUpdate) -> Result<()> {
        Self::upd(&mut c.pool_voting_thresholds, &p.pool_voting_thresholds);
        Self::upd(&mut c.d_rep_voting_thresholds, &p.drep_voting_thresholds);
        Self::upd(&mut c.committee_min_size, &p.min_committee_size);
        Self::upd_u32(&mut c.committee_max_term_length, &p.committee_term_limit)?;
        Self::upd_u32(&mut c.d_rep_activity, &p.drep_inactivity_period)?;
        Self::upd(&mut c.d_rep_deposit, &p.drep_deposit);
        Self::upd(&mut c.gov_action_deposit, &p.governance_action_deposit);
        Self::upd_u32(&mut c.gov_action_lifetime, &p.governance_action_validity_period)?;
        Self::upd(&mut c.min_fee_ref_script_cost_per_byte, &p.minfee_refscript_cost_per_byte);
        Self::upd(
            &mut c.plutus_v3_cost_model,
            &p.cost_models_for_script_languages.as_ref().and_then(|x| x.plutus_v3.clone())
        );
        Ok(())
    }

    fn update_shelley_params(s: &mut ShelleyParams, p: &ProtocolParamUpdate) -> Result<()> {
        let sp = &mut s.protocol_params;
        Self::upd(&mut sp.pool_pledge_influence, &p.pool_pledge_influence);
        Self::upd(&mut sp.monetary_expansion, &p.expansion_rate);
        Self::upd(&mut sp.min_pool_cost, &p.min_pool_cost);
        Self::upd(&mut sp.pool_retire_max_epoch, &p.maximum_epoch);
        Self::upd(&mut sp.key_deposit, &p.key_deposit);
        Self::upd(&mut sp.pool_deposit, &p.pool_deposit);
        Self::upd(&mut sp.treasury_cut, &p.treasury_growth_rate);
        Self::upd_u32(&mut sp.max_block_body_size, &p.max_block_body_size)?;
        Self::upd_u32(&mut sp.max_tx_size, &p.max_transaction_size)?;
        Self::upd_u32(&mut sp.max_block_header_size, &p.max_block_header_size)?;
        Self::upd_u32(&mut sp.minfee_a, &p.minfee_a)?;
        Self::upd_u32(&mut sp.minfee_b, &p.minfee_b)?;
        Self::upd_u32(&mut sp.stake_pool_target_num, &p.desired_number_of_stake_pools)?;
        Ok(())
    }

    fn update_alonzo_params(a: &mut AlonzoParams, p: &ProtocolParamUpdate) -> Result<()> {
        Self::upd_u32(&mut a.max_collateral_inputs, &p.max_collateral_inputs)?;
        Self::upd_u32(&mut a.collateral_percentage, &p.collateral_percentage)?;
        Self::upd_u32(&mut a.max_value_size, &p.max_value_size)?;
        Self::upd(&mut a.execution_prices, &p.execution_costs);
        Self::upd(&mut a.max_tx_ex_units, &p.max_tx_ex_units);
        Self::upd(&mut a.max_block_ex_units, &p.max_block_ex_units);
        Self::upd(&mut a.lovelace_per_utxo_word, &p.ada_per_utxo_byte);
        Self::upd_opt(
            &mut a.plutus_v1_cost_model,
            &p.cost_models_for_script_languages.as_ref().and_then(|x| x.plutus_v1.clone())
        );
        Self::upd_opt(
            &mut a.plutus_v2_cost_model,
            &p.cost_models_for_script_languages.as_ref().and_then(|x| x.plutus_v2.clone())
        );
        Ok(())
    }

    fn update_committee(c: &mut Committee, cu: &CommitteeChange) {
        for removed_member in cu.removed_committee_members.iter() {
            if let None = c.members.remove(removed_member) {
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
        c.threshold = cu.terms.clone();
    }

    fn apply_enact_state_elem(&mut self, (v,u): &(VotingOutcome,EnactStateElem)) -> Result<()> {
        if !v.accepted {
            bail!("Cannot apply not accepted enact state {:?}", (u,v))
        }

        let ref mut alonzo = self.params.alonzo.as_mut().ok_or_else(
            || anyhow!("Alonzo must present for enact state")
        )?;

        let ref mut shelley = self.params.shelley.as_mut().ok_or_else(
            || anyhow!("Shelley must present for enact state")
        )?;

        let ref mut conway = self.params.conway.as_mut().ok_or_else(
            || anyhow!("Conway must present for enact state")
        )?;

        match &u {
            EnactStateElem::Params(pu) => {
                Self::update_alonzo_params(alonzo, pu)?;
                Self::update_shelley_params(shelley, pu)?;
                Self::update_conway_params(conway, pu)?;
            }
            EnactStateElem::Constitution(cu) => conway.constitution = cu.clone(),
            EnactStateElem::Committee(cu) => Self::update_committee(&mut conway.committee, cu),
            EnactStateElem::NoConfidence => conway.committee.members.clear(),
        }

        Ok(())
    }

    pub fn apply_enact_state(&mut self, u: &GovernanceOutcomesMessage) -> Result<()> {
        for elem in u.enact_state.iter() {
            self.apply_enact_state_elem(elem)?;
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

    pub fn apply_genesis(&mut self, era: &Era) -> Result<()> {
        match era {
            Era::Byron =>
                Self::upgen(&mut self.params.byron, &genesis_params::read_byron_genesis()?),
            Era::Shelley =>
                Self::upgen(&mut self.params.shelley, &genesis_params::read_shelley_genesis()?),
            Era::Alonzo =>
                Self::upgen(&mut self.params.alonzo, &genesis_params::read_alonzo_genesis()?),
            Era::Conway =>
                Self::upgen(&mut self.params.conway, &genesis_params::read_conway_genesis()?),
            _ => {
                tracing::info!("Applying genesis: skipping, no genesis exist for {era}");
                Ok(())
            }
        }
    }

    pub fn get_params(&self) -> ProtocolParams {
        return self.params.clone();
    }
}
