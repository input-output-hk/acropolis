//! Acropolis AccountsState: State storage
use acropolis_common::{
    messages::{
        EpochActivityMessage, SPOStateMessage, StakeAddressDeltasMessage, TxCertificatesMessage,
    },
    serialization::SerializeMapAs,
    KeyHash, PoolRegistration, StakeAddressPayload, StakeCredential, TxCertificate,
};
use anyhow::Result;
use imbl::HashMap;
use serde_with::{hex::Hex, serde_as};
use tracing::{error, info};

/// State of an individual stake address
#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct StakeAddressState {
    /// Total value in UTXO addresses
    utxo_value: u64,

    /// Value in reward account
    rewards: u64,

    /// SPO ID they are delegated to ("operator" ID)
    #[serde_as(as = "Option<Hex>")]
    delegated_spo: Option<KeyHash>,
}

/// Overall state - stored per block
#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct State {
    /// Epoch this state is for
    epoch: u64,

    /// Map of active SPOs by VRF vkey
    #[serde_as(as = "SerializeMapAs<Hex, _>")]
    spos_by_vrf_key: HashMap<Vec<u8>, PoolRegistration>,

    /// Map of staking address values
    #[serde_as(as = "SerializeMapAs<Hex, _>")]
    stake_addresses: HashMap<Vec<u8>, StakeAddressState>,
}

impl State {
    /// Get the stake address state for a give stake key
    pub fn get_stake_state(&self, stake_key: &Vec<u8>) -> Option<StakeAddressState> {
        self.stake_addresses.get(stake_key).cloned()
    }

    /// Log statistics
    fn log_stats(&self) {
        info!(num_stake_addresses = self.stake_addresses.keys().len(),);
    }

    /// Background tick
    pub async fn tick(&self) -> Result<()> {
        self.log_stats();
        Ok(())
    }

    /// Handle an EpochActivityMessage giving total fees and block counts by VRF key for
    /// the just-ended epoch
    pub fn handle_epoch_activity(&mut self, ea_msg: &EpochActivityMessage) -> Result<()> {
        self.epoch = ea_msg.epoch;

        // Look up every VRF key in the SPO map
        for (vrf_vkey_hash, count) in ea_msg.vrf_vkey_hashes.iter() {
            match self.spos_by_vrf_key.get(vrf_vkey_hash) {
                Some(spo) => {
                    // !TODO count rewards for this block
                }

                None => error!(
                    "VRF vkey {} not found in SPO map",
                    hex::encode(vrf_vkey_hash)
                ),
            }
        }

        Ok(())
    }

    /// Handle an SPOStateMessage with the full set of SPOs valid at the end of the last
    /// epoch
    pub fn handle_spo_state(&mut self, spo_msg: &SPOStateMessage) -> Result<()> {
        // Capture current SPOs, mapped by VRF vkey hash
        self.spos_by_vrf_key = spo_msg
            .spos
            .iter()
            .cloned()
            .map(|spo| (spo.vrf_key_hash.clone(), spo))
            .collect();

        Ok(())
    }

    /// Record a delegation
    fn record_delegation(&mut self, credential: &StakeCredential, spo: &KeyHash) {
        let hash = credential.get_hash();

        // Get old stake address state, or create one
        let mut sas = match self.stake_addresses.get(&hash) {
            Some(sas) => sas.clone(),
            None => StakeAddressState::default()
        };

        // Immutably create or update the stake address
        sas.delegated_spo = Some(spo.clone());
        self.stake_addresses = self.stake_addresses.update(hash.clone(), sas);
    }

    /// Handle TxCertificates with stake delegations
    pub fn handle_tx_certificates(&mut self, tx_certs_msg: &TxCertificatesMessage) -> Result<()> {
        // Handle certificates
        for tx_cert in tx_certs_msg.certificates.iter() {
            match tx_cert {
                TxCertificate::StakeDelegation(delegation) => {
                    self.record_delegation(&delegation.credential, &delegation.operator);
                }

                TxCertificate::StakeAndVoteDelegation(delegation) => {
                    self.record_delegation(&delegation.credential, &delegation.operator)
                }

                TxCertificate::StakeRegistrationAndDelegation(delegation) => {
                    self.record_delegation(&delegation.credential, &delegation.operator)
                }

                TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(delegation) => {
                    self.record_delegation(&delegation.credential, &delegation.operator)
                }

                _ => (),
            }
        }

        Ok(())
    }

    /// Handle stake deltas
    pub fn handle_stake_deltas(&mut self, deltas_msg: &StakeAddressDeltasMessage) -> Result<()> {
        // Handle deltas
        for delta in deltas_msg.deltas.iter() {
            // Fold both stake key and script hashes into one - assuming the chance of
            // collision is negligible
            let hash = match &delta.address.payload {
                StakeAddressPayload::StakeKeyHash(hash) => hash,
                StakeAddressPayload::ScriptHash(hash) => hash,
            };

            // Get old stake address state, or create one
            let mut sas = match self.stake_addresses.get(hash) {
                Some(sas) => sas.clone(),
                None => StakeAddressState::default()
            };

            // Update UTXO value, with fences
            if delta.delta >= 0 {
                sas.utxo_value = sas.utxo_value.saturating_add(delta.delta as u64);
            } else {
                let abs = (-delta.delta) as u64;
                if abs > sas.utxo_value {
                    error!("Stake address went negative in delta {:?}", delta);
                    sas.utxo_value = 0;
                } else {
                    sas.utxo_value -= abs;
                }
            }

            // Immutably create or update the stake address
            self.stake_addresses = self.stake_addresses.update(hash.clone(), sas);
        }

        Ok(())
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{BlockInfo, BlockStatus, Era};

    fn new_msg() -> EpochActivityMessage {
        EpochActivityMessage {
            epoch: 0,
            total_blocks: 0,
            total_fees: 0,
            vrf_vkey_hashes: Vec::new(),
        }
    }

    fn new_block() -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            slot: 0,
            number: 0,
            hash: Vec::<u8>::new(),
            epoch: 0,
            new_epoch: true,
            era: Era::Byron,
        }
    }
}
