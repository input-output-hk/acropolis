//! Verification of calculated values against captured CSV from Haskell node / DBSync
use crate::state::Pots;
use crate::rewards::RewardsResult;
use std::collections::BTreeMap;
use tracing::{error, info, warn};

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
            let (epoch, reserves, treasury, deposits): (u64, u64, u64, u64) =
                match result {
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

    /// Verify rewards, logging any errors
    pub fn verify_rewards(&self, epoch: u64, rewards: &RewardsResult) {
        if let Some(template) = &self.rewards_file_template {
            let path = template.replace("{}", &epoch.to_string());

            let mut reader = match csv::Reader::from_path(&path) {
                Ok(reader) => reader,
                Err(err) => {
                    return;
                }
            };

            // Expect CSV header: spo,address,type,amount
            let mut expected_rewards: BTreeMap<String, (String, String, u64)> = BTreeMap::new();
            for result in reader.deserialize() {
                let (spo, address, rtype, amount): (String, String, String, u64) =
                    match result {
                        Ok(row) => row,
                        Err(err) => {
                            error!("Bad row in {path}: {err} - skipping");
                            continue;
                        }
                    };

                expected_rewards.insert(spo, (address, rtype, amount));
            }

            info!("Read rewards verification data for {} SPOs", expected_rewards.len());

            // TODO compare rewards with expected_rewards, log missing members/leaders in both
            // directions, changes of value
        }
    }
}
