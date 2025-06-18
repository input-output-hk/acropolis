//! Acropolis AccountsState: State storage
use acropolis_common::{
    messages::{
        DRepStateMessage, EpochActivityMessage, SPOStateMessage, StakeAddressDeltasMessage,
        TxCertificatesMessage,
    },
    serialization::SerializeMapAs,
    DRepChoice, DRepCredential, KeyHash, Lovelace, PoolRegistration, StakeAddressPayload,
    StakeCredential, TxCertificate,
};
use anyhow::Result;
use dashmap::DashMap;
use imbl::HashMap;
use rayon::prelude::*;
use serde_with::{hex::Hex, serde_as};
use std::sync::Arc;
use std::{collections::BTreeMap, sync::atomic::AtomicU64};
use tracing::{error, info, warn};

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

    /// DRep they are delegated to
    delegated_drep: Option<DRepChoice>,
}

#[derive(Default)]
pub struct DRepDelegationDistribution {
    pub abstain: Lovelace,
    pub no_confidence: Lovelace,
    pub dreps: Vec<(DRepCredential, Lovelace)>,
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

    dreps: Vec<(DRepCredential, Lovelace)>,
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

    /// Derive the Stake Pool Delegation Distribution (SPDD) - a map of total stake value
    /// (including both UTXO stake addresses and rewards) for each active SPO
    /// Key of returned map is the SPO 'operator' ID
    pub fn generate_spdd(&self) -> BTreeMap<KeyHash, u64> {
        // Shareable Dashmap with referenced keys
        let spo_stakes = Arc::new(DashMap::<&KeyHash, u64>::new());

        // Total stake across all addresses in parallel, first collecting into a vector
        // because imbl::HashMap doesn't work in Rayon
        self.stake_addresses
            .values()
            .collect::<Vec<_>>() // Vec<&StakeAddressState>
            .par_iter() // Rayon multi-threaded iterator
            .for_each_init(
                || Arc::clone(&spo_stakes),
                |map, sas| {
                    if let Some(spo) = sas.delegated_spo.as_ref() {
                        let stake = sas.utxo_value + sas.rewards;
                        map.entry(spo).and_modify(|v| *v += stake).or_insert(stake);
                    }
                },
            );

        // Collect into a plain BTreeMap, so that it is ordered on output
        spo_stakes
            .iter()
            .map(|entry| ((**entry.key()).clone(), *entry.value()))
            .collect()
    }

    /// Derive the DRep Delegation Distribution (SPDD) - the total amount
    /// delegated to each DRep, including the special "abstain" and "no confidence" dreps.
    pub fn generate_drdd(&self) -> DRepDelegationDistribution {
        let abstain = AtomicU64::new(0);
        let no_confidence = AtomicU64::new(0);
        let dreps = self
            .dreps
            .iter()
            .map(|(cred, deposit)| (cred.clone(), AtomicU64::new(*deposit)))
            .collect::<BTreeMap<_, _>>();
        for state in self.stake_addresses.values() {
            let Some(drep) = state.delegated_drep.clone() else {
                continue;
            };
            let total = match drep {
                DRepChoice::Key(hash) => {
                    let cred = DRepCredential::AddrKeyHash(hash);
                    let Some(total) = dreps.get(&cred) else {
                        warn!("Delegated to unregistered DRep address {cred:?}");
                        continue;
                    };
                    total
                }
                DRepChoice::Script(hash) => {
                    let cred = DRepCredential::ScriptHash(hash);
                    let Some(total) = dreps.get(&cred) else {
                        warn!("Delegated to unregistered DRep script {cred:?}");
                        continue;
                    };
                    total
                }
                DRepChoice::Abstain => &abstain,
                DRepChoice::NoConfidence => &no_confidence,
            };
            total.fetch_add(state.utxo_value, std::sync::atomic::Ordering::Relaxed);
        }
        let abstain = abstain.load(std::sync::atomic::Ordering::Relaxed);
        let no_confidence = no_confidence.load(std::sync::atomic::Ordering::Relaxed);
        let dreps = dreps
            .into_iter()
            .map(|(k, v)| (k, v.load(std::sync::atomic::Ordering::Relaxed)))
            .collect();
        DRepDelegationDistribution {
            abstain,
            no_confidence,
            dreps,
        }
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

    pub fn handle_drep_state(&mut self, drep_msg: &DRepStateMessage) {
        self.dreps = drep_msg.dreps.clone();
    }

    /// Record a stake delegation
    fn record_stake_delegation(&mut self, credential: &StakeCredential, spo: &KeyHash) {
        let hash = credential.get_hash();

        // Get old stake address state, or create one
        let mut sas = match self.stake_addresses.get(&hash) {
            Some(sas) => sas.clone(),
            None => StakeAddressState::default(),
        };

        // Immutably create or update the stake address
        sas.delegated_spo = Some(spo.clone());
        self.stake_addresses = self.stake_addresses.update(hash.clone(), sas);
    }

    /// record a drep delegation
    fn record_drep_delegation(&mut self, credential: &StakeCredential, drep: &DRepChoice) {
        let hash = credential.get_hash();
        self.stake_addresses = self.stake_addresses.alter(
            |old_state| {
                let mut state = old_state.unwrap_or_default();
                state.delegated_drep = Some(drep.clone());
                Some(state)
            },
            hash,
        );
    }

    /// Handle TxCertificates with stake or drep delegations
    pub fn handle_tx_certificates(&mut self, tx_certs_msg: &TxCertificatesMessage) -> Result<()> {
        // Handle certificates
        for tx_cert in tx_certs_msg.certificates.iter() {
            match tx_cert {
                TxCertificate::StakeDelegation(delegation) => {
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                }

                TxCertificate::VoteDelegation(delegation) => {
                    self.record_drep_delegation(&delegation.credential, &delegation.drep);
                }

                TxCertificate::StakeAndVoteDelegation(delegation) => {
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                    self.record_drep_delegation(&delegation.credential, &delegation.drep);
                }

                TxCertificate::StakeRegistrationAndDelegation(delegation) => {
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                }

                TxCertificate::StakeRegistrationAndVoteDelegation(delegation) => {
                    self.record_drep_delegation(&delegation.credential, &delegation.drep);
                }

                TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(delegation) => {
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                    self.record_drep_delegation(&delegation.credential, &delegation.drep);
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
                None => StakeAddressState::default(),
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
    use acropolis_common::{
        AddressNetwork, Credential, StakeAddress, StakeAddressDelta, StakeAddressPayload,
    };

    const STAKE_KEY_HASH: [u8; 3] = [0x99, 0x0f, 0x00];

    fn create_address(hash: &[u8]) -> StakeAddress {
        StakeAddress {
            network: AddressNetwork::Main,
            payload: StakeAddressPayload::StakeKeyHash(hash.to_vec()),
        }
    }

    #[test]
    fn stake_addresses_initialise_to_first_delta_and_increment_subsequently() {
        let mut state = State::default();
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&STAKE_KEY_HASH),
                delta: 42,
            }],
        };

        state.handle_stake_deltas(&msg).unwrap();

        assert_eq!(state.stake_addresses.len(), 1);
        assert_eq!(
            state
                .stake_addresses
                .get(&STAKE_KEY_HASH.to_vec())
                .unwrap()
                .utxo_value,
            42
        );

        state.handle_stake_deltas(&msg).unwrap();

        assert_eq!(state.stake_addresses.len(), 1);
        assert_eq!(
            state
                .stake_addresses
                .get(&STAKE_KEY_HASH.to_vec())
                .unwrap()
                .utxo_value,
            84
        );
    }

    #[test]
    fn stake_address_changes_dont_leak_across_clone() {
        let mut state = State::default();
        let state2 = state.clone();

        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&STAKE_KEY_HASH),
                delta: 42,
            }],
        };

        state.handle_stake_deltas(&msg).unwrap();

        // New delta must not be reflected in the clone
        assert_eq!(state.stake_addresses.len(), 1);
        assert_eq!(state2.stake_addresses.len(), 0);

        // Clone again and ensure value stays constant too
        let state2 = state.clone();
        state.handle_stake_deltas(&msg).unwrap();
        assert_eq!(
            state
                .stake_addresses
                .get(&STAKE_KEY_HASH.to_vec())
                .unwrap()
                .utxo_value,
            84
        );
        assert_eq!(
            state2
                .stake_addresses
                .get(&STAKE_KEY_HASH.to_vec())
                .unwrap()
                .utxo_value,
            42
        );
    }

    #[test]
    fn spdd_is_empty_at_start() {
        let state = State::default();
        let spdd = state.generate_spdd();
        assert!(spdd.is_empty());
    }

    #[test]
    fn spdd_from_delegation_with_utxo_values() {
        let mut state = State::default();

        // Delegate
        let spo1: KeyHash = vec![0x01];
        let addr1: KeyHash = vec![0x11];
        state.record_stake_delegation(&Credential::AddrKeyHash(addr1.clone()), &spo1);

        let spo2: KeyHash = vec![0x02];
        let addr2: KeyHash = vec![0x12];
        state.record_stake_delegation(&Credential::AddrKeyHash(addr2.clone()), &spo2);

        // Put some value in
        let msg1 = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&addr1),
                delta: 42,
            }],
        };

        state.handle_stake_deltas(&msg1).unwrap();

        let msg2 = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&addr2),
                delta: 21,
            }],
        };

        state.handle_stake_deltas(&msg2).unwrap();

        // Get the SPDD
        let spdd = state.generate_spdd();
        assert_eq!(spdd.len(), 2);

        let stake1 = spdd.get(&spo1).unwrap();
        assert_eq!(*stake1, 42);
        let stake2 = spdd.get(&spo2).unwrap();
        assert_eq!(*stake2, 21);
    }
}
