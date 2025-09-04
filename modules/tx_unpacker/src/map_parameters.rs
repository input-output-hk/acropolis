//! Acropolis transaction unpacker module for Caryatid
//! Performs conversion from Pallas library data to Acropolis

use anyhow::{anyhow, bail, Result};
use pallas::ledger::{
    primitives::{
        alonzo, babbage, conway, ExUnitPrices as PallasExUnitPrices, Nullable,
        ProtocolVersion as PallasProtocolVersion, Relay as PallasRelay, ScriptHash,
        StakeCredential as PallasStakeCredential,
    },
    traverse::MultiEraCert,
    *,
};

use acropolis_common::{
    protocol_params::{Nonce, NonceVariant, ProtocolVersion},
    rational_number::RationalNumber,
    *,
};
use std::collections::{HashMap, HashSet};

/// Map Pallas Network to our AddressNetwork
pub fn map_network(network: addresses::Network) -> Result<AddressNetwork> {
    match network {
        addresses::Network::Mainnet => Ok(AddressNetwork::Main),
        addresses::Network::Testnet => Ok(AddressNetwork::Test),
        _ => return Err(anyhow!("Unknown network in address")),
    }
}

/// Derive our Address from a Pallas address
// This is essentially a 1:1 mapping but makes the Message definitions independent
// of Pallas
pub fn map_address(address: &addresses::Address) -> Result<Address> {
    match address {
        addresses::Address::Byron(byron_address) => Ok(Address::Byron(ByronAddress {
            payload: byron_address.payload.to_vec(),
        })),

        addresses::Address::Shelley(shelley_address) => Ok(Address::Shelley(ShelleyAddress {
            network: map_network(shelley_address.network())?,

            payment: match shelley_address.payment() {
                addresses::ShelleyPaymentPart::Key(hash) => {
                    ShelleyAddressPaymentPart::PaymentKeyHash(hash.to_vec())
                }
                addresses::ShelleyPaymentPart::Script(hash) => {
                    ShelleyAddressPaymentPart::ScriptHash(hash.to_vec())
                }
            },

            delegation: match shelley_address.delegation() {
                addresses::ShelleyDelegationPart::Null => ShelleyAddressDelegationPart::None,
                addresses::ShelleyDelegationPart::Key(hash) => {
                    ShelleyAddressDelegationPart::StakeKeyHash(hash.to_vec())
                }
                addresses::ShelleyDelegationPart::Script(hash) => {
                    ShelleyAddressDelegationPart::ScriptHash(hash.to_vec())
                }
                addresses::ShelleyDelegationPart::Pointer(pointer) => {
                    ShelleyAddressDelegationPart::Pointer(ShelleyAddressPointer {
                        slot: pointer.slot(),
                        tx_index: pointer.tx_idx(),
                        cert_index: pointer.cert_idx(),
                    })
                }
            },
        })),

        addresses::Address::Stake(stake_address) => Ok(Address::Stake(StakeAddress {
            network: map_network(stake_address.network())?,
            payload: match stake_address.payload() {
                addresses::StakePayload::Stake(hash) => {
                    StakeAddressPayload::StakeKeyHash(hash.to_vec())
                }
                addresses::StakePayload::Script(hash) => {
                    StakeAddressPayload::ScriptHash(hash.to_vec())
                }
            },
        })),
    }
}

/// Map a Pallas StakeCredential to ours
pub fn map_stake_credential(cred: &PallasStakeCredential) -> StakeCredential {
    match cred {
        PallasStakeCredential::AddrKeyhash(key_hash) => {
            StakeCredential::AddrKeyHash(key_hash.to_vec())
        }
        PallasStakeCredential::ScriptHash(script_hash) => {
            StakeCredential::ScriptHash(script_hash.to_vec())
        }
    }
}

/// Map a Pallas DRep to our DRepChoice
pub fn map_drep(drep: &conway::DRep) -> DRepChoice {
    match drep {
        conway::DRep::Key(key_hash) => DRepChoice::Key(key_hash.to_vec()),
        conway::DRep::Script(script_hash) => DRepChoice::Script(script_hash.to_vec()),
        conway::DRep::Abstain => DRepChoice::Abstain,
        conway::DRep::NoConfidence => DRepChoice::NoConfidence,
    }
}

pub fn map_nullable<Src: Clone, Dst>(
    f: impl FnOnce(&Src) -> Dst,
    nullable_src: &Nullable<Src>,
) -> Option<Dst> {
    match nullable_src {
        Nullable::Some(src) => Some(f(src)),
        _ => None,
    }
}

pub fn map_nullable_result<Src: Clone, Dst>(
    f: impl FnOnce(&Src) -> Result<Dst>,
    nullable_src: &Nullable<Src>,
) -> Result<Option<Dst>> {
    match nullable_src {
        Nullable::Some(src) => {
            let res = f(src)?;
            Ok(Some(res))
        }
        _ => Ok(None),
    }
}

pub fn map_anchor(anchor: &conway::Anchor) -> Anchor {
    Anchor {
        url: anchor.url.clone(),
        data_hash: anchor.content_hash.to_vec(),
    }
}

/// Map a Nullable Anchor to ours
pub fn map_nullable_anchor(anchor: &Nullable<conway::Anchor>) -> Option<Anchor> {
    map_nullable(&map_anchor, anchor)
}

pub fn map_gov_action_id(pallas_action_id: &conway::GovActionId) -> Result<GovActionId> {
    let act_idx_u8: u8 = match pallas_action_id.action_index.try_into() {
        Ok(v) => v,
        Err(e) => return Err(anyhow!("Invalid action index {e}")),
    };

    Ok(GovActionId {
        transaction_id: *pallas_action_id.transaction_id,
        action_index: act_idx_u8,
    })
}

pub fn map_nullable_gov_action_id(
    id: &Nullable<conway::GovActionId>,
) -> Result<Option<GovActionId>> {
    map_nullable_result(&map_gov_action_id, id)
}

fn map_constitution(constitution: &conway::Constitution) -> Constitution {
    Constitution {
        anchor: map_anchor(&constitution.anchor),
        guardrail_script: map_nullable(|x| x.to_vec(), &constitution.guardrail_script),
    }
}

/// Map a Pallas Relay to ours
fn map_relay(relay: &PallasRelay) -> Relay {
    match relay {
        PallasRelay::SingleHostAddr(port, ipv4, ipv6) => Relay::SingleHostAddr(SingleHostAddr {
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
        PallasRelay::SingleHostName(port, dns_name) => Relay::SingleHostName(SingleHostName {
            port: match port {
                Nullable::Some(port) => Some(*port as u16),
                _ => None,
            },
            dns_name: dns_name.clone(),
        }),
        PallasRelay::MultiHostName(dns_name) => Relay::MultiHostName(MultiHostName {
            dns_name: dns_name.clone(),
        }),
    }
}

//
// Certificates
//

/// Derive our TxCertificate from a Pallas Certificate
pub fn map_certificate(
    cert: &MultiEraCert,
    tx_hash: TxHash,
    tx_index: usize,
    cert_index: usize,
) -> Result<TxCertificate> {
    match cert {
        MultiEraCert::NotApplicable => Err(anyhow!("Not applicable cert!")),

        MultiEraCert::AlonzoCompatible(cert) => match cert.as_ref().as_ref() {
            alonzo::Certificate::StakeRegistration(cred) => {
                Ok(TxCertificate::StakeRegistration(StakeCredentialWithPos {
                    stake_credential: map_stake_credential(cred),
                    tx_index: tx_index.try_into().unwrap(),
                    cert_index: cert_index.try_into().unwrap(),
                }))
            }
            alonzo::Certificate::StakeDeregistration(cred) => Ok(
                TxCertificate::StakeDeregistration(map_stake_credential(cred)),
            ),
            alonzo::Certificate::StakeDelegation(cred, pool_key_hash) => {
                Ok(TxCertificate::StakeDelegation(StakeDelegation {
                    credential: map_stake_credential(cred),
                    operator: pool_key_hash.to_vec(),
                }))
            }
            alonzo::Certificate::PoolRegistration {
                operator,
                vrf_keyhash,
                pledge,
                cost,
                margin,
                reward_account,
                pool_owners,
                relays,
                pool_metadata,
            } => Ok(TxCertificate::PoolRegistration(PoolRegistration {
                operator: operator.to_vec(),
                vrf_key_hash: vrf_keyhash.to_vec(),
                pledge: *pledge,
                cost: *cost,
                margin: Ratio {
                    numerator: margin.numerator,
                    denominator: margin.denominator,
                },
                reward_account: reward_account.to_vec(),
                pool_owners: pool_owners.into_iter().map(|v| v.to_vec()).collect(),
                relays: relays.into_iter().map(|relay| map_relay(relay)).collect(),
                pool_metadata: match pool_metadata {
                    Nullable::Some(md) => Some(PoolMetadata {
                        url: md.url.clone(),
                        hash: md.hash.to_vec(),
                    }),
                    _ => None,
                },
            })),
            alonzo::Certificate::PoolRetirement(pool_key_hash, epoch) => {
                Ok(TxCertificate::PoolRetirement(PoolRetirement {
                    operator: pool_key_hash.to_vec(),
                    epoch: *epoch,
                }))
            }
            alonzo::Certificate::GenesisKeyDelegation(
                genesis_hash,
                genesis_delegate_hash,
                vrf_key_hash,
            ) => Ok(TxCertificate::GenesisKeyDelegation(GenesisKeyDelegation {
                genesis_hash: genesis_hash.to_vec(),
                genesis_delegate_hash: genesis_delegate_hash.to_vec(),
                vrf_key_hash: vrf_key_hash.to_vec(),
            })),
            alonzo::Certificate::MoveInstantaneousRewardsCert(mir) => Ok(
                TxCertificate::MoveInstantaneousReward(MoveInstantaneousReward {
                    source: match mir.source {
                        alonzo::InstantaneousRewardSource::Reserves => {
                            InstantaneousRewardSource::Reserves
                        }
                        alonzo::InstantaneousRewardSource::Treasury => {
                            InstantaneousRewardSource::Treasury
                        }
                    },
                    target: match &mir.target {
                        alonzo::InstantaneousRewardTarget::StakeCredentials(creds) => {
                            InstantaneousRewardTarget::StakeCredentials(
                                creds
                                    .iter()
                                    .map(|(sc, v)| (map_stake_credential(&sc), *v))
                                    .collect(),
                            )
                        }
                        alonzo::InstantaneousRewardTarget::OtherAccountingPot(n) => {
                            InstantaneousRewardTarget::OtherAccountingPot(*n)
                        }
                    },
                }),
            ),
        },

        // Now repeated for a different type!
        MultiEraCert::Conway(cert) => {
            match cert.as_ref().as_ref() {
                conway::Certificate::StakeRegistration(cred) => {
                    Ok(TxCertificate::StakeRegistration(StakeCredentialWithPos {
                        stake_credential: map_stake_credential(cred),
                        tx_index: tx_index.try_into().unwrap(),
                        cert_index: cert_index.try_into().unwrap(),
                    }))
                }
                conway::Certificate::StakeDeregistration(cred) => Ok(
                    TxCertificate::StakeDeregistration(map_stake_credential(cred)),
                ),
                conway::Certificate::StakeDelegation(cred, pool_key_hash) => {
                    Ok(TxCertificate::StakeDelegation(StakeDelegation {
                        credential: map_stake_credential(cred),
                        operator: pool_key_hash.to_vec(),
                    }))
                }
                conway::Certificate::PoolRegistration {
                    // TODO relays, pool_metadata
                    operator,
                    vrf_keyhash,
                    pledge,
                    cost,
                    margin,
                    reward_account,
                    pool_owners,
                    relays,
                    pool_metadata,
                } => Ok(TxCertificate::PoolRegistration(PoolRegistration {
                    operator: operator.to_vec(),
                    vrf_key_hash: vrf_keyhash.to_vec(),
                    pledge: *pledge,
                    cost: *cost,
                    margin: Ratio {
                        numerator: margin.numerator,
                        denominator: margin.denominator,
                    },
                    reward_account: reward_account.to_vec(),
                    pool_owners: pool_owners.into_iter().map(|v| v.to_vec()).collect(),
                    relays: relays.into_iter().map(|relay| map_relay(relay)).collect(),
                    pool_metadata: match pool_metadata {
                        Nullable::Some(md) => Some(PoolMetadata {
                            url: md.url.clone(),
                            hash: md.hash.to_vec(),
                        }),
                        _ => None,
                    },
                })),
                conway::Certificate::PoolRetirement(pool_key_hash, epoch) => {
                    Ok(TxCertificate::PoolRetirement(PoolRetirement {
                        operator: pool_key_hash.to_vec(),
                        epoch: *epoch,
                    }))
                }

                conway::Certificate::Reg(cred, coin) => {
                    Ok(TxCertificate::Registration(Registration {
                        credential: map_stake_credential(cred),
                        deposit: *coin,
                    }))
                }

                conway::Certificate::UnReg(cred, coin) => {
                    Ok(TxCertificate::Deregistration(Deregistration {
                        credential: map_stake_credential(cred),
                        refund: *coin,
                    }))
                }

                conway::Certificate::VoteDeleg(cred, drep) => {
                    Ok(TxCertificate::VoteDelegation(VoteDelegation {
                        credential: map_stake_credential(cred),
                        drep: map_drep(drep),
                    }))
                }

                conway::Certificate::StakeVoteDeleg(cred, pool_key_hash, drep) => Ok(
                    TxCertificate::StakeAndVoteDelegation(StakeAndVoteDelegation {
                        credential: map_stake_credential(cred),
                        operator: pool_key_hash.to_vec(),
                        drep: map_drep(drep),
                    }),
                ),

                conway::Certificate::StakeRegDeleg(cred, pool_key_hash, coin) => Ok(
                    TxCertificate::StakeRegistrationAndDelegation(StakeRegistrationAndDelegation {
                        credential: map_stake_credential(cred),
                        operator: pool_key_hash.to_vec(),
                        deposit: *coin,
                    }),
                ),

                conway::Certificate::VoteRegDeleg(cred, drep, coin) => {
                    Ok(TxCertificate::StakeRegistrationAndVoteDelegation(
                        StakeRegistrationAndVoteDelegation {
                            credential: map_stake_credential(cred),
                            drep: map_drep(drep),
                            deposit: *coin,
                        },
                    ))
                }

                conway::Certificate::StakeVoteRegDeleg(cred, pool_key_hash, drep, coin) => {
                    Ok(TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(
                        StakeRegistrationAndStakeAndVoteDelegation {
                            credential: map_stake_credential(cred),
                            operator: pool_key_hash.to_vec(),
                            drep: map_drep(drep),
                            deposit: *coin,
                        },
                    ))
                }

                conway::Certificate::AuthCommitteeHot(cold_cred, hot_cred) => {
                    Ok(TxCertificate::AuthCommitteeHot(AuthCommitteeHot {
                        cold_credential: map_stake_credential(cold_cred),
                        hot_credential: map_stake_credential(hot_cred),
                    }))
                }

                conway::Certificate::ResignCommitteeCold(cold_cred, anchor) => {
                    Ok(TxCertificate::ResignCommitteeCold(ResignCommitteeCold {
                        cold_credential: map_stake_credential(cold_cred),
                        anchor: map_nullable_anchor(&anchor),
                    }))
                }

                conway::Certificate::RegDRepCert(cred, coin, anchor) => {
                    Ok(TxCertificate::DRepRegistration(DRepRegistrationWithPos {
                        reg: DRepRegistration {
                            credential: map_stake_credential(cred),
                            deposit: *coin,
                            anchor: map_nullable_anchor(&anchor),
                        },
                        tx_hash,
                        cert_index: cert_index as u64,
                    }))
                }

                conway::Certificate::UnRegDRepCert(cred, coin) => Ok(
                    TxCertificate::DRepDeregistration(DRepDeregistrationWithPos {
                        reg: DRepDeregistration {
                            credential: map_stake_credential(cred),
                            refund: *coin,
                        },
                        tx_hash,
                        cert_index: cert_index as u64,
                    }),
                ),

                conway::Certificate::UpdateDRepCert(cred, anchor) => {
                    Ok(TxCertificate::DRepUpdate(DRepUpdateWithPos {
                        reg: DRepUpdate {
                            credential: map_stake_credential(cred),
                            anchor: map_nullable_anchor(&anchor),
                        },
                        tx_hash,
                        cert_index: cert_index as u64,
                    }))
                }
            }
        }

        _ => Err(anyhow!("Unknown certificate era {:?} ignored", cert)),
    }
}

fn map_unit_interval(pallas_interval: &conway::UnitInterval) -> RationalNumber {
    RationalNumber::new(pallas_interval.numerator, pallas_interval.denominator)
}

fn map_ex_units(pallas_units: &conway::ExUnits) -> ExUnits {
    ExUnits {
        mem: pallas_units.mem,
        steps: pallas_units.steps,
    }
}

fn map_execution_costs(pallas_ex_costs: &PallasExUnitPrices) -> ExUnitPrices {
    ExUnitPrices {
        mem_price: map_unit_interval(&pallas_ex_costs.mem_price),
        step_price: map_unit_interval(&pallas_ex_costs.step_price),
    }
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

fn map_alonzo_nonce(e: &alonzo::Nonce) -> Nonce {
    Nonce {
        tag: match &e.variant {
            alonzo::NonceVariant::NeutralNonce => NonceVariant::NeutralNonce,
            alonzo::NonceVariant::Nonce => NonceVariant::Nonce,
        },
        hash: e.hash.map(|v| v.to_vec()),
    }
}

fn map_alonzo_single_model(model: &alonzo::CostModel) -> Option<CostModel> {
    Some(CostModel::new(model.clone()))
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

fn map_protocol_version((major, minor): &PallasProtocolVersion) -> ProtocolVersion {
    ProtocolVersion {
        minor: *minor,
        major: *major,
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

fn map_conway_protocol_param_update(p: &conway::ProtocolParamUpdate) -> Box<ProtocolParamUpdate> {
    Box::new(ProtocolParamUpdate {
        // Fields, common for Conway and Alonzo-compatible
        minfee_a: p.minfee_a.clone(),
        minfee_b: p.minfee_b.clone(),
        max_block_body_size: p.max_block_body_size.clone(),
        max_transaction_size: p.max_transaction_size.clone(),
        max_block_header_size: p.max_block_header_size.clone(),
        key_deposit: p.key_deposit.clone(),
        pool_deposit: p.pool_deposit.clone(),
        maximum_epoch: p.maximum_epoch.clone(),
        desired_number_of_stake_pools: p.desired_number_of_stake_pools.clone(),
        pool_pledge_influence: p.pool_pledge_influence.as_ref().map(&map_unit_interval),
        expansion_rate: p.expansion_rate.as_ref().map(&map_unit_interval),
        treasury_growth_rate: p.treasury_growth_rate.as_ref().map(&map_unit_interval),
        min_pool_cost: p.min_pool_cost.clone(),
        coins_per_utxo_byte: p.ada_per_utxo_byte.clone(),
        lovelace_per_utxo_word: None,
        cost_models_for_script_languages: p
            .cost_models_for_script_languages
            .as_ref()
            .map(&map_conway_cost_models),
        execution_costs: p.execution_costs.as_ref().map(&map_conway_execution_costs),
        max_tx_ex_units: p.max_tx_ex_units.as_ref().map(&map_ex_units),
        max_block_ex_units: p.max_block_ex_units.as_ref().map(&map_ex_units),
        max_value_size: p.max_value_size.clone(),
        collateral_percentage: p.collateral_percentage.clone(),
        max_collateral_inputs: p.max_collateral_inputs.clone(),

        // Fields, specific for Conway
        pool_voting_thresholds: p.pool_voting_thresholds.as_ref().map(&map_pool_voting_thresholds),
        drep_voting_thresholds: p.drep_voting_thresholds.as_ref().map(&map_drep_voting_thresholds),
        min_committee_size: p.min_committee_size.clone(),
        committee_term_limit: p.committee_term_limit.clone(),
        governance_action_validity_period: p.governance_action_validity_period.clone(),
        governance_action_deposit: p.governance_action_deposit.clone(),
        drep_deposit: p.drep_deposit.clone(),
        drep_inactivity_period: p.drep_inactivity_period.clone(),
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
                script_hash: map_nullable(&|x: &ScriptHash| x.to_vec(), &script),
            }))
        }

        conway::GovAction::HardForkInitiation(id, version) => Ok(
            GovernanceAction::HardForkInitiation(HardForkInitiationAction {
                previous_action_id: map_nullable_gov_action_id(id)?,
                protocol_version: *version,
            }),
        ),

        conway::GovAction::TreasuryWithdrawals(withdrawals, script) => Ok(
            GovernanceAction::TreasuryWithdrawals(TreasuryWithdrawalsAction {
                rewards: HashMap::from_iter(
                    withdrawals.iter().map(|(account, coin)| (account.to_vec(), *coin)),
                ),
                script_hash: map_nullable(&|x: &ScriptHash| x.to_vec(), script),
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
                new_constitution: map_constitution(&constitution),
            }))
        }

        conway::GovAction::Information => Ok(GovernanceAction::Information),
    }
}

fn map_u32_to_u64(n: Option<u32>) -> Option<u64> {
    n.as_ref().map(|x| *x as u64)
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
        key_deposit: p.key_deposit.clone(),
        pool_deposit: p.pool_deposit.clone(),
        maximum_epoch: p.maximum_epoch.clone(),
        desired_number_of_stake_pools: map_u32_to_u64(p.desired_number_of_stake_pools),
        pool_pledge_influence: p.pool_pledge_influence.as_ref().map(&map_unit_interval),
        expansion_rate: p.expansion_rate.as_ref().map(&map_unit_interval),
        treasury_growth_rate: p.treasury_growth_rate.as_ref().map(&map_unit_interval),
        min_pool_cost: p.min_pool_cost.clone(),
        lovelace_per_utxo_word: p.ada_per_utxo_byte.clone(), // Pre Babbage (Represents cost per 8-byte word)
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
        key_deposit: p.key_deposit.clone(),
        pool_deposit: p.pool_deposit.clone(),
        maximum_epoch: p.maximum_epoch.clone(),
        desired_number_of_stake_pools: map_u32_to_u64(p.desired_number_of_stake_pools),
        pool_pledge_influence: p.pool_pledge_influence.as_ref().map(&map_unit_interval),
        expansion_rate: p.expansion_rate.as_ref().map(&map_unit_interval),
        treasury_growth_rate: p.treasury_growth_rate.as_ref().map(&map_unit_interval),
        min_pool_cost: p.min_pool_cost.clone(),
        lovelace_per_utxo_word: None,
        coins_per_utxo_byte: p.ada_per_utxo_byte.clone(),
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

pub fn map_governance_proposals_procedures(
    gov_action_id: &GovActionId,
    prop: &conway::ProposalProcedure,
) -> Result<ProposalProcedure> {
    Ok(ProposalProcedure {
        deposit: prop.deposit,
        reward_account: prop.reward_account.to_vec(),
        gov_action_id: gov_action_id.clone(),
        gov_action: map_governance_action(&prop.gov_action)?,
        anchor: map_anchor(&prop.anchor),
    })
}

fn map_voter(voter: &conway::Voter) -> Voter {
    match voter {
        conway::Voter::ConstitutionalCommitteeKey(key_hash) => {
            Voter::ConstitutionalCommitteeKey(key_hash.to_vec())
        }
        conway::Voter::ConstitutionalCommitteeScript(script_hash) => {
            Voter::ConstitutionalCommitteeScript(script_hash.to_vec())
        }
        conway::Voter::DRepKey(addr_key_hash) => Voter::DRepKey(addr_key_hash.to_vec()),
        conway::Voter::DRepScript(script_hash) => Voter::DRepScript(script_hash.to_vec()),
        conway::Voter::StakePoolKey(key_hash) => Voter::StakePoolKey(key_hash.to_vec()),
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
                map_single_governance_voting_procedure(vote_index as u32, &pallas_voting_procedure);
            single_voter.voting_procedures.insert(action_id, vp);
        }
    }

    Ok(procs)
}
