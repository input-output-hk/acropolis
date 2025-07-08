//! Acropolis AccountsState: State storage
use acropolis_common::{
    messages::{
        DRepStateMessage, EpochActivityMessage, PotDeltasMessage, ProtocolParamsMessage,
        SPOStateMessage, StakeAddressDeltasMessage, TxCertificatesMessage, WithdrawalsMessage,
    },
    serialization::SerializeMapAs,
    DRepChoice, DRepCredential, InstantaneousRewardSource, InstantaneousRewardTarget, KeyHash,
    Lovelace, MoveInstantaneousReward, PoolRegistration, Pot, ProtocolParams, StakeCredential,
    TxCertificate,
};
use anyhow::{bail, Context, Result};
use dashmap::DashMap;
use imbl::OrdMap;
use rayon::prelude::*;
use serde_with::{hex::Hex, serde_as};
use std::collections::BTreeMap;
use std::sync::{atomic::AtomicU64, Arc};
use tracing::{debug, error, info, warn};

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

#[derive(Default, Debug, PartialEq, Eq)]
pub struct DRepDelegationDistribution {
    pub abstain: Lovelace,
    pub no_confidence: Lovelace,
    pub dreps: Vec<(DRepCredential, Lovelace)>,
}

/// Global 'pot' account state
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct Pots {
    /// Unallocated reserves
    reserves: u64,

    /// Treasury
    treasury: u64,

    /// Deposits
    deposits: u64,
}

/// Overall state - stored per block
#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct State {
    /// Epoch this state is for
    epoch: u64,

    /// Map of active SPOs by VRF vkey
    #[serde_as(as = "SerializeMapAs<Hex, _>")]
    spos_by_vrf_key: OrdMap<Vec<u8>, PoolRegistration>,

    /// Map of staking address values
    #[serde_as(as = "SerializeMapAs<Hex, _>")]
    stake_addresses: OrdMap<Vec<u8>, StakeAddressState>,

    /// Global account pots
    pots: Pots,

    /// All registered DReps
    dreps: Vec<(DRepCredential, Lovelace)>,

    /// Protocol parameters that apply during this epoch
    protocol_parameters: Option<ProtocolParams>,
}

impl State {
    /// Get the stake address state for a give stake key
    pub fn get_stake_state(&self, stake_key: &Vec<u8>) -> Option<StakeAddressState> {
        self.stake_addresses.get(stake_key).cloned()
    }

    /// Get the current pot balances
    pub fn get_pots(&self) -> Pots {
        self.pots.clone()
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
        // because imbl::OrdMap doesn't work in Rayon
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
        spo_stakes.iter().map(|entry| ((**entry.key()).clone(), *entry.value())).collect()
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
        self.stake_addresses.values().collect::<Vec<_>>().par_iter().for_each(|state| {
            let Some(drep) = state.delegated_drep.clone() else {
                return;
            };
            let total = match drep {
                DRepChoice::Key(hash) => {
                    let cred = DRepCredential::AddrKeyHash(hash);
                    let Some(total) = dreps.get(&cred) else {
                        warn!("Delegated to unregistered DRep address {cred:?}");
                        return;
                    };
                    total
                }
                DRepChoice::Script(hash) => {
                    let cred = DRepCredential::ScriptHash(hash);
                    let Some(total) = dreps.get(&cred) else {
                        warn!("Delegated to unregistered DRep script {cred:?}");
                        return;
                    };
                    total
                }
                DRepChoice::Abstain => &abstain,
                DRepChoice::NoConfidence => &no_confidence,
            };
            let stake = state.utxo_value + state.rewards;
            total.fetch_add(stake, std::sync::atomic::Ordering::Relaxed);
        });
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

    /// Handle an ProtocolParamsMessage with the latest parameters at the start of a new
    /// epoch
    pub fn handle_parameters(&mut self, params_msg: &ProtocolParamsMessage) -> Result<()> {
        self.protocol_parameters = Some(params_msg.params.clone());
        info!("New parameter set: {:?}", self.protocol_parameters);
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

                None => debug!(
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
        self.spos_by_vrf_key =
            spo_msg.spos.iter().cloned().map(|spo| (spo.vrf_key_hash.clone(), spo)).collect();

        Ok(())
    }

    /// Register a stake address
    fn register_stake_address(&mut self, credential: &StakeCredential) {
        let hash = credential.get_hash();

        // Repeated registrations seem common
        if !self.stake_addresses.contains_key(&hash) {
            self.stake_addresses =
                self.stake_addresses.update(hash.clone(), StakeAddressState::default());
        }
    }

    /// Deregister a stake address
    fn deregister_stake_address(&mut self, credential: &StakeCredential) {
        let hash = credential.get_hash();

        // Check if it existed
        // Repeated registrations seem common
        if self.stake_addresses.contains_key(&hash) {
            self.stake_addresses = self.stake_addresses.without(&hash);
        } else {
            warn!(
                "Deregistraton of unknown stake address {}",
                hex::encode(hash)
            );
        }
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

    /// Handle an MoveInstantaneousReward (pre-Conway only)
    pub fn handle_mir(&mut self, mir: &MoveInstantaneousReward) -> Result<()> {
        let (source, source_name, other, other_name) = match &mir.source {
            InstantaneousRewardSource::Reserves => (
                &mut self.pots.reserves,
                "reserves",
                &mut self.pots.treasury,
                "treasury",
            ),
            InstantaneousRewardSource::Treasury => (
                &mut self.pots.treasury,
                "treasury",
                &mut self.pots.reserves,
                "reserves",
            ),
        };

        match &mir.target {
            InstantaneousRewardTarget::StakeCredentials(deltas) => {
                // Transfer to (in theory also from) stake addresses from (to) a pot
                for (credential, value) in deltas.iter() {
                    let hash = credential.get_hash();

                    // Get old stake address state, or create one
                    let mut sas = match self.stake_addresses.get(&hash) {
                        Some(sas) => sas.clone(),
                        None => StakeAddressState::default(),
                    };

                    // Add to this one
                    Self::update_value_with_delta(&mut sas.rewards, *value)
                        .with_context(|| format!("Updating stake {}", hex::encode(&hash)))?;

                    // Immutably update it
                    self.stake_addresses = self.stake_addresses.update(hash.clone(), sas);

                    // Update the source
                    Self::update_value_with_delta(source, -*value)
                        .with_context(|| format!("Updating {source_name}"))?;
                }
            }

            InstantaneousRewardTarget::OtherAccountingPot(value) => {
                // Transfer between pots
                Self::update_value_with_delta(source, -(*value as i64))
                    .with_context(|| format!("Updating {source_name}"))?;
                Self::update_value_with_delta(other, *value as i64)
                    .with_context(|| format!("Updating {other_name}"))?;
            }
        }

        Ok(())
    }

    /// Update an unsigned value with a signed delta, with fences
    pub fn update_value_with_delta(value: &mut u64, delta: i64) -> Result<()> {
        if delta >= 0 {
            *value = (*value).saturating_add(delta as u64);
        } else {
            let abs = (-delta) as u64;
            if abs > *value {
                bail!("Value underflow - was {}, delta {}", *value, delta);
            } else {
                *value -= abs;
            }
        }

        Ok(())
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

    /// Handle TxCertificates
    pub fn handle_tx_certificates(&mut self, tx_certs_msg: &TxCertificatesMessage) -> Result<()> {
        // Handle certificates
        for tx_cert in tx_certs_msg.certificates.iter() {
            match tx_cert {
                TxCertificate::StakeRegistration(sc_with_pos) => {
                    self.register_stake_address(&sc_with_pos.stake_credential);
                }

                TxCertificate::StakeDeregistration(sc) => {
                    self.deregister_stake_address(&sc);
                }

                TxCertificate::MoveInstantaneousReward(mir) => {
                    self.handle_mir(&mir).unwrap_or_else(|e| debug!("MIR failed: {e:#}"));
                }

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

    /// Handle withdrawals
    pub fn handle_withdrawals(&mut self, withdrawals_msg: &WithdrawalsMessage) -> Result<()> {
        for withdrawal in withdrawals_msg.withdrawals.iter() {
            let hash = withdrawal.address.get_hash();

            // Get old stake address state - which must exist
            let mut sas = match self.stake_addresses.get(hash) {
                Some(sas) => sas.clone(),
                None => bail!(
                    "Unknown stake address in withdrawal: {:?}",
                    withdrawal.address
                ),
            };

            debug!(
                "Withdrawal of {} from stake key {}",
                withdrawal.value,
                hex::encode(hash)
            );

            Self::update_value_with_delta(&mut sas.rewards, -(withdrawal.value as i64))
                .with_context(|| format!("Withdrawing from stake address {}", hex::encode(hash)))?;

            // Immutably create or update the stake address
            self.stake_addresses = self.stake_addresses.update(hash.to_vec(), sas);
        }

        Ok(())
    }

    /// Handle pots
    pub fn handle_pot_deltas(&mut self, pot_deltas_msg: &PotDeltasMessage) -> Result<()> {
        for pot_delta in pot_deltas_msg.deltas.iter() {
            let pot = match pot_delta.pot {
                Pot::Reserves => &mut self.pots.reserves,
                Pot::Treasury => &mut self.pots.treasury,
                Pot::Deposits => &mut self.pots.deposits,
            };

            Self::update_value_with_delta(pot, pot_delta.delta)
                .with_context(|| format!("Applying pot delta {pot_delta:?}"))?;

            info!(
                "Pot delta for {:?} {} => {}",
                pot_delta.pot, pot_delta.delta, *pot
            );
        }

        Ok(())
    }

    /// Handle stake deltas
    pub fn handle_stake_deltas(&mut self, deltas_msg: &StakeAddressDeltasMessage) -> Result<()> {
        // Handle deltas
        for delta in deltas_msg.deltas.iter() {
            // Fold both stake key and script hashes into one - assuming the chance of
            // collision is negligible
            let hash = delta.address.get_hash();

            // Get old stake address state, or create one
            let mut sas = match self.stake_addresses.get(hash) {
                Some(sas) => sas.clone(),
                None => StakeAddressState::default(),
            };

            Self::update_value_with_delta(&mut sas.utxo_value, delta.delta)
                .with_context(|| format!("Updating stake {}", hex::encode(hash)))?;

            // Immutably create or update the stake address
            self.stake_addresses = self.stake_addresses.update(hash.to_vec(), sas);
        }

        Ok(())
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{
        rational_number::RationalNumber, AddressNetwork, Anchor, Committee, Constitution,
        ConwayParams, Credential, DRepVotingThresholds, PoolVotingThresholds, Pot, PotDelta,
        ProtocolParams, Registration, StakeAddress, StakeAddressDelta, StakeAddressPayload,
        StakeAndVoteDelegation, StakeRegistrationAndStakeAndVoteDelegation,
        StakeRegistrationAndVoteDelegation, UnitInterval, VoteDelegation, Withdrawal,
    };

    const STAKE_KEY_HASH: [u8; 3] = [0x99, 0x0f, 0x00];
    const DREP_HASH: [u8; 4] = [0xca, 0xfe, 0xd0, 0x0d];

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
            state.stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().utxo_value,
            42
        );

        state.handle_stake_deltas(&msg).unwrap();

        assert_eq!(state.stake_addresses.len(), 1);
        assert_eq!(
            state.stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().utxo_value,
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
            state.stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().utxo_value,
            84
        );
        assert_eq!(
            state2.stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().utxo_value,
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

    #[test]
    fn pots_are_zero_at_start() {
        let state = State::default();
        assert_eq!(state.pots.reserves, 0);
        assert_eq!(state.pots.treasury, 0);
        assert_eq!(state.pots.deposits, 0);
    }

    #[test]
    fn pot_delta_updates_pots() {
        let mut state = State::default();

        // Send in a MIR reserves->42->treasury
        let mir = PotDeltasMessage {
            deltas: vec![
                PotDelta {
                    pot: Pot::Reserves,
                    delta: 43,
                },
                PotDelta {
                    pot: Pot::Reserves,
                    delta: -1,
                },
                PotDelta {
                    pot: Pot::Treasury,
                    delta: 99,
                },
                PotDelta {
                    pot: Pot::Deposits,
                    delta: 77,
                },
            ],
        };

        state.handle_pot_deltas(&mir).unwrap();
        assert_eq!(state.pots.reserves, 42);
        assert_eq!(state.pots.treasury, 99);
        assert_eq!(state.pots.deposits, 77);
    }

    #[test]
    fn mir_transfers_between_pots() {
        let mut state = State::default();

        // Bootstrap with some in reserves
        state.pots.reserves = 100;

        // Send in a MIR reserves->42->treasury
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Reserves,
            target: InstantaneousRewardTarget::OtherAccountingPot(42),
        };

        state.handle_mir(&mir).unwrap();
        assert_eq!(state.pots.reserves, 58);
        assert_eq!(state.pots.treasury, 42);
        assert_eq!(state.pots.deposits, 0);

        // Send some of it back
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Treasury,
            target: InstantaneousRewardTarget::OtherAccountingPot(10),
        };

        state.handle_mir(&mir).unwrap();
        assert_eq!(state.pots.reserves, 68);
        assert_eq!(state.pots.treasury, 32);
        assert_eq!(state.pots.deposits, 0);
    }

    #[test]
    fn mir_transfers_to_stake_addresses() {
        let mut state = State::default();

        // Bootstrap with some in reserves
        state.pots.reserves = 100;

        // Set up one stake address
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&STAKE_KEY_HASH),
                delta: 99,
            }],
        };

        state.handle_stake_deltas(&msg).unwrap();
        assert_eq!(state.stake_addresses.len(), 1);

        let sas = state.stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
        assert_eq!(sas.utxo_value, 99);
        assert_eq!(sas.rewards, 0);

        // Send in a MIR reserves->{47,-5}->stake
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Reserves,
            target: InstantaneousRewardTarget::StakeCredentials(vec![
                (Credential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), 47),
                (Credential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), -5),
            ]),
        };

        state.handle_mir(&mir).unwrap();
        assert_eq!(state.pots.reserves, 58);
        assert_eq!(state.pots.treasury, 0);
        assert_eq!(state.pots.deposits, 0);

        let sas = state.stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
        assert_eq!(sas.utxo_value, 99);
        assert_eq!(sas.rewards, 42);
    }

    #[test]
    fn withdrawal_transfers_from_stake_addresses() {
        let mut state = State::default();

        // Bootstrap with some in reserves
        state.pots.reserves = 100;

        // Set up one stake address
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&STAKE_KEY_HASH),
                delta: 99,
            }],
        };

        state.handle_stake_deltas(&msg).unwrap();
        assert_eq!(state.stake_addresses.len(), 1);

        let sas = state.stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
        assert_eq!(sas.utxo_value, 99);
        assert_eq!(sas.rewards, 0);

        // Send in a MIR reserves->42->stake
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Reserves,
            target: InstantaneousRewardTarget::StakeCredentials(vec![(
                Credential::AddrKeyHash(STAKE_KEY_HASH.to_vec()),
                42,
            )]),
        };

        state.handle_mir(&mir).unwrap();
        assert_eq!(state.pots.reserves, 58);
        let sas = state.stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
        assert_eq!(sas.rewards, 42);

        // Withdraw most of it
        let withdrawals = WithdrawalsMessage {
            withdrawals: vec![Withdrawal {
                address: create_address(&STAKE_KEY_HASH),
                value: 39,
            }],
        };

        state.handle_withdrawals(&withdrawals).unwrap();
        let sas = state.stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
        assert_eq!(sas.rewards, 3);
    }

    #[test]
    fn drdd_is_default_from_start() {
        let state = State::default();
        let drdd = state.generate_drdd();
        assert_eq!(drdd, DRepDelegationDistribution::default());
    }

    #[test]
    fn drdd_includes_initial_deposit() {
        let mut state = State::default();

        let drep_addr_cred = DRepCredential::AddrKeyHash(DREP_HASH.to_vec());
        state.handle_drep_state(&DRepStateMessage {
            epoch: 1337,
            dreps: vec![(drep_addr_cred.clone(), 1_000_000)],
        });

        let drdd = state.generate_drdd();
        assert_eq!(
            drdd,
            DRepDelegationDistribution {
                abstain: 0,
                no_confidence: 0,
                dreps: vec![(drep_addr_cred, 1_000_000)],
            }
        );
    }

    #[test]
    fn drdd_respects_different_delegations() -> Result<()> {
        let mut state = State::default();

        let drep_addr_cred = DRepCredential::AddrKeyHash(DREP_HASH.to_vec());
        let drep_script_cred = DRepCredential::ScriptHash(DREP_HASH.to_vec());
        state.handle_drep_state(&DRepStateMessage {
            epoch: 1337,
            dreps: vec![
                (drep_addr_cred.clone(), 1_000_000),
                (drep_script_cred.clone(), 2_000_000),
            ],
        });

        let spo1 = vec![0x01];
        let spo2 = vec![0x02];
        let spo3 = vec![0x03];
        let spo4 = vec![0x04];

        let certificates = vec![
            // register the first two SPOs separately from their delegation
            TxCertificate::Registration(Registration {
                credential: Credential::AddrKeyHash(spo1.clone()),
                deposit: 1,
            }),
            TxCertificate::Registration(Registration {
                credential: Credential::AddrKeyHash(spo2.clone()),
                deposit: 1,
            }),
            TxCertificate::VoteDelegation(VoteDelegation {
                credential: Credential::AddrKeyHash(spo1.clone()),
                drep: DRepChoice::Key(DREP_HASH.to_vec()),
            }),
            TxCertificate::StakeAndVoteDelegation(StakeAndVoteDelegation {
                credential: Credential::AddrKeyHash(spo2.clone()),
                operator: spo1.clone(),
                drep: DRepChoice::Script(DREP_HASH.to_vec()),
            }),
            TxCertificate::StakeRegistrationAndVoteDelegation(StakeRegistrationAndVoteDelegation {
                credential: Credential::AddrKeyHash(spo3.clone()),
                drep: DRepChoice::Abstain,
                deposit: 1,
            }),
            TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(
                StakeRegistrationAndStakeAndVoteDelegation {
                    credential: Credential::AddrKeyHash(spo4.clone()),
                    operator: spo1.clone(),
                    drep: DRepChoice::NoConfidence,
                    deposit: 1,
                },
            ),
        ];

        state.handle_tx_certificates(&TxCertificatesMessage { certificates })?;

        let deltas = vec![
            StakeAddressDelta {
                address: create_address(&spo1),
                delta: 100,
            },
            StakeAddressDelta {
                address: create_address(&spo2),
                delta: 1_000,
            },
            StakeAddressDelta {
                address: create_address(&spo3),
                delta: 10_000,
            },
            StakeAddressDelta {
                address: create_address(&spo4),
                delta: 100_000,
            },
        ];
        state.handle_stake_deltas(&StakeAddressDeltasMessage { deltas })?;

        let drdd = state.generate_drdd();
        assert_eq!(
            drdd,
            DRepDelegationDistribution {
                abstain: 10_000,
                no_confidence: 100_000,
                dreps: vec![(drep_addr_cred, 1_000_100), (drep_script_cred, 2_001_000),],
            }
        );

        Ok(())
    }

    #[test]
    fn protocol_params_are_captured_from_message() {
        // Fake Conway parameters (a lot of work to test an assignment!)
        let params = ProtocolParams {
            conway: Some(ConwayParams {
                pool_voting_thresholds: PoolVotingThresholds {
                    motion_no_confidence: UnitInterval::ONE,
                    committee_normal: UnitInterval::ZERO,
                    committee_no_confidence: UnitInterval::ZERO,
                    hard_fork_initiation: UnitInterval::ONE,
                    security_voting_threshold: UnitInterval::ZERO,
                },
                d_rep_voting_thresholds: DRepVotingThresholds {
                    motion_no_confidence: UnitInterval::ONE,
                    committee_normal: UnitInterval::ZERO,
                    committee_no_confidence: UnitInterval::ZERO,
                    update_constitution: UnitInterval::ONE,
                    hard_fork_initiation: UnitInterval::ZERO,
                    pp_network_group: UnitInterval::ZERO,
                    pp_economic_group: UnitInterval::ZERO,
                    pp_technical_group: UnitInterval::ZERO,
                    pp_governance_group: UnitInterval::ZERO,
                    treasury_withdrawal: UnitInterval::ONE,
                },
                committee_min_size: 42,
                committee_max_term_length: 3,
                gov_action_lifetime: 99,
                gov_action_deposit: 500_000_000,
                d_rep_deposit: 100_000_000,
                d_rep_activity: 27,
                min_fee_ref_script_cost_per_byte: RationalNumber::new(1, 42).unwrap(),
                plutus_v3_cost_model: Vec::new(),
                constitution: Constitution {
                    anchor: Anchor {
                        url: "constitution.cardano.org".to_string(),
                        data_hash: vec![0x99],
                    },
                    guardrail_script: None,
                },
                committee: Committee {
                    members: std::collections::HashMap::new(),
                    threshold: RationalNumber::new(5, 32).unwrap(),
                },
            }),

            ..ProtocolParams::default()
        };

        let msg = ProtocolParamsMessage {
            params: params.clone(),
        };
        let mut state = State::default();

        state.handle_parameters(&msg).unwrap();

        assert_eq!(
            state.protocol_parameters.unwrap().conway.unwrap().pool_voting_thresholds,
            params.conway.unwrap().pool_voting_thresholds
        );
    }
}
