use acropolis_common::{
    messages::EnactStateMessage, Committee, CommitteeChange, ConwayParams, EnactStateElem, Era,
    ProtocolParamUpdate, ProtocolParams,
};
use tracing::error;

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

    fn update_conway_params(c: &mut ConwayParams, p: &ProtocolParamUpdate) {
        Self::upd(&mut c.pool_voting_thresholds, &p.pool_voting_thresholds);
        Self::upd(&mut c.d_rep_voting_thresholds, &p.drep_voting_thresholds);
        Self::upd(&mut c.committee_min_size, &p.min_committee_size);
        Self::upd(
            &mut c.committee_max_term_length,
            &p.committee_term_limit.map(|x| x as u32),
        );
        Self::upd(
            &mut c.d_rep_activity,
            &p.drep_inactivity_period.map(|x| x as u32),
        );
        Self::upd(&mut c.d_rep_deposit, &p.drep_deposit);
        Self::upd(&mut c.gov_action_deposit, &p.governance_action_deposit);
        Self::upd(
            &mut c.gov_action_lifetime,
            &p.governance_action_validity_period.map(|x| x as u32),
        );
        Self::upd(
            &mut c.min_fee_ref_script_cost_per_byte,
            &p.minfee_refscript_cost_per_byte,
        )
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

    fn apply_enact_state_elem(&mut self, u: &EnactStateElem) {
        if let Some(ref mut conway) = self.params.conway {
            match &u {
                EnactStateElem::Params(pu) => Self::update_conway_params(conway, pu),
                EnactStateElem::Constitution(cu) => conway.constitution = cu.clone(),
                EnactStateElem::Committee(cu) => Self::update_committee(&mut conway.committee, cu),
                EnactStateElem::NoConfidence => conway.committee.members.clear(),
            }
        }
    }

    pub fn apply_enact_state(&mut self, u: &EnactStateMessage) {
        for elem in u.enactments.iter() {
            self.apply_enact_state_elem(elem);
        }
    }

    fn upd_empty_dst<T: Clone>(dst: &mut Option<T>, src: &Option<T>) {
        if src.is_some() {
            if dst.is_some() {
                tracing::error!("Genesis update for non-empty parameters, skipping update");
            } else {
                *dst = src.clone();
            }
        }
    }

    pub fn apply_genesis(&mut self, era: &Era, genesis_params: &ProtocolParams) {
        match era {
            Era::Byron => Self::upd_empty_dst(&mut self.params.byron, &genesis_params.byron),
            Era::Shelley => Self::upd_empty_dst(&mut self.params.shelley, &genesis_params.shelley),
            Era::Alonzo => Self::upd_empty_dst(&mut self.params.alonzo, &genesis_params.alonzo),
            Era::Conway => Self::upd_empty_dst(&mut self.params.conway, &genesis_params.conway),
            _ => (), // does not have corresponding genesis params
        }
    }

    pub fn get_params(&self) -> ProtocolParams {
        return self.params.clone();
    }
}
