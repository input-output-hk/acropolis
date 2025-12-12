//! Verification of calculated values against captured CSV from Haskell node / DBSync
use crate::{
    rewards::{RewardDetail, RewardsResult},
    state::Pots,
};
use acropolis_common::{BlockInfo, DelegatedStake, Lovelace, PoolId, RewardType, StakeAddress};
use hex::FromHex;
use itertools::{
    EitherOrBoth::{Both, Left, Right},
    Itertools,
};
use std::{cmp::Ordering, collections::BTreeMap, fs::File};
use tracing::{debug, error, info, warn};

/// Verifier
pub struct Verifier {
    /// Map of pots values for every epoch
    epoch_pots: BTreeMap<u64, Pots>,

    /// Template (with {} for epoch) for SPDD reference data files
    spdd_file_template: Option<String>,

    /// Template (with {} for epoch) for rewards files
    rewards_file_template: Option<String>,
}

impl Verifier {
    /// Construct empty
    pub fn new() -> Self {
        Self {
            epoch_pots: BTreeMap::new(),
            spdd_file_template: None,
            rewards_file_template: None,
        }
    }

    fn get_reader(
        &self,
        template: &Option<String>,
        epoch: u64,
    ) -> Option<(String, csv::Reader<File>)> {
        let Some(template) = template else {
            return None;
        };

        let path = template.replace("{}", &epoch.to_string());

        // Silently return None if there's no file for it
        match csv::Reader::from_path(&path) {
            Ok(filename) => Some((path, filename)),
            Err(_e) => None,
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
    pub fn set_rewards_template(&mut self, path: &str) {
        self.rewards_file_template = Some(path.to_string());
    }

    pub fn set_spdd_template(&mut self, path: &str) {
        self.spdd_file_template = Some(path.to_string());
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
            _ => left.account.get_credential().cmp(&right.account.get_credential()),
        }
    }

    /// Verify rewards, logging any errors
    pub fn verify_rewards(&self, rewards: &RewardsResult) {
        let epoch = rewards.epoch;
        if let Some((path, mut reader)) = self.get_reader(&self.rewards_file_template, epoch) {
            // Expect CSV header: spo,address,type,amount
            let mut expected_rewards: BTreeMap<PoolId, Vec<RewardDetail>> = BTreeMap::new();
            for result in reader.deserialize() {
                let (spo, address, rtype, amount): (String, String, String, u64) = match result {
                    Ok(row) => row,
                    Err(err) => {
                        error!("Bad row in {path}: {err} - skipping");
                        continue;
                    }
                };

                let Some(spo) =
                    Vec::from_hex(&spo).ok().and_then(|bytes| PoolId::try_from(bytes).ok())
                else {
                    error!("Bad hex/SPO in {path} for SPO: {spo} - skipping");
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
                    pool: spo,
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
                            expected_spo.0,
                            expected_spo.1.len()
                        );
                        errors += 1;
                    }
                    Right(actual_spo) => {
                        error!(
                            "Extra rewards SPO: {} {} rewards",
                            actual_spo.0,
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
                                        expected_spo.0,
                                        expected.account,
                                        expected.rtype,
                                        expected.amount
                                    );
                                    errors += 1;
                                }
                                Right(actual) => {
                                    error!(
                                        "Extra reward: SPO {} account {} {:?} {}",
                                        actual_spo.0, actual.account, actual.rtype, actual.amount
                                    );
                                    errors += 1;
                                }
                                Both(expected, actual) => {
                                    if expected.amount != actual.amount {
                                        error!("Different reward: SPO {} account {} {:?} expected {}, actual {} ({})",
                                               expected_spo.0,
                                               expected.account,
                                               expected.rtype,
                                               expected.amount,
                                               actual.amount,
                                               actual.amount as i64-expected.amount as i64);
                                        errors += 1;
                                    } else {
                                        debug!(
                                            "Reward match: SPO {} account {} {:?} {}",
                                            expected_spo.0,
                                            expected.account,
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

    #[allow(clippy::question_mark)]
    fn read_spdd(&self, epoch: u64) -> Option<BTreeMap<PoolId, Lovelace>> {
        let mut reference_spdd: BTreeMap<PoolId, Lovelace> = BTreeMap::new();

        let Some((path, mut reader)) = self.get_reader(&self.spdd_file_template, epoch) else {
            return None;
        };

        for result in reader.deserialize() {
            let (spo, amount): (String, Lovelace) = match result {
                Ok(row) => row,
                Err(err) => {
                    error!("Bad row in {path}: {err} - skipping");
                    continue;
                }
            };

            let Some(spo) = Vec::from_hex(&spo).ok().and_then(|bytes| PoolId::try_from(bytes).ok())
            else {
                error!("Bad hex/SPO in {path} for SPO: {spo} - skipping");
                continue;
            };

            if let Some(old) = reference_spdd.insert(spo, amount) {
                error!("Double entry in {path} for {spo}: replacing {amount} with {old}");
                continue;
            }
        }

        Some(reference_spdd)
    }

    pub fn verify_spdd(&self, blk: &BlockInfo, spdd: &BTreeMap<PoolId, DelegatedStake>) {
        let epoch = blk.epoch - 1;
        let Some(reference) = self.read_spdd(epoch) else {
            // No reference = no verification; silently exiting.
            return;
        };

        let (outcome, total, _, _, _) = Self::verify_spdd_impl(epoch, spdd, &reference);
        if outcome {
            info!("Verification of SPDD, end of epoch {epoch}: OK, total active stake {total}");
        } else {
            error!("Verification of SPDD, end of epoch {epoch}: Failed");
        }
    }

    pub fn verify_spdd_impl(
        epoch: u64,
        spdd: &BTreeMap<PoolId, DelegatedStake>,
        reference: &BTreeMap<PoolId, Lovelace>,
    ) -> (bool, Lovelace, usize, usize, usize) {
        let mut different = Vec::new();
        let mut extra = Vec::new();
        let mut missing = Vec::new();

        // Compare the SPDD table by checking three properties:
        // 1. All values from Computed SPDD, which present in Reference, are equal
        // 2. There are no non-zero values from Computed SPDD, which are absent in Reference.

        let mut total_computed = 0;
        for (pool, computed_stake) in spdd.iter() {
            total_computed += computed_stake.live;
            if let Some(ref_stake) = reference.get(pool) {
                if *ref_stake != computed_stake.live {
                    different.push((pool, ref_stake, computed_stake.live));
                }
            } else if computed_stake.live != 0 {
                extra.push((pool, computed_stake.live));
            }
        }

        // 3. All non-zero values from Reference must present in Computed SPDD.

        let mut total_reference = 0;
        for (pool, ref_stake) in reference.iter() {
            total_reference += ref_stake;
            if *ref_stake != 0 && spdd.get(pool).is_none() {
                missing.push((pool, ref_stake));
            }
        }

        // Check whether we have everything correct

        if total_computed == total_reference
            && different.is_empty()
            && extra.is_empty()
            && missing.is_empty()
        {
            return (true, total_computed, 0, 0, 0);
        }

        // There are some errors, print them

        if total_computed != total_reference {
            error!(
                "SPDD verification epoch {epoch} total active stake difference: \
                reference {total_reference} != computed {total_computed}"
            );
        }

        for (p, e, s) in different.iter() {
            error!("SPDD verification epoch {epoch}, {p}: ref {e} != comp {s}");
        }

        for (p, s) in extra.iter() {
            error!("SPDD verification epoch {epoch}, {p}: No ref, comp {s}");
        }

        for (p, e) in missing.iter() {
            error!("SPDD verification epoch {epoch}, {p}: ref {e}, No comp");
        }

        (
            false,
            total_computed,
            different.len(),
            extra.len(),
            missing.len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_spdd() {
        let test_cases: [(Option<Lovelace>, Option<Lovelace>); 10] = [
            // Comparing with None
            (Some(0), None),
            (Some(1), None),
            (None, Some(0)),
            (None, Some(1)),
            // Comparing with Zero
            (Some(0), Some(0)),
            (Some(0), Some(10)),
            (Some(10), Some(0)),
            // Comparing Non-zero and Non-zero
            (Some(2), Some(2)),
            (Some(2), Some(3)),
            (Some(3), Some(2)),
        ];

        let mut spdd = BTreeMap::new();
        let mut reference = BTreeMap::new();

        for (idx, (cmp, refr)) in test_cases.iter().enumerate() {
            let poolid = PoolId::from([idx as u8; 28]);

            if let Some(cmp) = cmp {
                spdd.insert(
                    poolid,
                    DelegatedStake {
                        active: *cmp,
                        active_delegators_count: 1,
                        live: *cmp,
                    },
                );
            }

            if let Some(refr) = refr {
                reference.insert(poolid, *refr);
            }
        }

        assert_eq!(
            Verifier::verify_spdd_impl(0, &spdd, &reference),
            (false, 18, 4, 1, 1)
        );
    }

    #[test]
    fn test_read_spdd() {
        let mut verifier = Verifier::new();
        verifier.spdd_file_template = Some("./test-data/spdd-test.{}.csv".to_string());
        let res = verifier.read_spdd(99999);
        let refr = BTreeMap::from([
            (PoolId::from([1; 28]), 1000),
            (PoolId::from([0xee; 28]), 1111),
        ]);
        assert_eq!(res, Some(refr))
    }
}
