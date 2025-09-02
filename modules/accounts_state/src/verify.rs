//! Verification of pots values against captured CSV
use crate::state::Pots;
use anyhow::Result;
use std::collections::BTreeMap;
use tracing::{error, info, warn};

/// Pots verifier
pub struct PotsVerifier {
    /// Map of pots values for every epoch
    epoch_pots: BTreeMap<u64, Pots>,
}

impl PotsVerifier {
    /// Read a CSV file
    pub fn new(path: &str) -> Result<Self> {
        let mut reader = csv::Reader::from_path(path)?;
        let mut epoch_pots = BTreeMap::new();

        // Expect CSV header: epoch,reserves,treasury,deposits
        for result in reader.deserialize() {
            let (epoch, reserves, treasury, deposits): (u64, u64, u64, u64) = result?;
            epoch_pots.insert(
                epoch,
                Pots {
                    reserves,
                    treasury,
                    deposits,
                },
            );
        }

        Ok(Self { epoch_pots })
    }

    /// Verify an epoch, logging any errors
    pub fn verify(&self, epoch: u64, pots: &Pots) {
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
}
