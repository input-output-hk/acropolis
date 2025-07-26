//! Acropolis AccountsState: State storage
use acropolis_common::{
    messages::{
        DRepStateMessage, EpochActivityMessage, PotDeltasMessage, ProtocolParamsMessage,
        SPOStateMessage, StakeAddressDeltasMessage, TxCertificatesMessage, WithdrawalsMessage,
    },
    DelegatedStake,
    DRepChoice, DRepCredential, InstantaneousRewardSource, InstantaneousRewardTarget, KeyHash,
    Lovelace, MoveInstantaneousReward, PoolRegistration, Pot, ProtocolParams,
    StakeAddress, StakeCredential, TxCertificate,
};
use crate::snapshot::Snapshot;
use crate::rewards::RewardsState;
use anyhow::{bail, Result};
use dashmap::DashMap;
use imbl::OrdMap;
use std::collections::{HashMap, BTreeMap, HashSet};
use rayon::prelude::*;
use serde_with::{hex::Hex, serde_as};
use std::sync::{atomic::AtomicU64, Arc, Mutex};
use tracing::{debug, error, info, warn};
use std::mem::take;

const DEFAULT_KEY_DEPOSIT: u64 = 2_000_000;
const DEFAULT_POOL_DEPOSIT: u64 = 500_000_000;

/// State of an individual stake address
#[serde_as]
#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct StakeAddressState {
    /// Is it registered (or only used in addresses)?
    pub registered: bool,

    /// Total value in UTXO addresses
    pub utxo_value: u64,

    /// Value in reward account
    pub rewards: u64,

    /// SPO ID they are delegated to ("operator" ID)
    #[serde_as(as = "Option<Hex>")]
    pub delegated_spo: Option<KeyHash>,

    /// DRep they are delegated to
    pub delegated_drep: Option<DRepChoice>,
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
    pub reserves: Lovelace,

    /// Treasury
    pub treasury: Lovelace,

    /// Deposits
    pub deposits: Lovelace,
}

/// Overall state - stored per block
#[derive(Debug, Default, Clone)]
pub struct State {
    /// Map of active SPOs by operator ID
    spos: OrdMap<KeyHash, PoolRegistration>,

    /// Map of staking address values
    /// Wrapped in an Arc so it doesn't get cloned in full by StateHistory
    stake_addresses: Arc<Mutex<HashMap<KeyHash, StakeAddressState>>>,

    /// Reward state - short history of snapshots
    rewards_state: RewardsState,

    /// Global account pots
    pots: Pots,

    /// All registered DReps
    dreps: Vec<(DRepCredential, Lovelace)>,

    /// Protocol parameters that apply during this epoch
    protocol_parameters: Option<ProtocolParams>,

    /// Pool refunds to apply next epoch (list of reward accounts to refund to)
    pool_refunds: Vec<KeyHash>,

    // Stake address refunds to apply next epoch
    stake_refunds: Vec<(KeyHash, Lovelace)>,

    // MIRs to pay next epoch
    mirs: Vec<MoveInstantaneousReward>,
}

impl State {
    /// Get the stake address state for a give stake key
    pub fn get_stake_state(&self, stake_key: &KeyHash) -> Option<StakeAddressState> {
        self.stake_addresses.lock().unwrap().get(stake_key).cloned()
    }

    /// Get the current pot balances
    pub fn get_pots(&self) -> Pots {
        self.pots.clone()
    }

    /// Log statistics
    fn log_stats(&self) {
        info!(num_stake_addresses = self.stake_addresses.lock().unwrap().keys().len(),);
    }

    /// Background tick
    pub async fn tick(&self) -> Result<()> {
        self.log_stats();
        Ok(())
    }

    /// Process entry into a new epoch
    ///   epoch: Number of epoch we are entering
    ///   total_fees: Total fees taken in previous epoch
    ///   spo_block_counts: Count of blocks minted by operator ID in previous epoch
    // Follows the general scheme in https://docs.cardano.org/about-cardano/learn/pledging-rewards
    fn enter_epoch(&mut self, epoch: u64, total_fees: u64,
                   spo_block_counts: HashMap<KeyHash, usize>) -> Result<()> {

        // TODO HACK! Investigate why this differs to our calculated reserves after AVVM
        // 13,887,515,255 - epoch 207 is as we enter 208 (Shelley)
        if epoch == 207 {
            // Fix reserves to that given in the CF Java implementation:
            // https://github.com/cardano-foundation/cf-java-rewards-calculation/blob/b05eddf495af6dc12d96c49718f27c34fa2042b1/calculation/src/main/java/org/cardanofoundation/rewards/calculation/config/NetworkConfig.java#L45C57-L45C74
            self.pots.reserves = 13_888_022_852_926_644;
            warn!("Fixed reserves to {}", self.pots.reserves);
        }

        // Filter the block counts for SPOs that are registered - treating any we don't know
        // as 'OBFT' style (the legacy nodes)
        let known_vrf_keys: HashSet<_> = self.spos.values().map(|spo| &spo.vrf_key_hash).collect();
        let obft_block_count: usize = spo_block_counts
            .iter()
            .filter(|(vrf_key, _)| !known_vrf_keys.contains(vrf_key))
            .map(|(_, count)| count)
            .sum();

        // Capture a new snapshot and push it to state
        let snapshot = Snapshot::new(epoch, &self.stake_addresses.lock().unwrap(),
                                     &self.spos, &spo_block_counts, obft_block_count,
                                     &self.pots, total_fees);
        self.rewards_state.push(snapshot);

        // Get Shelley parameters, silently return if too early in the chain so no
        // rewards to calculate
        let shelley_params = match &self.protocol_parameters {
            Some(ProtocolParams { shelley: Some(sp), .. }) => sp,
            _ => {
                return Ok(())
            }
        };

        // Calculate reward payouts and reserves/treasury changes
        let reward_result = self.rewards_state.calculate_rewards(epoch, &shelley_params)?;

        // Pay the rewards
        for (account, amount) in reward_result.rewards {
            self.add_to_reward(&account, amount);
        }

        // Adjust the pots
        Self::update_value_with_delta(&mut self.pots.reserves, reward_result.reserves_delta)?;
        Self::update_value_with_delta(&mut self.pots.treasury, reward_result.treasury_delta)?;

        // Pay the refunds and MIRs ready for next time
        self.pay_pool_refunds();
        self.pay_stake_refunds();
        self.pay_mirs();

        Ok(())
    }

    /// Pay pool refunds
    fn pay_pool_refunds(&mut self) {
        // Get pool deposit amount from parameters, or default
        let deposit = self.protocol_parameters
            .as_ref()
            .and_then(|pp| pp.shelley.as_ref())
            .map(|sp| sp.protocol_params.pool_deposit)
            .unwrap_or(DEFAULT_POOL_DEPOSIT);

        let refunds = take(&mut self.pool_refunds);
        if !refunds.is_empty() {
            info!("{} retiring SPOs, total refunds {}", refunds.len(),
                  (refunds.len() as u64) * deposit);
        }

        // TODO - if their reward account has been deregistered, it goes to Treasury

        // Send them their deposits back
        for keyhash in refunds {
            self.add_to_reward(&keyhash, deposit);
            self.pots.deposits -= deposit;
        }
    }

    /// Pay stake address refunds
    fn pay_stake_refunds(&mut self) {
        let refunds = take(&mut self.stake_refunds);
        if !refunds.is_empty() {
            info!("{} deregistered stake addresses, total refunds {}", refunds.len(),
                  refunds.iter().map(|(_, n)| n).sum::<Lovelace>());
        }

        // Send them their deposits back
        for (keyhash, deposit) in refunds {
            self.add_to_reward(&keyhash, deposit);
            self.pots.deposits -= deposit;
        }
    }

    /// Pay MIRs
    fn pay_mirs(&mut self) {
        let mirs = take(&mut self.mirs);
        for mir in mirs {
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
                    let mut total_value: u64 = 0;
                    for (credential, value) in deltas.iter() {
                        let hash = credential.get_hash();

                        // Get old stake address state, or create one
                        let mut stake_addresses = self.stake_addresses.lock().unwrap();
                        let sas = stake_addresses.entry(hash.clone()).or_default();

                        // Add to this one
                        if let Err(e) = Self::update_value_with_delta(&mut sas.rewards, *value) {
                            error!("MIR to stake hash {}: {e}", hex::encode(hash));
                        }

                        // Update the source
                        if let Err(e) = Self::update_value_with_delta(source, -*value) {
                            error!("MIR from {source_name}: {e}");
                        }

                        let _ = Self::update_value_with_delta(&mut total_value, *value);
                    }

                    info!("MIR of {total_value} to {} stake addresses from {source_name}",
                          deltas.len());
                }

                InstantaneousRewardTarget::OtherAccountingPot(value) => {
                    // Transfer between pots
                    if let Err(e) = Self::update_value_with_delta(source, -(*value as i64)) {
                        error!("MIR from {source_name}: {e}");
                    }
                    if let Err(e) = Self::update_value_with_delta(other, *value as i64) {
                        error!("MIR to {other_name}: {e}");
                    }

                    info!("MIR of {value} from {source_name} to {other_name}");
                }
            }
        }
    }

    /// Add a reward to a reward account (by hash)
    fn add_to_reward(&mut self, account: &KeyHash, amount: Lovelace) {
        // Get old stake address state, or create one
        let mut stake_addresses = self.stake_addresses.lock().unwrap();

        // Get or create account entry, avoiding clone when existing
        let sas = match stake_addresses.get_mut(account) {
            Some(existing) => existing,
            None => {
                stake_addresses.insert(account.clone(), StakeAddressState::default());
                stake_addresses.get_mut(account).unwrap()
            }
        };

        if let Err(e) = Self::update_value_with_delta(&mut sas.rewards, amount as i64) {
            error!("Adding to reward account {}: {e}", hex::encode(account));
        }
    }

    /// Derive the Stake Pool Delegation Distribution (SPDD) - a map of total stake values
    /// (both with and without rewards) for each active SPO
    /// Key of returned map is the SPO 'operator' ID
    pub fn generate_spdd(&self) -> BTreeMap<KeyHash, DelegatedStake> {
        // Shareable Dashmap with referenced keys
        let spo_stakes = Arc::new(DashMap::<KeyHash, DelegatedStake>::new());

        // Total stake across all addresses in parallel, first collecting into a vector
        // because imbl::OrdMap doesn't work in Rayon
        let stake_addresses = self.stake_addresses.lock().unwrap();

        // Collect the SPO keys and UTXO, reward values
        let sas_data: Vec<(KeyHash, (u64, u64))> = stake_addresses
            .values()
            .filter_map(|sas| {
                sas.delegated_spo.as_ref()
                    .map(|spo| (spo.clone(), (sas.utxo_value, sas.rewards)))
            })
            .collect();

        // Parallel sum all the stakes into the spo_stake map
        sas_data
            .par_iter() // Rayon multi-threaded iterator
            .for_each_init(
                || Arc::clone(&spo_stakes),
                |map, (spo, (utxo_value, rewards))| {
                    map.entry(spo.clone()).and_modify(|v| {
                        v.active += *utxo_value;
                        v.live += *utxo_value + *rewards;
                    }).or_insert(DelegatedStake {
                        active: *utxo_value,
                        live: *utxo_value + *rewards
                    });
                },
            );

        // Collect into a plain BTreeMap, so that it is ordered on output
        spo_stakes.iter().map(|entry| (entry.key().clone(), entry.value().clone())).collect()
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
        self.stake_addresses
            .lock()
            .unwrap()
            .values()
            .collect::<Vec<_>>()
            .par_iter()
            .for_each(|state| {
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

        let different = match &self.protocol_parameters {
            Some(old_params) => old_params != &params_msg.params,
            None => true
        };

        if different {
            info!("New parameter set: {:?}", params_msg.params);
        }

        self.protocol_parameters = Some(params_msg.params.clone());
        Ok(())
    }

    /// Handle an EpochActivityMessage giving total fees and block counts by VRF key for
    /// the just-ended epoch
    pub fn handle_epoch_activity(&mut self, ea_msg: &EpochActivityMessage) -> Result<()> {

        // Reverse map of VRF key to SPO operator ID
        let vrf_to_operator: HashMap<KeyHash, KeyHash> = self.spos
            .iter()
            .map(|(id, spo)| (spo.vrf_key_hash.clone(), id.clone()))
            .collect();

        // Create a map of operator ID to block count
        let spo_block_counts: HashMap<KeyHash, usize> =
            ea_msg.vrf_vkey_hashes
            .iter()
            .filter_map(|(vrf, count)| {
                vrf_to_operator.get(vrf).map(|operator| (operator.clone(), *count))
                    .or_else(|| {
                        warn!("Unknown VRF key {}", hex::encode(vrf));
                        None
                    })
            })
            .collect();

        // Enter epoch - note the message specifies the epoch that has just *ended*
        self.enter_epoch(ea_msg.epoch+1, ea_msg.total_fees, spo_block_counts)
    }

    /// Handle an SPOStateMessage with the full set of SPOs valid at the end of the last
    /// epoch
    pub fn handle_spo_state(&mut self, spo_msg: &SPOStateMessage) -> Result<()> {

        // Capture current SPOs, mapped by operator ID
        let mut new_spos: OrdMap<KeyHash, PoolRegistration> = spo_msg
            .spos
            .iter()
            .cloned()
            .map(|spo| (spo.operator.clone(), spo))
            .collect();

        // Get pool deposit amount from parameters, or default
        let deposit = self.protocol_parameters
            .as_ref()
            .and_then(|pp| pp.shelley.as_ref())
            .map(|sp| sp.protocol_params.pool_deposit)
            .unwrap_or(DEFAULT_POOL_DEPOSIT);

        // Check for how many new SPOs
        let new_count = new_spos
            .keys()
            .filter(|id| !self.spos.contains_key(*id))
            .count();

        // They've each paid their deposit, so increment that (the UTXO spend is taken
        // care of in UTXOState)
        let total_deposits = (new_count as u64) * deposit;
        self.pots.deposits += total_deposits;

        if new_count > 0 {
            info!("{new_count} new SPOs, total new deposits {total_deposits}");
        }

        // Check for any SPOs that have retired this epoch and need deposit refunds
        self.pool_refunds = Vec::new();
        for id in &spo_msg.retired_spos {
            if let Some(retired_spo) = new_spos.get(id) {
                match StakeAddress::from_binary(&retired_spo.reward_account) {
                    Ok(stake_address) => {
                        let keyhash = stake_address.get_hash();
                        debug!("SPO {} has retired - refunding their deposit to {}",
                              hex::encode(id), hex::encode(keyhash));
                        self.pool_refunds.push(keyhash.to_vec());
                    }
                    Err(e) => error!("Error repaying SPO deposit: {e}")
                }

                // Remove from our list
                new_spos.remove(id);
                // TODO - wipe any delegations to retired pools
            }
        }

        self.spos = new_spos;
        Ok(())
    }

    /// Register a stake address, with specified deposit if known
    fn register_stake_address(&mut self, credential: &StakeCredential,
                              deposit: Option<Lovelace>) {
        let hash = credential.get_hash();

        // Stake addresses can be registered after being used in UTXOs
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        let sas = stake_addresses.entry(hash.clone()).or_default();
        if sas.registered {
            error!("Stake address hash {} registered when already registered", hex::encode(&hash));
        } else {
            sas.registered = true;

            // Account for the deposit
            let deposit = match deposit {
                Some(deposit) => deposit,
                None => {
                    // Get stake deposit amount from parameters, or default
                    self.protocol_parameters
                        .as_ref()
                        .and_then(|pp| pp.shelley.as_ref())
                        .map(|sp| sp.protocol_params.key_deposit)
                        .unwrap_or(DEFAULT_KEY_DEPOSIT)
                }
            };

            self.pots.deposits += deposit;
        }
    }

    /// Deregister a stake address, with specified refund if known
    fn deregister_stake_address(&mut self, credential: &StakeCredential,
                                refund: Option<Lovelace>) {
        let hash = credential.get_hash();

        // Check if it existed
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        if let Some(sas) = stake_addresses.get_mut(&hash) {

            if sas.registered {
                // Account for the deposit, if registered before
                let deposit = match refund {
                    Some(deposit) => deposit,
                    None => {
                        // Get stake deposit amount from parameters, or default
                        self.protocol_parameters
                            .as_ref()
                            .and_then(|pp| pp.shelley.as_ref())
                            .map(|sp| sp.protocol_params.key_deposit)
                            .unwrap_or(DEFAULT_KEY_DEPOSIT)
                    }
                };
                self.pots.deposits -= deposit;
                sas.registered = false;
            } else {
                error!("Deregistration of unregistered stake address hash {}", hex::encode(hash));
            }
        } else {
            error!("Deregistration of unknown stake address hash {}", hex::encode(hash));
        }
    }

    pub fn handle_drep_state(&mut self, drep_msg: &DRepStateMessage) {
        self.dreps = drep_msg.dreps.clone();
    }

    /// Record a stake delegation
    fn record_stake_delegation(&mut self, credential: &StakeCredential, spo: &KeyHash) {
        let hash = credential.get_hash();

        // Get old stake address state, or create one
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        if let Some(sas) = stake_addresses.get_mut(&hash) {
            if sas.registered {
                sas.delegated_spo = Some(spo.clone());
            } else {
                error!("Unregistered stake address in stake delegation: {}", hex::encode(hash));
            }
        } else {
            error!("Unknown stake address in stake delegation: {}", hex::encode(hash));
        }
    }

    /// Handle an MoveInstantaneousReward (pre-Conway only)
    pub fn handle_mir(&mut self, mir: &MoveInstantaneousReward) -> Result<()> {
        self.mirs.push(mir.clone());
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
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        if let Some(sas) = stake_addresses.get_mut(&hash) {
            if sas.registered {
                sas.delegated_drep = Some(drep.clone());
            } else {
                error!("Unregistered stake address in DRep delegation: {}", hex::encode(hash));
            }
        } else {
            error!("Unknown stake address in stake delegation: {}", hex::encode(hash));
        }
    }

    /// Handle TxCertificates
    pub fn handle_tx_certificates(&mut self, tx_certs_msg: &TxCertificatesMessage) -> Result<()> {
        // Handle certificates
        for tx_cert in tx_certs_msg.certificates.iter() {
            match tx_cert {
                TxCertificate::StakeRegistration(sc_with_pos) => {
                    self.register_stake_address(&sc_with_pos.stake_credential, None);
                }

                TxCertificate::StakeDeregistration(sc) => {
                    self.deregister_stake_address(&sc, None);
                }

                TxCertificate::MoveInstantaneousReward(mir) => {
                    self.handle_mir(&mir).unwrap_or_else(|e| error!("MIR failed: {e:#}"));
                }

                TxCertificate::Registration(reg) => {
                    self.register_stake_address(&reg.credential, Some(reg.deposit));
                }

                TxCertificate::Deregistration(dreg) => {
                    self.deregister_stake_address(&dreg.credential, Some(dreg.refund));
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
                    self.register_stake_address(&delegation.credential, Some(delegation.deposit));
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                }

                TxCertificate::StakeRegistrationAndVoteDelegation(delegation) => {
                    self.register_stake_address(&delegation.credential, Some(delegation.deposit));
                    self.record_drep_delegation(&delegation.credential, &delegation.drep);
                }

                TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(delegation) => {
                    self.register_stake_address(&delegation.credential, Some(delegation.deposit));
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
            let mut stake_addresses = self.stake_addresses.lock().unwrap();
            if let Some(sas) = stake_addresses.get(hash) {

                // Zero withdrawals are expected, as a way to validate stake addresses (per Pi)
                if withdrawal.value != 0 {
                    let mut sas = sas.clone();
                    if let Err(e) = Self::update_value_with_delta(&mut sas.rewards,
                                                                  -(withdrawal.value as i64)) {
                        error!("Withdrawing from stake address {} hash {}: {e}",
                               withdrawal.address.to_string().unwrap_or("???".to_string()),
                               hex::encode(hash));
                        continue;
                    } else {
                        // Update the stake address
                        stake_addresses.insert(hash.to_vec(), sas);
                    }
                }
            } else {
                error!("Unknown stake address in withdrawal: {}",
                       withdrawal.address.to_string().unwrap_or("???".to_string()));
            }
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

            if let Err(e) = Self::update_value_with_delta(pot, pot_delta.delta) {
                error!("Applying pot delta {pot_delta:?}: {e}");
            } else {
                info!("Pot delta for {:?} {} => {}", pot_delta.pot, pot_delta.delta, *pot);
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
            let hash = delta.address.get_hash();

            // Stake addresses don't need to be registered if they aren't used for
            // stake or drep delegation, but we need to track them in case they are later
            let mut stake_addresses = self.stake_addresses.lock().unwrap();
            let sas = stake_addresses.entry(hash.to_vec()).or_default();

            if let Err(e) = Self::update_value_with_delta(&mut sas.utxo_value, delta.delta) {
                error!("Applying delta to stake hash {}: {e}", hex::encode(hash));
            }
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
        ProtocolParams, Ratio, Registration, StakeAddress, StakeAddressDelta, StakeAddressPayload,
        StakeAndVoteDelegation, StakeRegistrationAndStakeAndVoteDelegation,
        StakeRegistrationAndVoteDelegation, VoteDelegation, Withdrawal,
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

        // Register first
        state.register_stake_address(&StakeCredential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), None);

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(stake_addresses.len(), 1);
        }

        // Pass in deltas
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&STAKE_KEY_HASH),
                delta: 42,
            }],
        };

        state.handle_stake_deltas(&msg).unwrap();

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(
                stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().utxo_value,
                42
            );
        }

        state.handle_stake_deltas(&msg).unwrap();

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(
                stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap().utxo_value,
                84
            );
        }
    }

    #[test]
    fn spdd_is_empty_at_start() {
        let state = State::default();
        let spdd = state.generate_spdd();
        assert!(spdd.is_empty());
    }

    // TODO! Misnomer - pledge is not specifically included in spdd because it is handled
    // by the owner's own staking.  What does need to be tested is the difference between
    // 'active' and 'live' by adding rewards
    #[test]
    fn spdd_from_delegation_with_utxo_values_and_pledge() {
        let mut state = State::default();

        let spo1: KeyHash = vec![0x01];
        let spo2: KeyHash = vec![0x02];

        // Create the SPOs
        state.handle_spo_state(&SPOStateMessage {
            epoch: 1,
            spos: vec![
                PoolRegistration {
                    operator: spo1.clone(),
                    vrf_key_hash: spo1.clone(),
                    pledge: 26,
                    cost: 0,
                    margin: Ratio { numerator: 1, denominator: 20 },
                    reward_account: Vec::new(),
                    pool_owners: Vec::new(),
                    relays: Vec::new(),
                    pool_metadata: None
                },
                PoolRegistration {
                    operator: spo2.clone(),
                    vrf_key_hash: spo2.clone(),
                    pledge: 47,
                    cost: 10,
                    margin: Ratio { numerator: 1, denominator: 10 },
                    reward_account: Vec::new(),
                    pool_owners: Vec::new(),
                    relays: Vec::new(),
                    pool_metadata: None
                },
            ],
            retired_spos: vec![],
        }).unwrap();

        // Delegate
        let addr1: KeyHash = vec![0x11];
        let cred1 = Credential::AddrKeyHash(addr1.clone());
        state.register_stake_address(&cred1, None);
        state.record_stake_delegation(&cred1, &spo1);

        let addr2: KeyHash = vec![0x12];
        let cred2 = Credential::AddrKeyHash(addr2.clone());
        state.register_stake_address(&cred2, None);
        state.record_stake_delegation(&cred2, &spo2);

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
        assert_eq!(stake1.active, 42);
        let stake2 = spdd.get(&spo2).unwrap();
        assert_eq!(stake2.active, 21);
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
        let pot_deltas = PotDeltasMessage {
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

        state.handle_pot_deltas(&pot_deltas).unwrap();
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
        state.pay_mirs();
        assert_eq!(state.pots.reserves, 58);
        assert_eq!(state.pots.treasury, 42);
        assert_eq!(state.pots.deposits, 0);

        // Send some of it back
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Treasury,
            target: InstantaneousRewardTarget::OtherAccountingPot(10),
        };

        state.handle_mir(&mir).unwrap();
        state.pay_mirs();
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
        state.register_stake_address(&StakeCredential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), None);
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&STAKE_KEY_HASH),
                delta: 99,
            }],
        };

        state.handle_stake_deltas(&msg).unwrap();

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(stake_addresses.len(), 1);
            let sas = stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
            assert_eq!(sas.utxo_value, 99);
            assert_eq!(sas.rewards, 0);
        }

        // Send in a MIR reserves->{47,-5}->stake
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Reserves,
            target: InstantaneousRewardTarget::StakeCredentials(vec![
                (Credential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), 47),
                (Credential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), -5),
            ]),
        };

        state.handle_mir(&mir).unwrap();
        state.pay_mirs();
        assert_eq!(state.pots.reserves, 58);
        assert_eq!(state.pots.treasury, 0);
        assert_eq!(state.pots.deposits, 2_000_000);  // Paid deposit

        let stake_addresses = state.stake_addresses.lock().unwrap();
        let sas = stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
        assert_eq!(sas.utxo_value, 99);
        assert_eq!(sas.rewards, 42);
    }

    #[test]
    fn withdrawal_transfers_from_stake_addresses() {
        let mut state = State::default();

        // Bootstrap with some in reserves
        state.pots.reserves = 100;

        // Set up one stake address
        state.register_stake_address(&StakeCredential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), None);
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: create_address(&STAKE_KEY_HASH),
                delta: 99,
            }],
        };

        state.handle_stake_deltas(&msg).unwrap();

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(stake_addresses.len(), 1);

            let sas = stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
            assert_eq!(sas.utxo_value, 99);
            assert_eq!(sas.rewards, 0);
        }

        // Send in a MIR reserves->42->stake
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Reserves,
            target: InstantaneousRewardTarget::StakeCredentials(vec![(
                Credential::AddrKeyHash(STAKE_KEY_HASH.to_vec()),
                42,
            )]),
        };

        state.handle_mir(&mir).unwrap();
        state.pay_mirs();
        assert_eq!(state.pots.reserves, 58);

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            let sas = stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
            assert_eq!(sas.rewards, 42);
        }

        // Withdraw most of it
        let withdrawals = WithdrawalsMessage {
            withdrawals: vec![Withdrawal {
                address: create_address(&STAKE_KEY_HASH),
                value: 39,
            }],
        };

        state.handle_withdrawals(&withdrawals).unwrap();

        let stake_addresses = state.stake_addresses.lock().unwrap();
        let sas = stake_addresses.get(&STAKE_KEY_HASH.to_vec()).unwrap();
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
                    motion_no_confidence: RationalNumber::ONE,
                    committee_normal: RationalNumber::ZERO,
                    committee_no_confidence: RationalNumber::ZERO,
                    hard_fork_initiation: RationalNumber::ONE,
                    security_voting_threshold: RationalNumber::ZERO,
                },
                d_rep_voting_thresholds: DRepVotingThresholds {
                    motion_no_confidence: RationalNumber::ONE,
                    committee_normal: RationalNumber::ZERO,
                    committee_no_confidence: RationalNumber::ZERO,
                    update_constitution: RationalNumber::ONE,
                    hard_fork_initiation: RationalNumber::ZERO,
                    pp_network_group: RationalNumber::ZERO,
                    pp_economic_group: RationalNumber::ZERO,
                    pp_technical_group: RationalNumber::ZERO,
                    pp_governance_group: RationalNumber::ZERO,
                    treasury_withdrawal: RationalNumber::ONE,
                },
                committee_min_size: 42,
                committee_max_term_length: 3,
                gov_action_lifetime: 99,
                gov_action_deposit: 500_000_000,
                d_rep_deposit: 100_000_000,
                d_rep_activity: 27,
                min_fee_ref_script_cost_per_byte: RationalNumber::new(1, 42),
                plutus_v3_cost_model: Vec::new(),
                constitution: Constitution {
                    anchor: Anchor {
                        url: "constitution.cardano.org".to_string(),
                        data_hash: vec![0x99],
                    },
                    guardrail_script: None,
                },
                committee: Committee {
                    members: HashMap::new(),
                    threshold: RationalNumber::new(5, 32),
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
