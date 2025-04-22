//! Acropolis transaction unpacker module for Caryatid
//! Unpacks transaction bodies into UTXO events

use std::{collections::HashMap, sync::Arc};
use std::collections::HashSet;
use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{
    messages::{GovernanceProceduresMessage, Message, TxCertificatesMessage, UTXODeltasMessage}, *
};

use anyhow::{anyhow, Result};
use config::Config;
use tracing::{debug, info, error};
use pallas::{
    ledger::*,
    ledger::{
        primitives::{
            alonzo, conway, Nullable, Relay as PallasRelay, StakeCredential as PallasStakeCredential,
            ScriptHash
        },
        traverse::{MultiEraCert, MultiEraTx},
    }
};

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.txs";

/// Tx unpacker module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "tx-unpacker",
    description = "Transaction to UTXO event unpacker"
)]
pub struct TxUnpacker;

impl TxUnpacker
{
    /// Map Pallas Network to our AddressNetwork
    fn map_network(network: addresses::Network) -> Result<AddressNetwork> {
        match network {
            addresses::Network::Mainnet => Ok(AddressNetwork::Main),
            addresses::Network::Testnet => Ok(AddressNetwork::Test),
            _ => return Err(anyhow!("Unknown network in address"))
        }
    }

    /// Derive our Address from a Pallas address
    // This is essentially a 1:1 mapping but makes the Message definitions independent
    // of Pallas
    fn map_address(address: &addresses::Address) -> Result<Address> {
        match address {
            addresses::Address::Byron(byron_address) => Ok(Address::Byron(ByronAddress {
                payload: byron_address.payload.to_vec(),
            })),

            addresses::Address::Shelley(shelley_address) => Ok(Address::Shelley(ShelleyAddress {
                network: Self::map_network(shelley_address.network())?, 

                payment: match shelley_address.payment() {
                    addresses::ShelleyPaymentPart::Key(hash) => 
                        ShelleyAddressPaymentPart::PaymentKeyHash(hash.to_vec()),
                    addresses::ShelleyPaymentPart::Script(hash) => 
                        ShelleyAddressPaymentPart::ScriptHash(hash.to_vec()),

                },

                delegation: match shelley_address.delegation() {
                    addresses::ShelleyDelegationPart::Null =>
                        ShelleyAddressDelegationPart::None,
                    addresses::ShelleyDelegationPart::Key(hash) =>
                        ShelleyAddressDelegationPart::StakeKeyHash(hash.to_vec()),
                    addresses::ShelleyDelegationPart::Script(hash) =>
                        ShelleyAddressDelegationPart::ScriptHash(hash.to_vec()),
                    addresses::ShelleyDelegationPart::Pointer(pointer) =>
                        ShelleyAddressDelegationPart::Pointer(ShelleyAddressPointer {
                            slot: pointer.slot(),
                            tx_index: pointer.tx_idx(),
                            cert_index: pointer.cert_idx()
                        })
                }
            })),

            addresses::Address::Stake(stake_address) => Ok(Address::Stake(StakeAddress {
                network: Self::map_network(stake_address.network())?,
                payload: match stake_address.payload() {
                    addresses::StakePayload::Stake(hash) => 
                        StakeAddressPayload::StakeKeyHash(hash.to_vec()),
                    addresses::StakePayload::Script(hash) => 
                        StakeAddressPayload::ScriptHash(hash.to_vec()),
                }
            })),

        }
    }

    /// Map a Pallas StakeCredential to ours
    fn map_stake_credential(cred: &PallasStakeCredential) -> StakeCredential {
        match cred {
            PallasStakeCredential::AddrKeyhash(key_hash) =>
                StakeCredential::AddrKeyHash(key_hash.to_vec()),
            PallasStakeCredential::ScriptHash(script_hash) =>
                StakeCredential::ScriptHash(script_hash.to_vec()),
        }
    }

    /// Map a Pallas DRep to our DRepChoice
    fn map_drep(drep: &conway::DRep) -> DRepChoice {
        match drep {
            conway::DRep::Key(key_hash) => DRepChoice::Key(key_hash.to_vec()),
            conway::DRep::Script(script_hash) => DRepChoice::Script(script_hash.to_vec()),
            conway::DRep::Abstain => DRepChoice::Abstain,
            conway::DRep::NoConfidence => DRepChoice::NoConfidence,
        }
    }

    fn map_nullable<Src: Clone,Dst>(f: impl FnOnce(&Src) -> Dst, nullable_src: &Nullable<Src>) -> Option<Dst> {
        match nullable_src {
            Nullable::Some(src) => Some(f(src)),
            _ => None
        }
    }

    fn map_nullable_result<Src: Clone,Dst>(f: impl FnOnce(&Src) -> Result<Dst>, nullable_src: &Nullable<Src>) -> Result<Option<Dst>> {
        match nullable_src {
            Nullable::Some(src) => { let res = f(src)?; Ok(Some(res)) }
            _ => Ok(None)
        }
    }

    fn map_anchor(anchor: &conway::Anchor) -> Anchor {
        Anchor {
            url: anchor.url.clone(),
            data_hash: anchor.content_hash.to_vec(),
        }
    }

    /// Map a Nullable Anchor to ours
    fn map_nullable_anchor(anchor: &Nullable<conway::Anchor>) -> Option<Anchor> {
        Self::map_nullable(&Self::map_anchor, anchor)
    }

    fn map_gov_action_id(pallas_action_id: &conway::GovActionId) -> Result<GovActionId> {
        let act_idx_u8: u8 = match pallas_action_id.action_index.try_into() {
            Ok(v) => v,
            Err(e) => return Err(anyhow!("Invalid action index {e}"))
        };

        Ok(GovActionId {
            transaction_id: pallas_action_id.transaction_id.to_vec(),
            action_index: act_idx_u8,
        })
    }

    fn map_nullable_gov_action_id(id: &Nullable<conway::GovActionId>) -> Result<Option<GovActionId>> {
        Self::map_nullable_result(&Self::map_gov_action_id, id)
    }

    /// Map a Pallas Relay to ours
    fn map_relay(relay: &PallasRelay) -> Relay {
        match relay {
            PallasRelay::SingleHostAddr(port, ipv4, ipv6) =>
                Relay::SingleHostAddr(SingleHostAddr{
                    port: match port {
                        Nullable::Some(port) => Some(*port as u16),
                        _ => None,
                    }, 
                    ipv4: match ipv4 {
                        Nullable::Some(ipv4) => ipv4.try_into().ok(),
                        _ => None,
                    }, 
                    ipv6: match ipv6 {
                        Nullable::Some(ipv6) => ipv6.try_into().ok(),
                        _ => None,
                    }, 
                }),
            PallasRelay::SingleHostName(port, dns_name) =>
                Relay::SingleHostName(SingleHostName { 
                    port: match port {
                        Nullable::Some(port) => Some(*port as u16),
                        _ => None,
                    }, 
                    dns_name: dns_name.clone(),
                }),
            PallasRelay::MultiHostName(dns_name) =>
                Relay::MultiHostName(MultiHostName { 
                    dns_name: dns_name.clone()
                }),
        }
    }

    /// Derive our TxCertificate from a Pallas Certificate
    fn map_certificate(cert: &MultiEraCert) -> Result<TxCertificate> {
        match cert {
            MultiEraCert::NotApplicable => Err(anyhow!("Not applicable cert!")),

            MultiEraCert::AlonzoCompatible(cert) => {
                match cert.as_ref().as_ref() {
                    alonzo::Certificate::StakeRegistration(cred) =>
                        Ok(TxCertificate::StakeRegistration(
                            Self::map_stake_credential(cred))),
                    alonzo::Certificate::StakeDeregistration(cred) =>
                            Ok(TxCertificate::StakeDeregistration(
                                Self::map_stake_credential(cred))),
                    alonzo::Certificate::StakeDelegation(cred, pool_key_hash) =>
                                Ok(TxCertificate::StakeDelegation(StakeDelegation {
                                    credential: Self::map_stake_credential(cred),
                                    operator: pool_key_hash.to_vec()
                                })),
                    alonzo::Certificate::PoolRegistration { 
                        // TODO relays, pool_metadata
                        operator, vrf_keyhash, pledge, cost, margin, 
                        reward_account, pool_owners, relays, pool_metadata } =>
                                Ok(TxCertificate::PoolRegistration(PoolRegistration { 
                                    operator: operator.to_vec(), 
                                    vrf_key_hash: vrf_keyhash.to_vec(),
                                    pledge: *pledge,
                                    cost: *cost,
                                    margin: Ratio {
                                        numerator: margin.numerator,
                                        denominator: margin.denominator,
                                    },
                                    reward_account: reward_account.to_vec(),
                                    pool_owners: pool_owners
                                        .into_iter()
                                        .map(|v| v.to_vec())
                                        .collect(),
                                    relays: relays
                                        .into_iter()
                                        .map(|relay| Self::map_relay(relay))
                                        .collect(),
                                    pool_metadata: match pool_metadata {
                                        Nullable::Some(md) => Some(PoolMetadata {
                                            url: md.url.clone(),
                                            hash: md.hash.to_vec(),
                                        }),
                                        _ => None
                                    }
                                })),
                    alonzo::Certificate::PoolRetirement(pool_key_hash, epoch) =>
                                Ok(TxCertificate::PoolRetirement(PoolRetirement {
                                    operator: pool_key_hash.to_vec(), 
                                    epoch: *epoch
                                })),
                    alonzo::Certificate::GenesisKeyDelegation(
                        genesis_hash, genesis_delegate_hash, vrf_key_hash) =>
                                Ok(TxCertificate::GenesisKeyDelegation(GenesisKeyDelegation{
                                    genesis_hash: genesis_hash.to_vec(),
                                    genesis_delegate_hash: genesis_delegate_hash.to_vec(),
                                    vrf_key_hash: vrf_key_hash.to_vec(),
                        })),
                    alonzo::Certificate::MoveInstantaneousRewardsCert(mir) =>
                                Ok(TxCertificate::MoveInstantaneousReward(MoveInstantaneosReward{
                                    source: match mir.source {
                                        alonzo::InstantaneousRewardSource::Reserves =>
                                            InstantaneousRewardSource::Reserves,
                                        alonzo::InstantaneousRewardSource::Treasury =>
                                            InstantaneousRewardSource::Treasury,
                                    },
                                    target: match &mir.target {
                                        alonzo::InstantaneousRewardTarget::StakeCredentials(creds) =>
                                            InstantaneousRewardTarget::StakeCredentials(
                                                creds.iter()
                                                .map(|(sc, v)| (Self::map_stake_credential(&sc),
                                                                *v as u64)) // TODO can be negative?
                                                .collect()),
                                        alonzo::InstantaneousRewardTarget::OtherAccountingPot(n) =>
                                            InstantaneousRewardTarget::OtherAccountingPot(*n),
                                    }
                                })),
                }
            }

            // Now repeated for a different type!
            MultiEraCert::Conway(cert) => {
                match cert.as_ref().as_ref() {
                    conway::Certificate::StakeRegistration(cred) =>
                        Ok(TxCertificate::StakeRegistration(
                            Self::map_stake_credential(cred))),
                    conway::Certificate::StakeDeregistration(cred) =>
                            Ok(TxCertificate::StakeDeregistration(
                                Self::map_stake_credential(cred))),
                    conway::Certificate::StakeDelegation(cred, pool_key_hash) =>
                                Ok(TxCertificate::StakeDelegation(StakeDelegation {
                                    credential: Self::map_stake_credential(cred),
                                    operator: pool_key_hash.to_vec()
                                })),
                    conway::Certificate::PoolRegistration { 
                        // TODO relays, pool_metadata
                        operator, vrf_keyhash, pledge, cost, margin, 
                        reward_account, pool_owners, relays, pool_metadata } =>
                                Ok(TxCertificate::PoolRegistration(PoolRegistration { 
                                    operator: operator.to_vec(), 
                                    vrf_key_hash: vrf_keyhash.to_vec(),
                                    pledge: *pledge,
                                    cost: *cost,
                                    margin: Ratio {
                                        numerator: margin.numerator,
                                        denominator: margin.denominator,
                                    },
                                    reward_account: reward_account.to_vec(),
                                    pool_owners: pool_owners
                                        .into_iter()
                                        .map(|v| v.to_vec())
                                        .collect(),
                                    relays: relays
                                        .into_iter()
                                        .map(|relay| Self::map_relay(relay))
                                        .collect(),
                                    pool_metadata: match pool_metadata {
                                        Nullable::Some(md) => Some(PoolMetadata {
                                            url: md.url.clone(),
                                            hash: md.hash.to_vec(),
                                        }),
                                        _ => None
                                    }
                                })),
                    conway::Certificate::PoolRetirement(pool_key_hash, epoch) =>
                                Ok(TxCertificate::PoolRetirement(PoolRetirement {
                                    operator: pool_key_hash.to_vec(), 
                                    epoch: *epoch
                                })),

                    conway::Certificate::Reg(cred, coin) =>
                                Ok(TxCertificate::Registration(Registration {
                                    credential: Self::map_stake_credential(cred),
                                    deposit: *coin,
                                })),

                    conway::Certificate::UnReg(cred, coin) =>
                                Ok(TxCertificate::Deregistration(Deregistration {
                                    credential: Self::map_stake_credential(cred),
                                    refund: *coin,
                                })),

                    conway::Certificate::VoteDeleg(cred, drep) =>
                                Ok(TxCertificate::VoteDelegation(VoteDelegation {
                                    credential: Self::map_stake_credential(cred),
                                    drep: Self::map_drep(drep),
                                })),

                    conway::Certificate::StakeVoteDeleg(cred, pool_key_hash, drep) =>
                                Ok(TxCertificate::StakeAndVoteDelegation(StakeAndVoteDelegation {
                                    credential: Self::map_stake_credential(cred),
                                    operator: pool_key_hash.to_vec(),
                                    drep: Self::map_drep(drep),
                                })),

                    conway::Certificate::StakeRegDeleg(cred, pool_key_hash, coin) =>
                                Ok(TxCertificate::StakeRegistrationAndDelegation(
                                    StakeRegistrationAndDelegation {
                                        credential: Self::map_stake_credential(cred),
                                        operator: pool_key_hash.to_vec(),
                                        deposit: *coin,
                                })),

                    conway::Certificate::VoteRegDeleg(cred, drep, coin) =>
                                Ok(TxCertificate::StakeRegistrationAndVoteDelegation(
                                    StakeRegistrationAndVoteDelegation {
                                        credential: Self::map_stake_credential(cred),
                                        drep: Self::map_drep(drep),
                                        deposit: *coin,
                                })),

                    conway::Certificate::StakeVoteRegDeleg(cred, pool_key_hash, drep, coin) =>
                                Ok(TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(
                                    StakeRegistrationAndStakeAndVoteDelegation {
                                        credential: Self::map_stake_credential(cred),
                                        operator: pool_key_hash.to_vec(),
                                        drep: Self::map_drep(drep),
                                        deposit: *coin,
                                })),

                    conway::Certificate::AuthCommitteeHot(cold_cred, hot_cred) =>
                                Ok(TxCertificate::AuthCommitteeHot(AuthCommitteeHot { 
                                    cold_credential: Self::map_stake_credential(cold_cred), 
                                    hot_credential: Self::map_stake_credential(hot_cred), 
                                })),

                    conway::Certificate::ResignCommitteeCold(cold_cred, anchor) =>
                                Ok(TxCertificate::ResignCommitteeCold(ResignCommitteeCold { 
                                    cold_credential: Self::map_stake_credential(cold_cred), 
                                    anchor: Self::map_nullable_anchor(&anchor)
                                })),

                    conway::Certificate::RegDRepCert(cred, coin, anchor) =>
                                Ok(TxCertificate::DRepRegistration(DRepRegistration {
                                    credential: Self::map_stake_credential(cred),
                                    deposit: *coin,
                                    anchor: Self::map_nullable_anchor(&anchor),
                                })),

                    conway::Certificate::UnRegDRepCert(cred, coin) =>
                                Ok(TxCertificate::DRepDeregistration(DRepDeregistration {
                                    credential: Self::map_stake_credential(cred),
                                    refund: *coin,
                                })),

                    conway::Certificate::UpdateDRepCert(cred, anchor) =>
                                Ok(TxCertificate::DRepUpdate(DRepUpdate {
                                    credential: Self::map_stake_credential(cred),
                                    anchor: Self::map_nullable_anchor(&anchor),
                                })),
                }
            }

            _ => Err(anyhow!("Unknown certificate era {:?} ignored", cert)),
        }
    }

    fn map_unit_interval(pallas_interval: &conway::UnitInterval) -> UnitInterval {
        UnitInterval {
            numerator: pallas_interval.numerator,
            denominator: pallas_interval.denominator,
        }
    }

    fn map_ex_units(pallas_units: &conway::ExUnits) -> ExUnits {
        ExUnits {
            mem: pallas_units.mem,
            steps: pallas_units.steps,
        }
    }

    fn map_execution_costs(pallas_ex_costs: &conway::ExUnitPrices) -> ExUnitPrices {
        ExUnitPrices {
            mem_price: Self::map_unit_interval(&pallas_ex_costs.mem_price),
            step_price: Self::map_unit_interval(&pallas_ex_costs.step_price),
        }
    }

    fn map_cost_models(pallas_cost_models: &conway::CostModels) -> CostModels {
        CostModels {
            plutus_v1: pallas_cost_models.plutus_v1.clone(),
            plutus_v2: pallas_cost_models.plutus_v2.clone(),
            plutus_v3: pallas_cost_models.plutus_v3.clone()
        }
    }

    fn map_pool_voting_thresholds(ts: &conway::PoolVotingThresholds) -> PoolVotingThresholds {
        PoolVotingThresholds {
            motion_no_confidence: Self::map_unit_interval(&ts.motion_no_confidence),
            committee_normal: Self::map_unit_interval(&ts.committee_normal),
            committee_no_confidence: Self::map_unit_interval(&ts.committee_no_confidence),
            hard_fork_initiation: Self::map_unit_interval(&ts.hard_fork_initiation),
            security_voting_threshold: Self::map_unit_interval(&ts.security_voting_threshold),
        }
    }

    fn map_drep_voting_thresholds(ts: &conway::DRepVotingThresholds) -> DRepVotingThresholds {
        DRepVotingThresholds {
            motion_no_confidence: Self::map_unit_interval(&ts.motion_no_confidence),
            committee_normal: Self::map_unit_interval(&ts.committee_normal),
            committee_no_confidence: Self::map_unit_interval(&ts.committee_no_confidence),
            update_constitution: Self::map_unit_interval(&ts.update_constitution),
            hard_fork_initiation: Self::map_unit_interval(&ts.hard_fork_initiation),
            pp_network_group: Self::map_unit_interval(&ts.pp_network_group),
            pp_economic_group: Self::map_unit_interval(&ts.pp_economic_group),
            pp_technical_group: Self::map_unit_interval(&ts.pp_technical_group),
            pp_governance_group: Self::map_unit_interval(&ts.pp_governance_group),
            treasury_withdrawal: Self::map_unit_interval(&ts.treasury_withdrawal),
        }
    }

    fn map_protocol_param_update(p: &conway::ProtocolParamUpdate) -> Box<ProtocolParamUpdate> {
        Box::new (ProtocolParamUpdate {
            minfee_a: p.minfee_a.clone(),
            minfee_b: p.minfee_b.clone(),
            max_block_body_size: p.max_block_body_size.clone(),
            max_transaction_size: p.max_transaction_size.clone(),
            max_block_header_size: p.max_block_header_size.clone(),
            key_deposit: p.key_deposit.clone(),
            pool_deposit: p.pool_deposit.clone(),
            maximum_epoch: p.maximum_epoch.clone(),
            desired_number_of_stake_pools: p.desired_number_of_stake_pools.clone(),
            pool_pledge_influence: p.pool_pledge_influence.as_ref().map(&Self::map_unit_interval),
            expansion_rate: p.expansion_rate.as_ref().map(&Self::map_unit_interval),
            treasury_growth_rate: p.expansion_rate.as_ref().map(&Self::map_unit_interval),
            min_pool_cost: p.min_pool_cost.clone(),
            ada_per_utxo_byte: p.ada_per_utxo_byte.clone(),
            cost_models_for_script_languages: p.cost_models_for_script_languages.as_ref().map(&Self::map_cost_models),
            execution_costs: p.execution_costs.as_ref().map(&Self::map_execution_costs),
            max_tx_ex_units: p.max_tx_ex_units.as_ref().map(&Self::map_ex_units),
            max_block_ex_units: p.max_block_ex_units.as_ref().map(&Self::map_ex_units),
            max_value_size: p.max_value_size.clone(),
            collateral_percentage: p.collateral_percentage.clone(),
            max_collateral_inputs: p.max_collateral_inputs.clone(),
            pool_voting_thresholds: p.pool_voting_thresholds.as_ref().map(&Self::map_pool_voting_thresholds),
            drep_voting_thresholds: p.drep_voting_thresholds.as_ref().map(&Self::map_drep_voting_thresholds),
            min_committee_size: p.min_committee_size.clone(),
            committee_term_limit: p.committee_term_limit.clone(),
            governance_action_validity_period: p.governance_action_validity_period.clone(),
            governance_action_deposit: p.governance_action_deposit.clone(),
            drep_deposit: p.drep_deposit.clone(),
            drep_inactivity_period: p.drep_inactivity_period.clone(),
            minfee_refscript_cost_per_byte: p.minfee_refscript_cost_per_byte.as_ref().map(&Self::map_unit_interval),
        })
    }

    fn map_governance_action(action: &conway::GovAction) -> Result<GovernanceAction> {
        match action {
            conway::GovAction::ParameterChange(id, protocol_update, script) =>
                Ok(GovernanceAction::ParameterChange(ParameterChangeAction {
                    previous_action_id: Self::map_nullable_gov_action_id(id)?,
                    protocol_param_update: Self::map_protocol_param_update(protocol_update),
                    script_hash: Self::map_nullable(&|x: &ScriptHash| x.to_vec(), &script),
                })),

            conway::GovAction::HardForkInitiation(id, version) =>
                Ok(GovernanceAction::HardForkInitiation(HardForkInitiationAction {
                    previous_action_id: Self::map_nullable_gov_action_id(id)?,
                    protocol_version: *version,
                })),

            conway::GovAction::TreasuryWithdrawals(withdrawals, script) =>
                Ok(GovernanceAction::TreasuryWithdrawals(TreasuryWithdrawalsAction{
                    rewards: HashMap::from_iter(withdrawals.iter().map(|(account,coin)| (account.to_vec(), *coin))),
                    script_hash: Self::map_nullable(&|x: &ScriptHash| x.to_vec(), script),
                })),

            conway::GovAction::NoConfidence(id) =>
                Ok(GovernanceAction::NoConfidence(Self::map_nullable_gov_action_id(id)?)),

            conway::GovAction::UpdateCommittee(id, committee, threshold, terms) =>
                Ok(GovernanceAction::UpdateCommittee(UpdateCommitteeAction {
                    previous_action_id: Self::map_nullable_gov_action_id(id)?,
                    committee: HashSet::from_iter(
                        committee.iter().map(Self::map_stake_credential)
                    ),
                    committee_thresold: HashMap::from_iter(
                        threshold.iter().map(|(k,v)| (Self::map_stake_credential(k), *v))
                    ),
                    terms: Self::map_unit_interval(terms),
                })),

            conway::GovAction::NewConstitution(id, constitution) =>
                Ok(GovernanceAction::NewConstitution(NewConstitutionAction {
                    previous_action_id: Self::map_nullable_gov_action_id(id)?,
                    new_constitution: Constitution {
                        anchor: Self::map_anchor(&constitution.anchor),
                        guardrail_script: Self::map_nullable(&|x: &ScriptHash| x.to_vec(), &constitution.guardrail_script)
                    },
                })),

            conway::GovAction::Information => Ok(GovernanceAction::Information)
        }
    }

    fn map_governance_proposals_procedures(gov_action_id: &GovActionId, prop: &conway::ProposalProcedure) -> Result<ProposalProcedure> {
        Ok(ProposalProcedure {
            deposit: prop.deposit,
            reward_account: prop.reward_account.to_vec(),
            gov_action_id: gov_action_id.clone(),
            gov_action: Self::map_governance_action(&prop.gov_action)?,
            anchor: Self::map_anchor(&prop.anchor),
        })
    }

    fn map_voter(voter: &conway::Voter) -> Voter {
        match voter {
            conway::Voter::ConstitutionalCommitteeKey(key_hash) => Voter::ConstitutionalCommitteeKey(key_hash.to_vec()),
            conway::Voter::ConstitutionalCommitteeScript(script_hash) => Voter::ConstitutionalCommitteeScript(script_hash.to_vec()),
            conway::Voter::DRepKey(addr_key_hash) => Voter::DRepKey(addr_key_hash.to_vec()),
            conway::Voter::DRepScript(script_hash) => Voter::DRepScript(script_hash.to_vec()),
            conway::Voter::StakePoolKey(key_hash) => Voter::StakePoolKey(key_hash.to_vec())
        }
    }

    fn map_vote(vote: &conway::Vote) -> Vote {
        match vote {
            conway::Vote::No => Vote::No,
            conway::Vote::Yes => Vote::Yes,
            conway::Vote::Abstain => Vote::Abstain
        }
    }

    fn map_single_governance_voting_procedure(proc: &conway::VotingProcedure) -> VotingProcedure {
        VotingProcedure {
            vote: Self::map_vote(&proc.vote),
            anchor: Self::map_nullable_anchor(&proc.anchor),
        }
    }

    fn map_all_governance_voting_procedures(vote_procs: &conway::VotingProcedures) -> Result<VotingProcedures> {
        let mut procs = VotingProcedures { votes: HashMap::new() };

        for (pallas_voter,pallas_pair) in vote_procs.iter() {
            let voter = Self::map_voter(pallas_voter);

            if let Some(_x) = procs.votes.insert(voter.clone(), HashMap::new()) {
                return Err(anyhow!("Duplicate voter {:?} in governance voting procedures: {:?}", voter, vote_procs));
            }

            let single_voter = procs.votes.get_mut(&voter)
                .ok_or_else(|| anyhow!("Cannot find voter {:?}, which must present", voter))?;

            for (pallas_action_id, pallas_voting_procedure) in pallas_pair.iter() {
                let action_id = Self::map_gov_action_id(pallas_action_id)?;
                let vp = Self::map_single_governance_voting_procedure(&pallas_voting_procedure);
                single_voter.insert(action_id, vp);
            }
        }

        Ok(procs)
    }
    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Subscribe for tx messages
        // Get configuration
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let publish_utxo_deltas_topic = config.get_string("publish-utxo-deltas-topic").ok();
        if let Some(ref topic) = publish_utxo_deltas_topic {
            info!("Publishing UTXO deltas on '{topic}'");
        }

        let publish_certificates_topic = config.get_string("publish-certificates-topic").ok();
        if let Some(ref topic) = publish_certificates_topic {
            info!("Publishing certificates on '{topic}'");
        }

        let publish_governance_procedures_topic = config.get_string("publish-governance-topic").ok();
        if let Some(ref topic) = publish_governance_procedures_topic {
            info!("Publishing governance procedures on '{topic}'");
        }

        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {
            let context = context.clone();
            let publish_utxo_deltas_topic = publish_utxo_deltas_topic.clone();
            let publish_certificates_topic = publish_certificates_topic.clone();
            let publish_governance_procedures_topic = publish_governance_procedures_topic.clone();

            async move {
                match message.as_ref() {
                    Message::ReceivedTxs(txs_msg) => {
                        if tracing::enabled!(tracing::Level::DEBUG) {
                            debug!("Received {} txs for slot {}",
                                txs_msg.txs.len(), txs_msg.block.slot);
                        }

                        // Construct messages which we batch up
                        let mut utxo_deltas_message = UTXODeltasMessage {
                            sequence: txs_msg.sequence,
                            block: txs_msg.block.clone(),
                            deltas: Vec::new(),
                        };

                        let mut certificates_message = TxCertificatesMessage {
                            sequence: txs_msg.sequence,
                            block: txs_msg.block.clone(),
                            certificates: Vec::new(),
                        };

                        let mut governance_message = GovernanceProceduresMessage {
                            sequence: txs_msg.sequence,
                            block: txs_msg.block.clone(),
                            voting_procedures: Vec::new(),
                            proposal_procedures: Vec::new(),
                        };

                        for raw_tx in &txs_msg.txs {
                            // Parse the tx
                            match MultiEraTx::decode(&raw_tx) {
                                Ok(tx) => {
                                    let inputs = tx.consumes();
                                    let outputs = tx.produces();
                                    let certs = tx.certs();
                                    let mut props = None;
                                    let mut votes = None;

                                    if let Some(conway) = tx.as_conway() {
                                        if let Some(ref v) = conway.transaction_body.voting_procedures {
                                            votes = Some(v);
                                        }

                                        if let Some(ref p) = conway.transaction_body.proposal_procedures {
                                            props = Some(p);
                                        }
/*
                                        if votes.is_some() || props.is_some() {
                                            let filename = format!("governance-logs/{:012}.json", txs_msg.sequence);
                                            let data = match serde_json::to_string(&message) {
                                                Ok(data) => data,
                                                Err(e) => format!("Error serializing message to json: {e}")
                                            };
                                            if let Err(e) = fs::write(filename, data) {
                                                error!("Error writing to file: {}", e);
                                            }
                                        }
 */
                                    }

                                    if tracing::enabled!(tracing::Level::DEBUG) {
                                        debug!("Decoded tx with {} inputs, {} outputs, {} certs",
                                           inputs.len(), outputs.len(), certs.len());
                                    }

                                    if publish_utxo_deltas_topic.is_some() {
                                        // Add all the inputs
                                        for input in inputs {  // MultiEraInput

                                            let oref = input.output_ref();

                                            // Construct message
                                            let tx_input = TxInput {
                                                tx_hash: oref.hash().to_vec(),
                                                index: oref.index(),
                                            };

                                            utxo_deltas_message.deltas
                                                .push(UTXODelta::Input(tx_input));
                                        }

                                        // Add all the outputs
                                        for (index, output) in outputs {  // MultiEraOutput

                                            match output.address() {
                                                Ok(pallas_address) =>
                                                {
                                                    match Self::map_address(&pallas_address) {
                                                        Ok(address) => {
                                                            let tx_output = TxOutput {
                                                                tx_hash: tx.hash().to_vec(),
                                                                index: index as u64,
                                                                address: address,
                                                                value: output.value().coin(),
                                                                // !!! datum
                                                            };

                                                            utxo_deltas_message.deltas
                                                                .push(UTXODelta::Output(tx_output));
                                                        }

                                                        Err(e) => 
                                                            error!("Output {index} in tx ignored: {e}")
                                                    }
                                                }

                                                Err(e) => 
                                                    error!("Can't parse output {index} in tx: {e}")
                                            }
                                        }
                                    }

                                    if publish_certificates_topic.is_some() {
                                        for cert in certs {
                                            match Self::map_certificate(&cert) {
                                                Ok(tx_cert) => {
                                                    certificates_message.certificates.push(tx_cert);
                                                },
                                                Err(_e) => { 
                                                    // TODO error unexpected
                                                    //error!("{e}"); 
                                                }
                                            }
                                        }
                                    }

                                    if publish_governance_procedures_topic.is_some() {
                                        if let Some(pp) = props {
                                            // Nonempty set -- governance_message.proposal_procedures will not be empty
                                            let mut proc_id = GovActionId { transaction_id: tx.hash().to_vec(), action_index: 0 };
                                            let mut action_index: usize = 0;
                                            for pallas_governance_proposals in pp.iter() {
                                                match proc_id.set_action_index(action_index)
                                                    .and_then (|proc_id| Self::map_governance_proposals_procedures(&proc_id, &pallas_governance_proposals))
                                                {
                                                    Ok(g) => governance_message.proposal_procedures.push(g),
                                                    Err(e) => error!("Cannot decode governance proposal procedure {} idx {} in slot {}: {e}", proc_id, action_index, txs_msg.block.slot)
                                                }
                                                action_index += 1;
                                            }
                                        }

                                        if let Some(pallas_vp) = votes {
                                            // Nonempty set -- governance_message.voting_procedures will not be empty
                                            match Self::map_all_governance_voting_procedures(pallas_vp) {
                                                Ok(vp) => governance_message.voting_procedures.push((tx.hash().to_vec(), vp)),
                                                Err(e) => error!("Cannot decode governance voting procedures in slot {}: {e}", txs_msg.block.slot)
                                            }
                                        }
                                    }
                                },

                                Err(e) => error!("Can't decode transaction in slot {}: {e}",
                                                 txs_msg.block.slot)
                            }
                        }

                        if let Some(topic) = publish_utxo_deltas_topic {
                            let utxo_deltas_message = Message::UTXODeltas(utxo_deltas_message);
                            context.message_bus.publish(&topic, Arc::new(utxo_deltas_message))
                                .await
                                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                        }

                        if let Some(topic) = publish_certificates_topic {
                            let certificates_message = Message::TxCertificates(certificates_message);
                            context.message_bus.publish(&topic, Arc::new(certificates_message))
                                .await
                                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                        }

                        if let Some(topic) = publish_governance_procedures_topic {
                            if !governance_message.is_empty() {
                                let gov_message = Message::GovernanceProcedures(governance_message);
                                context.message_bus.publish(&topic, Arc::new(gov_message))
                                    .await
                                    .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                            }
                        }
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        Ok(())
    }
}
