//! Verification of calculated values against captured CSV from Haskell node / DBSync
use crate::rewards::{RewardDetail, RewardType, RewardsResult};
use crate::state::Pots;
use acropolis_common::{KeyHash, StakeAddress};
use hex::FromHex;
use itertools::EitherOrBoth::{Both, Left, Right};
use itertools::Itertools;
use std::cmp::Ordering;
use std::collections::BTreeMap;
use tracing::{debug, error, info, warn};

/// Verifier
pub struct Verifier {
    /// Map of pots values for every epoch
    epoch_pots: BTreeMap<u64, Pots>,

    /// Template (with {} for epoch) for rewards files
    rewards_file_template: Option<String>,
}

impl Verifier {
    /// Construct empty
    pub fn new() -> Self {
        Self {
            epoch_pots: BTreeMap::new(),
            rewards_file_template: None,
        }
    }

    /// Read in a pots file
    pub fn read_pots(&mut self, path: &str) {
        let mut reader = match csv::Reader::from_path(path) {
            Ok(reader) => reader,
            Err(err) => {
                error!("Failed to load pots CSV from {path}: {err} - not verifying");
                return;
            }
        };

        // Expect CSV header: epoch,reserves,treasury,deposits
        for result in reader.deserialize() {
            let (epoch, reserves, treasury, deposits): (u64, u64, u64, u64) = match result {
                Ok(row) => row,
                Err(err) => {
                    error!("Bad row in {path}: {err} - skipping");
                    continue;
                }
            };

            self.epoch_pots.insert(
                epoch,
                Pots {
                    reserves,
                    treasury,
                    deposits,
                },
            );
        }
    }

    /// Read in rewards files
    // Actually just stores the template and reads them on demand
    pub fn read_rewards(&mut self, path: &str) {
        self.rewards_file_template = Some(path.to_string());
    }

    /// Verify an epoch, logging any errors
    pub fn verify_pots(&self, epoch: u64, pots: &Pots) {
        if self.epoch_pots.is_empty() {
            return;
        }

        if let Some(desired_pots) = self.epoch_pots.get(&epoch) {
            if pots.reserves != desired_pots.reserves {
                error!(
                    epoch = epoch,
                    calculated = pots.reserves,
                    desired = desired_pots.reserves,
                    difference = desired_pots.reserves as i64 - pots.reserves as i64,
                    "Verification mismatch: reserves for"
                );
            }

            if pots.treasury != desired_pots.treasury {
                error!(
                    epoch = epoch,
                    calculated = pots.treasury,
                    desired = desired_pots.treasury,
                    difference = desired_pots.treasury as i64 - pots.treasury as i64,
                    "Verification mismatch: treasury for"
                );
            }

            if pots.deposits != desired_pots.deposits {
                error!(
                    epoch = epoch,
                    calculated = pots.deposits,
                    desired = desired_pots.deposits,
                    difference = desired_pots.deposits as i64 - pots.deposits as i64,
                    "Verification mismatch: deposits for"
                );
            }

            if pots == desired_pots {
                info!(epoch = epoch, "Verification success for");
            }
        } else {
            warn!("Epoch {epoch} not represented in verify test data");
        }
    }

    /// Sort rewards for zipper compare - type first, then by account
    fn sort_rewards(left: &RewardDetail, right: &RewardDetail) -> Ordering {
        match (&left.rtype, &right.rtype) {
            (RewardType::Leader, RewardType::Member) => Ordering::Less,
            (RewardType::Member, RewardType::Leader) => Ordering::Greater,
            _ => left.account.get_hash().cmp(&right.account.get_hash()),
        }
    }

    /// Verify rewards, logging any errors
    pub fn verify_rewards(&self, epoch: u64, rewards: &RewardsResult) {
        if let Some(template) = &self.rewards_file_template {
            let path = template.replace("{}", &epoch.to_string());

            // Silently return if there's no file for it
            let mut reader = match csv::Reader::from_path(&path) {
                Ok(reader) => reader,
                _ => return,
            };

            // Expect CSV header: spo,address,type,amount
            let mut expected_rewards: BTreeMap<KeyHash, Vec<RewardDetail>> = BTreeMap::new();
            for result in reader.deserialize() {
                let (spo, address, rtype, amount): (String, String, String, u64) = match result {
                    Ok(row) => row,
                    Err(err) => {
                        error!("Bad row in {path}: {err} - skipping");
                        continue;
                    }
                };

                let Ok(spo) = Vec::from_hex(&spo) else {
                    error!("Bad hex in {path} for SPO: {spo} - skipping");
                    continue;
                };
                let Ok(account) = Vec::from_hex(&address) else {
                    error!("Bad hex in {path} for address: {address} - skipping");
                    continue;
                };

                // Ignore 0 amounts
                if amount == 0 {
                    continue;
                }

                // Convert from string and ignore refunds
                let rtype = match rtype.as_str() {
                    "leader" => RewardType::Leader,
                    "member" => RewardType::Member,
                    _ => continue,
                };

                let Ok(stake_address) = StakeAddress::from_binary(&account) else {
                    error!("Bad stake address in {path} for address: {address} - skipping");
                    continue;
                };

                expected_rewards.entry(spo).or_default().push(RewardDetail {
                    account: stake_address,
                    rtype,
                    amount,
                });
            }

            info!(
                epoch,
                "Read rewards verification data for {} SPOs",
                expected_rewards.len()
            );

            // TODO compare rewards with expected_rewards, log missing members/leaders in both
            // directions, changes of value
            let mut errors: usize = 0;
            for either in expected_rewards
                .into_iter()
                .merge_join_by(rewards.rewards.clone().into_iter(), |i, j| i.0.cmp(&j.0))
            {
                match either {
                    Left(expected_spo) => {
                        error!(
                            "Missing rewards SPO: {} {} rewards",
                            hex::encode(&expected_spo.0),
                            expected_spo.1.len()
                        );
                        errors += 1;
                    }
                    Right(actual_spo) => {
                        error!(
                            "Extra rewards SPO: {} {} rewards",
                            hex::encode(&actual_spo.0),
                            actual_spo.1.len()
                        );
                        errors += 1;
                    }
                    Both(mut expected_spo, mut actual_spo) => {
                        expected_spo.1.sort_by(Self::sort_rewards);
                        actual_spo.1.sort_by(Self::sort_rewards);
                        for either in expected_spo
                            .1
                            .into_iter()
                            .merge_join_by(actual_spo.1.into_iter(), |i, j| {
                                Self::sort_rewards(i, j)
                            })
                        {
                            match either {
                                Left(expected) => {
                                    error!(
                                        "Missing reward: SPO {} account {} {:?} {}",
                                        hex::encode(&expected_spo.0),
                                        hex::encode(&expected.account.get_hash()),
                                        expected.rtype,
                                        expected.amount
                                    );
                                    errors += 1;
                                }
                                Right(actual) => {
                                    error!(
                                        "Extra reward: SPO {} account {} {:?} {}",
                                        hex::encode(&actual_spo.0),
                                        hex::encode(&actual.account.get_hash()),
                                        actual.rtype,
                                        actual.amount
                                    );
                                    errors += 1;
                                }
                                Both(expected, actual) => {
                                    if expected.amount != actual.amount {
                                        error!("Different reward: SPO {} account {} {:?} expected {}, actual {} ({})",
                                               hex::encode(&expected_spo.0),
                                               hex::encode(&expected.account.get_hash()),
                                               expected.rtype,
                                               expected.amount,
                                               actual.amount,
                                               actual.amount as i64-expected.amount as i64);
                                        errors += 1;
                                    } else {
                                        debug!(
                                            "Reward match: SPO {} account {} {:?} {}",
                                            hex::encode(&expected_spo.0),
                                            hex::encode(&expected.account.get_hash()),
                                            expected.rtype,
                                            expected.amount
                                        );
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if errors == 0 {
                info!(epoch, "Rewards verification OK");
            } else {
                error!(errors, epoch, "Rewards verification:");
            }
        }
    }
}
