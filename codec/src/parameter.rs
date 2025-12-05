use crate::{address::map_stake_credential, utils::*};
use acropolis_common::{
    protocol_params::{Nonce, NonceVariant, ProtocolVersion},
    *,
};
use anyhow::{Result, anyhow, bail};
use pallas_primitives::{
    ProtocolVersion as PallasProtocolVersion, ScriptHash as PallasScriptHash, alonzo, babbage,
    conway,
};
use std::collections::{HashMap, HashSet};

fn map_u32_to_u64(n: Option<u32>) -> Option<u64> {
    n.as_ref().map(|x| *x as u64)
}

fn map_constitution(constitution: &conway::Constitution) -> Constitution {
    Constitution {
        anchor: map_anchor(&constitution.anchor),
        guardrail_script: map_nullable(to_hash, &constitution.guardrail_script),
    }
}

fn map_protocol_version((major, minor): &PallasProtocolVersion) -> ProtocolVersion {
    ProtocolVersion {
        minor: *minor,
        major: *major,
    }
}

fn map_alonzo_single_model(model: &alonzo::CostModel) -> Option<CostModel> {
    Some(CostModel::new(model.clone()))
}

fn map_alonzo_nonce(e: &alonzo::Nonce) -> Nonce {
    Nonce {
        tag: match &e.variant {
            alonzo::NonceVariant::NeutralNonce => NonceVariant::NeutralNonce,
            alonzo::NonceVariant::Nonce => NonceVariant::Nonce,
        },
        hash: e.hash.map(|v| *v),
    }
}

fn map_alonzo_cost_models(pallas_cost_models: &alonzo::CostModels) -> Result<CostModels> {
    let mut res = CostModels {
        plutus_v1: None,
        plutus_v2: None,
        plutus_v3: None,
    };
    for (lang, mdl) in pallas_cost_models.iter() {
        if *lang == alonzo::Language::PlutusV1 {
            res.plutus_v1 = map_alonzo_single_model(mdl);
        } else {
            bail!("Alonzo may not contain {lang:?} language");
        }
    }
    Ok(res)
}

fn map_conway_execution_costs(pallas_ex_costs: &conway::ExUnitPrices) -> ExUnitPrices {
    ExUnitPrices {
        mem_price: map_unit_interval(&pallas_ex_costs.mem_price),
        step_price: map_unit_interval(&pallas_ex_costs.step_price),
    }
}

fn map_conway_cost_models(pallas_cost_models: &conway::CostModels) -> CostModels {
    CostModels {
        plutus_v1: pallas_cost_models.plutus_v1.as_ref().map(|x| CostModel::new(x.clone())),
        plutus_v2: pallas_cost_models.plutus_v2.as_ref().map(|x| CostModel::new(x.clone())),
        plutus_v3: pallas_cost_models.plutus_v3.as_ref().map(|x| CostModel::new(x.clone())),
    }
}

fn map_pool_voting_thresholds(ts: &conway::PoolVotingThresholds) -> PoolVotingThresholds {
    PoolVotingThresholds {
        motion_no_confidence: map_unit_interval(&ts.motion_no_confidence),
        committee_normal: map_unit_interval(&ts.committee_normal),
        committee_no_confidence: map_unit_interval(&ts.committee_no_confidence),
        hard_fork_initiation: map_unit_interval(&ts.hard_fork_initiation),
        security_voting_threshold: map_unit_interval(&ts.security_voting_threshold),
    }
}

fn map_drep_voting_thresholds(ts: &conway::DRepVotingThresholds) -> DRepVotingThresholds {
    DRepVotingThresholds {
        motion_no_confidence: map_unit_interval(&ts.motion_no_confidence),
        committee_normal: map_unit_interval(&ts.committee_normal),
        committee_no_confidence: map_unit_interval(&ts.committee_no_confidence),
        update_constitution: map_unit_interval(&ts.update_constitution),
        hard_fork_initiation: map_unit_interval(&ts.hard_fork_initiation),
        pp_network_group: map_unit_interval(&ts.pp_network_group),
        pp_economic_group: map_unit_interval(&ts.pp_economic_group),
        pp_technical_group: map_unit_interval(&ts.pp_technical_group),
        pp_governance_group: map_unit_interval(&ts.pp_governance_group),
        treasury_withdrawal: map_unit_interval(&ts.treasury_withdrawal),
    }
}

pub fn map_alonzo_protocol_param_update(
    p: &alonzo::ProtocolParamUpdate,
) -> Result<Box<ProtocolParamUpdate>> {
    Ok(Box::new(ProtocolParamUpdate {
        // Fields, common for Conway and Alonzo-compatible
        minfee_a: map_u32_to_u64(p.minfee_a),
        minfee_b: map_u32_to_u64(p.minfee_b),
        max_block_body_size: map_u32_to_u64(p.max_block_body_size),
        max_transaction_size: map_u32_to_u64(p.max_transaction_size),
        max_block_header_size: map_u32_to_u64(p.max_block_header_size),
        key_deposit: p.key_deposit,
        pool_deposit: p.pool_deposit,
        maximum_epoch: p.maximum_epoch,
        desired_number_of_stake_pools: map_u32_to_u64(p.desired_number_of_stake_pools),
        pool_pledge_influence: p.pool_pledge_influence.as_ref().map(&map_unit_interval),
        expansion_rate: p.expansion_rate.as_ref().map(&map_unit_interval),
        treasury_growth_rate: p.treasury_growth_rate.as_ref().map(&map_unit_interval),
        min_pool_cost: p.min_pool_cost,
        lovelace_per_utxo_word: p.ada_per_utxo_byte, // Pre Babbage (Represents cost per 8-byte word)
        coins_per_utxo_byte: None,
        cost_models_for_script_languages: p
            .cost_models_for_script_languages
            .as_ref()
            .map(&map_alonzo_cost_models)
            .transpose()?,
        execution_costs: p.execution_costs.as_ref().map(&map_execution_costs),
        max_tx_ex_units: p.max_tx_ex_units.as_ref().map(&map_ex_units),
        max_block_ex_units: p.max_block_ex_units.as_ref().map(&map_ex_units),
        max_value_size: map_u32_to_u64(p.max_value_size),
        collateral_percentage: map_u32_to_u64(p.collateral_percentage),
        max_collateral_inputs: map_u32_to_u64(p.max_collateral_inputs),

        // Fields, specific for Conway
        pool_voting_thresholds: None,
        drep_voting_thresholds: None,
        min_committee_size: None,
        committee_term_limit: None,
        governance_action_validity_period: None,
        governance_action_deposit: None,
        drep_deposit: None,
        drep_inactivity_period: None,
        minfee_refscript_cost_per_byte: None,

        // Fields, specific for Alonzo-compatible (Alonzo, Babbage, Shelley)
        decentralisation_constant: p.decentralization_constant.as_ref().map(&map_unit_interval),
        extra_enthropy: p.extra_entropy.as_ref().map(&map_alonzo_nonce),
        protocol_version: p.protocol_version.as_ref().map(map_protocol_version),
    }))
}

fn map_babbage_cost_models(cost_models: &babbage::CostModels) -> CostModels {
    CostModels {
        plutus_v1: cost_models.plutus_v1.as_ref().map(|p| CostModel::new(p.clone())),
        plutus_v2: cost_models.plutus_v2.as_ref().map(|p| CostModel::new(p.clone())),
        plutus_v3: None,
    }
}

pub fn map_babbage_protocol_param_update(
    p: &babbage::ProtocolParamUpdate,
) -> Result<Box<ProtocolParamUpdate>> {
    Ok(Box::new(ProtocolParamUpdate {
        // Fields, common for Conway and Alonzo-compatible
        minfee_a: map_u32_to_u64(p.minfee_a),
        minfee_b: map_u32_to_u64(p.minfee_b),
        max_block_body_size: map_u32_to_u64(p.max_block_body_size),
        max_transaction_size: map_u32_to_u64(p.max_transaction_size),
        max_block_header_size: map_u32_to_u64(p.max_block_header_size),
        key_deposit: p.key_deposit,
        pool_deposit: p.pool_deposit,
        maximum_epoch: p.maximum_epoch,
        desired_number_of_stake_pools: map_u32_to_u64(p.desired_number_of_stake_pools),
        pool_pledge_influence: p.pool_pledge_influence.as_ref().map(&map_unit_interval),
        expansion_rate: p.expansion_rate.as_ref().map(&map_unit_interval),
        treasury_growth_rate: p.treasury_growth_rate.as_ref().map(&map_unit_interval),
        min_pool_cost: p.min_pool_cost,
        lovelace_per_utxo_word: None,
        coins_per_utxo_byte: p.ada_per_utxo_byte,
        cost_models_for_script_languages: p
            .cost_models_for_script_languages
            .as_ref()
            .map(&map_babbage_cost_models),
        execution_costs: p.execution_costs.as_ref().map(&map_execution_costs),
        max_tx_ex_units: p.max_tx_ex_units.as_ref().map(&map_ex_units),
        max_block_ex_units: p.max_block_ex_units.as_ref().map(&map_ex_units),
        max_value_size: map_u32_to_u64(p.max_value_size),
        collateral_percentage: map_u32_to_u64(p.collateral_percentage),
        max_collateral_inputs: map_u32_to_u64(p.max_collateral_inputs),

        // Fields, specific for Conway
        pool_voting_thresholds: None,
        drep_voting_thresholds: None,
        min_committee_size: None,
        committee_term_limit: None,
        governance_action_validity_period: None,
        governance_action_deposit: None,
        drep_deposit: None,
        drep_inactivity_period: None,
        minfee_refscript_cost_per_byte: None,

        // Fields not found in Babbage
        decentralisation_constant: None,
        extra_enthropy: None,
        // Fields, specific for Alonzo-compatible (Alonzo, Babbage, Shelley)
        protocol_version: p.protocol_version.as_ref().map(map_protocol_version),
    }))
}

fn map_conway_protocol_param_update(p: &conway::ProtocolParamUpdate) -> Box<ProtocolParamUpdate> {
    Box::new(ProtocolParamUpdate {
        // Fields, common for Conway and Alonzo-compatible
        minfee_a: p.minfee_a,
        minfee_b: p.minfee_b,
        max_block_body_size: p.max_block_body_size,
        max_transaction_size: p.max_transaction_size,
        max_block_header_size: p.max_block_header_size,
        key_deposit: p.key_deposit,
        pool_deposit: p.pool_deposit,
        maximum_epoch: p.maximum_epoch,
        desired_number_of_stake_pools: p.desired_number_of_stake_pools,
        pool_pledge_influence: p.pool_pledge_influence.as_ref().map(&map_unit_interval),
        expansion_rate: p.expansion_rate.as_ref().map(&map_unit_interval),
        treasury_growth_rate: p.treasury_growth_rate.as_ref().map(&map_unit_interval),
        min_pool_cost: p.min_pool_cost,
        coins_per_utxo_byte: p.ada_per_utxo_byte,
        lovelace_per_utxo_word: None,
        cost_models_for_script_languages: p
            .cost_models_for_script_languages
            .as_ref()
            .map(&map_conway_cost_models),
        execution_costs: p.execution_costs.as_ref().map(&map_conway_execution_costs),
        max_tx_ex_units: p.max_tx_ex_units.as_ref().map(&map_ex_units),
        max_block_ex_units: p.max_block_ex_units.as_ref().map(&map_ex_units),
        max_value_size: p.max_value_size,
        collateral_percentage: p.collateral_percentage,
        max_collateral_inputs: p.max_collateral_inputs,

        // Fields, specific for Conway
        pool_voting_thresholds: p.pool_voting_thresholds.as_ref().map(&map_pool_voting_thresholds),
        drep_voting_thresholds: p.drep_voting_thresholds.as_ref().map(&map_drep_voting_thresholds),
        min_committee_size: p.min_committee_size,
        committee_term_limit: p.committee_term_limit,
        governance_action_validity_period: p.governance_action_validity_period,
        governance_action_deposit: p.governance_action_deposit,
        drep_deposit: p.drep_deposit,
        drep_inactivity_period: p.drep_inactivity_period,
        minfee_refscript_cost_per_byte: p
            .minfee_refscript_cost_per_byte
            .as_ref()
            .map(&map_unit_interval),

        // Fields, missing from Conway
        decentralisation_constant: None,
        extra_enthropy: None,
        protocol_version: None,
    })
}

fn map_governance_action(action: &conway::GovAction) -> Result<GovernanceAction> {
    match action {
        conway::GovAction::ParameterChange(id, protocol_update, script) => {
            Ok(GovernanceAction::ParameterChange(ParameterChangeAction {
                previous_action_id: map_nullable_gov_action_id(id)?,
                protocol_param_update: map_conway_protocol_param_update(protocol_update),
                script_hash: map_nullable(|x: &PallasScriptHash| x.to_vec(), script),
            }))
        }

        conway::GovAction::HardForkInitiation(id, version) => Ok(
            GovernanceAction::HardForkInitiation(HardForkInitiationAction {
                previous_action_id: map_nullable_gov_action_id(id)?,
                protocol_version: ProtocolVersion::new(version.0, version.1),
            }),
        ),

        conway::GovAction::TreasuryWithdrawals(withdrawals, script) => Ok(
            GovernanceAction::TreasuryWithdrawals(TreasuryWithdrawalsAction {
                rewards: HashMap::from_iter(
                    withdrawals.iter().map(|(account, coin)| (account.to_vec(), *coin)),
                ),
                script_hash: map_nullable(|x: &PallasScriptHash| x.to_vec(), script),
            }),
        ),

        conway::GovAction::NoConfidence(id) => Ok(GovernanceAction::NoConfidence(
            map_nullable_gov_action_id(id)?,
        )),

        conway::GovAction::UpdateCommittee(id, committee, threshold, terms) => {
            Ok(GovernanceAction::UpdateCommittee(UpdateCommitteeAction {
                previous_action_id: map_nullable_gov_action_id(id)?,
                data: CommitteeChange {
                    removed_committee_members: HashSet::from_iter(
                        committee.iter().map(map_stake_credential),
                    ),
                    new_committee_members: HashMap::from_iter(
                        threshold.iter().map(|(k, v)| (map_stake_credential(k), *v)),
                    ),
                    terms: map_unit_interval(terms),
                },
            }))
        }

        conway::GovAction::NewConstitution(id, constitution) => {
            Ok(GovernanceAction::NewConstitution(NewConstitutionAction {
                previous_action_id: map_nullable_gov_action_id(id)?,
                new_constitution: map_constitution(constitution),
            }))
        }

        conway::GovAction::Information => Ok(GovernanceAction::Information),
    }
}

pub fn map_governance_proposals_procedures(
    gov_action_id: &GovActionId,
    prop: &conway::ProposalProcedure,
) -> Result<ProposalProcedure> {
    Ok(ProposalProcedure {
        deposit: prop.deposit,
        reward_account: StakeAddress::from_binary(&prop.reward_account)?,
        gov_action_id: gov_action_id.clone(),
        gov_action: map_governance_action(&prop.gov_action)?,
        anchor: map_anchor(&prop.anchor),
    })
}

fn map_voter(voter: &conway::Voter) -> Voter {
    match voter {
        conway::Voter::ConstitutionalCommitteeKey(key_hash) => {
            Voter::ConstitutionalCommitteeKey(to_hash(key_hash).into())
        }
        conway::Voter::ConstitutionalCommitteeScript(script_hash) => {
            Voter::ConstitutionalCommitteeScript(to_hash(script_hash).into())
        }
        conway::Voter::DRepKey(addr_key_hash) => Voter::DRepKey(to_hash(addr_key_hash).into()),
        conway::Voter::DRepScript(script_hash) => Voter::DRepScript(to_hash(script_hash).into()),
        conway::Voter::StakePoolKey(key_hash) => Voter::StakePoolKey(to_pool_id(key_hash)),
    }
}

fn map_vote(vote: &conway::Vote) -> Vote {
    match vote {
        conway::Vote::No => Vote::No,
        conway::Vote::Yes => Vote::Yes,
        conway::Vote::Abstain => Vote::Abstain,
    }
}

fn map_single_governance_voting_procedure(
    vote_index: u32,
    proc: &conway::VotingProcedure,
) -> VotingProcedure {
    VotingProcedure {
        vote: map_vote(&proc.vote),
        anchor: map_nullable_anchor(&proc.anchor),
        vote_index,
    }
}

pub fn map_all_governance_voting_procedures(
    vote_procs: &conway::VotingProcedures,
) -> Result<VotingProcedures> {
    let mut procs = VotingProcedures {
        votes: HashMap::new(),
    };

    for (pallas_voter, pallas_pair) in vote_procs.iter() {
        let voter = map_voter(pallas_voter);

        if let Some(existing) = procs.votes.insert(voter.clone(), SingleVoterVotes::default()) {
            bail!("Duplicate voter {voter:?}: procedure {vote_procs:?}, existing {existing:?}");
        }

        let single_voter = procs
            .votes
            .get_mut(&voter)
            .ok_or_else(|| anyhow!("Cannot find voter {:?}, which must present", voter))?;

        for (vote_index, (pallas_action_id, pallas_voting_procedure)) in
            pallas_pair.iter().enumerate()
        {
            let action_id = map_gov_action_id(pallas_action_id)?;
            let vp =
                map_single_governance_voting_procedure(vote_index as u32, pallas_voting_procedure);
            single_voter.voting_procedures.insert(action_id, vp);
        }
    }

    Ok(procs)
}
