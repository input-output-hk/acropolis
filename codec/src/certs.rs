use crate::{
    address::{map_stake_address, map_stake_credential},
    utils::*,
};
use acropolis_common::*;
use anyhow::{Result, anyhow};
use pallas_primitives::{Nullable, alonzo, conway};
use pallas_traverse::MultiEraCert;

#[allow(clippy::too_many_arguments)]
pub fn to_pool_reg(
    operator: &pallas_primitives::PoolKeyhash,
    vrf_keyhash: &pallas_primitives::VrfKeyhash,
    pledge: &pallas_primitives::Coin,
    cost: &pallas_primitives::Coin,
    margin: &pallas_primitives::UnitInterval,
    reward_account: &pallas_primitives::RewardAccount,
    pool_owners: &[pallas_primitives::AddrKeyhash],
    relays: &[pallas_primitives::Relay],
    pool_metadata: &Nullable<pallas_primitives::PoolMetadata>,
    network_id: NetworkId,
    force_reward_network_id: bool,
) -> Result<PoolRegistration> {
    Ok(PoolRegistration {
        operator: to_pool_id(operator),
        vrf_key_hash: to_vrf_key(vrf_keyhash),
        pledge: *pledge,
        cost: *cost,
        margin: Ratio {
            numerator: margin.numerator,
            denominator: margin.denominator,
        },
        reward_account: if force_reward_network_id {
            StakeAddress::new(
                StakeAddress::from_binary(reward_account)?.credential,
                network_id.clone(),
            )
        } else {
            StakeAddress::from_binary(reward_account)?
        },
        pool_owners: pool_owners
            .iter()
            .map(|v| {
                StakeAddress::new(StakeCredential::AddrKeyHash(to_hash(v)), network_id.clone())
            })
            .collect(),
        relays: relays.iter().map(map_relay).collect(),
        pool_metadata: match pool_metadata {
            Nullable::Some(md) => Some(PoolMetadata {
                url: md.url.clone(),
                hash: md.hash.to_vec(),
            }),
            _ => None,
        },
    })
}

/// Derive our TxCertificate from a Pallas Certificate
pub fn map_certificate(
    cert: &MultiEraCert,
    tx_identifier: TxIdentifier,
    tx_hash: TxHash,
    cert_index: usize,
    network_id: NetworkId,
) -> Result<TxCertificateWithPos> {
    match cert {
        MultiEraCert::NotApplicable => Err(anyhow!("Not applicable cert!")),

        MultiEraCert::AlonzoCompatible(cert) => match cert.as_ref().as_ref() {
            alonzo::Certificate::StakeRegistration(cred) => Ok(TxCertificateWithPos {
                cert: TxCertificate::StakeRegistration(map_stake_address(cred, network_id)),
                tx_identifier,
                tx_hash,
                cert_index: cert_index.try_into().unwrap(),
            }),
            alonzo::Certificate::StakeDeregistration(cred) => Ok(TxCertificateWithPos {
                cert: TxCertificate::StakeDeregistration(map_stake_address(cred, network_id)),
                tx_identifier,
                tx_hash,
                cert_index: cert_index.try_into().unwrap(),
            }),
            alonzo::Certificate::StakeDelegation(cred, pool_key_hash) => Ok(TxCertificateWithPos {
                cert: TxCertificate::StakeDelegation(StakeDelegation {
                    stake_address: map_stake_address(cred, network_id),
                    operator: to_pool_id(pool_key_hash),
                }),
                tx_identifier,
                tx_hash,
                cert_index: cert_index.try_into().unwrap(),
            }),
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
            } => Ok(TxCertificateWithPos {
                cert: TxCertificate::PoolRegistration(to_pool_reg(
                    operator,
                    vrf_keyhash,
                    pledge,
                    cost,
                    margin,
                    reward_account,
                    pool_owners,
                    relays,
                    pool_metadata,
                    network_id,
                    false,
                )?),
                tx_identifier,
                tx_hash,
                cert_index: cert_index as u64,
            }),
            alonzo::Certificate::PoolRetirement(pool_key_hash, epoch) => Ok(TxCertificateWithPos {
                cert: TxCertificate::PoolRetirement(PoolRetirement {
                    operator: to_pool_id(pool_key_hash),
                    epoch: *epoch,
                }),
                tx_identifier,
                tx_hash,
                cert_index: cert_index as u64,
            }),
            alonzo::Certificate::GenesisKeyDelegation(
                genesis_hash,
                genesis_delegate_hash,
                vrf_key_hash,
            ) => Ok(TxCertificateWithPos {
                cert: TxCertificate::GenesisKeyDelegation(GenesisKeyDelegation {
                    genesis_hash: genesis_to_hash(genesis_hash),
                    genesis_delegate_hash: genesis_delegate_to_hash(genesis_delegate_hash),
                    vrf_key_hash: to_vrf_key(vrf_key_hash),
                }),
                tx_identifier,
                tx_hash,
                cert_index: cert_index as u64,
            }),
            alonzo::Certificate::MoveInstantaneousRewardsCert(mir) => Ok(TxCertificateWithPos {
                cert: TxCertificate::MoveInstantaneousReward(MoveInstantaneousReward {
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
                            InstantaneousRewardTarget::StakeAddresses(
                                creds
                                    .iter()
                                    .map(|(sc, v)| (map_stake_address(sc, network_id.clone()), *v))
                                    .collect(),
                            )
                        }
                        alonzo::InstantaneousRewardTarget::OtherAccountingPot(n) => {
                            InstantaneousRewardTarget::OtherAccountingPot(*n)
                        }
                    },
                }),
                tx_identifier,
                tx_hash,
                cert_index: cert_index as u64,
            }),
        },

        // Now repeated for a different type!
        MultiEraCert::Conway(cert) => {
            match cert.as_ref().as_ref() {
                conway::Certificate::StakeRegistration(cred) => Ok(TxCertificateWithPos {
                    cert: TxCertificate::StakeRegistration(map_stake_address(cred, network_id)),
                    tx_identifier,
                    tx_hash,

                    cert_index: cert_index.try_into().unwrap(),
                }),

                conway::Certificate::StakeDeregistration(cred) => Ok(TxCertificateWithPos {
                    cert: TxCertificate::StakeDeregistration(map_stake_address(cred, network_id)),
                    tx_identifier,
                    tx_hash,
                    cert_index: cert_index.try_into().unwrap(),
                }),

                conway::Certificate::StakeDelegation(cred, pool_key_hash) => {
                    Ok(TxCertificateWithPos {
                        cert: TxCertificate::StakeDelegation(StakeDelegation {
                            stake_address: map_stake_address(cred, network_id),
                            operator: to_pool_id(pool_key_hash),
                        }),
                        tx_identifier,
                        tx_hash,
                        cert_index: cert_index.try_into().unwrap(),
                    })
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
                } => Ok(TxCertificateWithPos {
                    cert: TxCertificate::PoolRegistration(to_pool_reg(
                        operator,
                        vrf_keyhash,
                        pledge,
                        cost,
                        margin,
                        reward_account,
                        pool_owners,
                        relays,
                        pool_metadata,
                        network_id,
                        // Force networkId - in mainnet epoch 208, one SPO (c63dab6d780a) uses
                        // an e0 (testnet!) address, and this then fails to match their actual
                        // reward account (e1).  Feels like this should have been
                        // a validation failure, but clearly wasn't!
                        true,
                    )?),
                    tx_identifier,
                    tx_hash,
                    cert_index: cert_index as u64,
                }),
                conway::Certificate::PoolRetirement(pool_key_hash, epoch) => {
                    Ok(TxCertificateWithPos {
                        cert: TxCertificate::PoolRetirement(PoolRetirement {
                            operator: to_pool_id(pool_key_hash),
                            epoch: *epoch,
                        }),
                        tx_identifier,
                        tx_hash,
                        cert_index: cert_index as u64,
                    })
                }

                conway::Certificate::Reg(cred, coin) => Ok(TxCertificateWithPos {
                    cert: TxCertificate::Registration(Registration {
                        stake_address: map_stake_address(cred, network_id),
                        deposit: *coin,
                    }),
                    tx_identifier,
                    tx_hash,
                    cert_index: cert_index as u64,
                }),

                conway::Certificate::UnReg(cred, coin) => Ok(TxCertificateWithPos {
                    cert: TxCertificate::Deregistration(Deregistration {
                        stake_address: map_stake_address(cred, network_id),
                        refund: *coin,
                    }),
                    tx_identifier,
                    tx_hash,
                    cert_index: cert_index as u64,
                }),

                conway::Certificate::VoteDeleg(cred, drep) => Ok(TxCertificateWithPos {
                    cert: TxCertificate::VoteDelegation(VoteDelegation {
                        stake_address: map_stake_address(cred, network_id),
                        drep: map_drep(drep),
                    }),
                    tx_identifier,
                    tx_hash,
                    cert_index: cert_index as u64,
                }),

                conway::Certificate::StakeVoteDeleg(cred, pool_key_hash, drep) => {
                    Ok(TxCertificateWithPos {
                        cert: TxCertificate::StakeAndVoteDelegation(StakeAndVoteDelegation {
                            stake_address: map_stake_address(cred, network_id),
                            operator: to_pool_id(pool_key_hash),
                            drep: map_drep(drep),
                        }),
                        tx_identifier,
                        tx_hash,
                        cert_index: cert_index as u64,
                    })
                }

                conway::Certificate::StakeRegDeleg(cred, pool_key_hash, coin) => {
                    Ok(TxCertificateWithPos {
                        cert: TxCertificate::StakeRegistrationAndDelegation(
                            StakeRegistrationAndDelegation {
                                stake_address: map_stake_address(cred, network_id),
                                operator: to_pool_id(pool_key_hash),
                                deposit: *coin,
                            },
                        ),
                        tx_identifier,
                        tx_hash,
                        cert_index: cert_index as u64,
                    })
                }

                conway::Certificate::VoteRegDeleg(cred, drep, coin) => Ok(TxCertificateWithPos {
                    cert: TxCertificate::StakeRegistrationAndVoteDelegation(
                        StakeRegistrationAndVoteDelegation {
                            stake_address: map_stake_address(cred, network_id),
                            drep: map_drep(drep),
                            deposit: *coin,
                        },
                    ),
                    tx_identifier,
                    tx_hash,
                    cert_index: cert_index as u64,
                }),

                conway::Certificate::StakeVoteRegDeleg(cred, pool_key_hash, drep, coin) => {
                    Ok(TxCertificateWithPos {
                        cert: TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(
                            StakeRegistrationAndStakeAndVoteDelegation {
                                stake_address: map_stake_address(cred, network_id),
                                operator: to_pool_id(pool_key_hash),
                                drep: map_drep(drep),
                                deposit: *coin,
                            },
                        ),
                        tx_identifier,
                        tx_hash,
                        cert_index: cert_index as u64,
                    })
                }

                conway::Certificate::AuthCommitteeHot(cold_cred, hot_cred) => {
                    Ok(TxCertificateWithPos {
                        cert: TxCertificate::AuthCommitteeHot(AuthCommitteeHot {
                            cold_credential: map_stake_credential(cold_cred),
                            hot_credential: map_stake_credential(hot_cred),
                        }),
                        tx_identifier,
                        tx_hash,
                        cert_index: cert_index as u64,
                    })
                }

                conway::Certificate::ResignCommitteeCold(cold_cred, anchor) => {
                    Ok(TxCertificateWithPos {
                        cert: TxCertificate::ResignCommitteeCold(ResignCommitteeCold {
                            cold_credential: map_stake_credential(cold_cred),
                            anchor: map_nullable_anchor(anchor),
                        }),
                        tx_identifier,
                        tx_hash,
                        cert_index: cert_index as u64,
                    })
                }

                conway::Certificate::RegDRepCert(cred, coin, anchor) => Ok(TxCertificateWithPos {
                    cert: TxCertificate::DRepRegistration(DRepRegistration {
                        credential: map_stake_credential(cred),
                        deposit: *coin,
                        anchor: map_nullable_anchor(anchor),
                    }),
                    tx_identifier,
                    tx_hash,
                    cert_index: cert_index as u64,
                }),

                conway::Certificate::UnRegDRepCert(cred, coin) => Ok(TxCertificateWithPos {
                    cert: TxCertificate::DRepDeregistration(DRepDeregistration {
                        credential: map_stake_credential(cred),
                        refund: *coin,
                    }),
                    tx_identifier,
                    tx_hash,
                    cert_index: cert_index as u64,
                }),

                conway::Certificate::UpdateDRepCert(cred, anchor) => Ok(TxCertificateWithPos {
                    cert: TxCertificate::DRepUpdate(DRepUpdate {
                        credential: map_stake_credential(cred),
                        anchor: map_nullable_anchor(anchor),
                    }),
                    tx_identifier,
                    tx_hash,
                    cert_index: cert_index as u64,
                }),
            }
        }

        _ => Err(anyhow!("Unknown certificate era {:?} ignored", cert)),
    }
}
