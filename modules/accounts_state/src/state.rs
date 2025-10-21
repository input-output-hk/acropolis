//! Acropolis AccountsState: State storage
use crate::monetary::calculate_monetary_change;
use crate::rewards::{calculate_rewards, RewardsResult};
use crate::snapshot::Snapshot;
use crate::verifier::Verifier;
use acropolis_common::queries::accounts::OptimalPoolSizing;
use acropolis_common::{
    math::update_value_with_delta,
    messages::{
        DRepDelegationDistribution, DRepStateMessage, EpochActivityMessage, PotDeltasMessage,
        ProtocolParamsMessage, SPOStateMessage, StakeAddressDeltasMessage, TxCertificatesMessage,
        WithdrawalsMessage,
    },
    protocol_params::ProtocolParams,
    stake_addresses::{StakeAddressMap, StakeAddressState},
    BlockInfo, DRepChoice, DRepCredential, DelegatedStake, InstantaneousRewardSource,
    InstantaneousRewardTarget, KeyHash, Lovelace, MoveInstantaneousReward, PoolLiveStakeInfo,
    PoolRegistration, Pot, SPORewards, StakeAddress, StakeCredential, StakeRewardDelta,
    TxCertificate,
};
use anyhow::Result;
use imbl::OrdMap;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::mem::take;
use std::sync::{mpsc, Arc, Mutex};
use tokio::task::{spawn_blocking, JoinHandle};
use tracing::{debug, error, info, warn, Level};

const DEFAULT_KEY_DEPOSIT: u64 = 2_000_000;
const DEFAULT_POOL_DEPOSIT: u64 = 500_000_000;

/// Stability window = slots into epoch at which Haskell node starts the rewards calculation
// We need this because of a Shelley-era bug where stake deregistrations were still counted
// up to the point of start of the calculation, rather than point of snapshot
const STABILITY_WINDOW_SLOT: u64 = 4 * 2160 * 20; // TODO configure from genesis?

/// Global 'pot' account state
#[derive(Debug, Default, PartialEq, Clone, serde::Serialize)]
pub struct Pots {
    /// Unallocated reserves
    pub reserves: Lovelace,

    /// Treasury
    pub treasury: Lovelace,

    /// Deposits
    pub deposits: Lovelace,
}

/// State for rewards calculation
#[derive(Debug, Default, Clone)]
pub struct EpochSnapshots {
    /// Latest snapshot (epoch i)
    pub mark: Arc<Snapshot>,

    /// Previous snapshot (epoch i-1)
    pub set: Arc<Snapshot>,

    /// One before that (epoch i-2)
    pub go: Arc<Snapshot>,
}

impl EpochSnapshots {
    /// Push a new snapshot
    pub fn push(&mut self, latest: Snapshot) {
        self.go = self.set.clone();
        self.set = self.mark.clone();
        self.mark = Arc::new(latest);
    }
}

/// Registration change kind
#[derive(Debug, Clone)]
pub enum RegistrationChangeKind {
    Registered,
    Deregistered,
}

/// Registration change on a stake address
#[derive(Debug, Clone)]
pub struct RegistrationChange {
    /// Stake address (full address, not just hash)
    pub address: StakeAddress,

    /// Change type
    pub kind: RegistrationChangeKind,
}

/// Overall state - stored per block
#[derive(Debug, Default, Clone)]
pub struct State {
    /// Map of active SPOs by operator ID
    spos: OrdMap<KeyHash, PoolRegistration>,

    /// List of SPOs (by operator ID) retiring in the current epoch
    retiring_spos: Vec<KeyHash>,

    /// Map of staking address values
    /// Wrapped in an Arc so it doesn't get cloned in full by StateHistory
    stake_addresses: Arc<Mutex<StakeAddressMap>>,

    /// Short history of snapshots
    epoch_snapshots: EpochSnapshots,

    /// Global account pots
    pots: Pots,

    /// All registered DReps
    dreps: Vec<(DRepCredential, Lovelace)>,

    /// Protocol parameters that apply during this epoch
    protocol_parameters: Option<ProtocolParams>,

    /// Protocol parameters that applied in the previous epoch
    previous_protocol_parameters: Option<ProtocolParams>,

    /// Pool refunds to apply next epoch (list of reward accounts to refund to)
    pool_refunds: Vec<StakeAddress>,

    /// Stake address refunds to apply next epoch
    stake_refunds: Vec<(StakeAddress, Lovelace)>,

    /// MIRs to pay next epoch
    mirs: Vec<MoveInstantaneousReward>,

    /// Addresses registration changes in current epoch
    current_epoch_registration_changes: Arc<Mutex<Vec<RegistrationChange>>>,

    /// Task for rewards calculation if necessary
    epoch_rewards_task: Arc<Mutex<Option<JoinHandle<Result<RewardsResult>>>>>,

    /// Signaller to start the above - delayed in early Shelley to replicate bug
    start_rewards_tx: Option<mpsc::Sender<()>>,
}

impl State {
    /// Get the stake address state for a give stake key
    pub fn get_stake_state(&self, stake_key: &StakeAddress) -> Option<StakeAddressState> {
        self.stake_addresses.lock().unwrap().get(stake_key)
    }

    /// Get the current pot balances
    pub fn _get_pots(&self) -> Pots {
        self.pots.clone()
    }

    /// Get maximum pool size
    /// ( total_supply - reserves) / nopt (from protocol parameters)
    /// Return None if it is before Shelley Era
    pub fn get_optimal_pool_sizing(&self) -> Option<OptimalPoolSizing> {
        // Get Shelley parameters, silently return if too early in the chain so no
        // rewards to calculate
        let shelley_params = match &self.protocol_parameters {
            Some(ProtocolParams {
                shelley: Some(sp), ..
            }) => sp,
            _ => return None,
        }
        .clone();

        let total_supply =
            shelley_params.max_lovelace_supply - self.epoch_snapshots.mark.pots.reserves;
        let nopt = shelley_params.protocol_params.stake_pool_target_num as u64;
        Some(OptimalPoolSizing { total_supply, nopt })
    }

    /// Get Pool Live Stake Info
    pub fn get_pool_live_stake_info(&self, pool_operator: &KeyHash) -> PoolLiveStakeInfo {
        self.stake_addresses.lock().unwrap().get_pool_live_stake_info(pool_operator)
    }

    /// Get Pools Live stake
    pub fn get_pools_live_stakes(&self, pool_operators: &Vec<KeyHash>) -> Vec<u64> {
        self.stake_addresses.lock().unwrap().get_pools_live_stakes(pool_operators)
    }

    /// Get Pool Delegators with live_stakes
    pub fn get_pool_delegators(&self, pool_operator: &KeyHash) -> Vec<(KeyHash, u64)> {
        self.stake_addresses.lock().unwrap().get_pool_delegators(pool_operator)
    }

    /// Get Drep Delegators with live_stakes
    pub fn get_drep_delegators(&self, drep: &DRepChoice) -> Vec<(KeyHash, u64)> {
        self.stake_addresses.lock().unwrap().get_drep_delegators(drep)
    }

    /// Map stake_keys to their utxo_values
    pub fn get_accounts_utxo_values_map(
        &self,
        stake_keys: &[StakeAddress],
    ) -> Option<HashMap<Vec<u8>, u64>> {
        let stake_addresses = self.stake_addresses.lock().ok()?; // If lock fails, return None
        stake_addresses.get_accounts_utxo_values_map(stake_keys)
    }

    /// Sum stake_keys utxo_values
    pub fn get_accounts_utxo_values_sum(&self, stake_keys: &[StakeAddress]) -> Option<u64> {
        let stake_addresses = self.stake_addresses.lock().ok()?; // If lock fails, return None
        stake_addresses.get_accounts_utxo_values_sum(stake_keys)
    }

    /// Map stake_keys to their total balances (utxo + rewards)
    pub fn get_accounts_balances_map(
        &self,
        stake_keys: &[StakeAddress],
    ) -> Option<HashMap<Vec<u8>, u64>> {
        let stake_addresses = self.stake_addresses.lock().ok()?; // If lock fails, return None
        stake_addresses.get_accounts_balances_map(stake_keys)
    }

    /// Sum total_active_stake for delegators of all spos in the latest snapshot
    pub fn get_latest_snapshot_account_balances(&self) -> u64 {
        let mut total_active_stake: u64 = 0;
        for spo in self.epoch_snapshots.mark.spos.iter() {
            for delegator in spo.1.delegators.iter() {
                total_active_stake += delegator.1;
            }
        }
        total_active_stake
    }

    /// Map stake_keys to their delegated DRep
    pub fn get_drep_delegations_map(
        &self,
        stake_keys: &[StakeAddress],
    ) -> Option<HashMap<KeyHash, Option<DRepChoice>>> {
        let stake_addresses = self.stake_addresses.lock().ok()?; // If lock fails, return None
        stake_addresses.get_drep_delegations_map(stake_keys)
    }

    /// Sum stake_keys balances (utxo + rewards)
    pub fn get_account_balances_sum(&self, stake_keys: &[StakeAddress]) -> Option<u64> {
        let stake_addresses = self.stake_addresses.lock().ok()?; // If lock fails, return None
        stake_addresses.get_account_balances_sum(stake_keys)
    }

    /// Log statistics
    fn log_stats(&self) {
        info!(num_stake_addresses = self.stake_addresses.lock().unwrap().len());
    }

    /// Background tick
    pub async fn tick(&self) -> Result<()> {
        self.log_stats();
        Ok(())
    }

    /// Process entry into a new epoch
    ///   epoch: Number of epoch we are entering
    ///   total_fees: Total fees taken in previous epoch
    ///   total_blocks: Total blocks minted (both SPO and OBFT)
    ///   spo_block_counts: Count of blocks minted by operator ID in previous epoch
    ///   verifier: Verifier against Haskell node output
    // Follows the general scheme in https://docs.cardano.org/about-cardano/learn/pledging-rewards
    fn enter_epoch(
        &mut self,
        epoch: u64,
        total_fees: u64,
        spo_block_counts: HashMap<KeyHash, usize>,
        verifier: &Verifier,
    ) -> Result<Vec<StakeRewardDelta>> {
        // TODO HACK! Investigate why this differs to our calculated reserves after AVVM
        // 13,887,515,255 - as we enter 208 (Shelley)
        // TODO this will only work in Mainnet - need to know when Shelley starts across networks
        // and the reserves value, if we can't properly calculate it
        if epoch == 208 {
            // Fix reserves to that given in the CF Java implementation:
            // https://github.com/cardano-foundation/cf-java-rewards-calculation/blob/b05eddf495af6dc12d96c49718f27c34fa2042b1/calculation/src/main/java/org/cardanofoundation/rewards/calculation/config/NetworkConfig.java#L45C57-L45C74
            let old_reserves = self.pots.reserves;
            self.pots.reserves = 13_888_022_852_926_644;
            warn!(
                new = self.pots.reserves,
                old = old_reserves,
                diff = self.pots.reserves - old_reserves,
                "Fixed reserves"
            );
        }

        // Get previous Shelley parameters, silently return if too early in the chain so no
        // rewards to calculate
        // In the first epoch of Shelley, there are no previous_protocol_parameters, so we
        // have to use the genesis parameters we just received
        let shelley_params = match &self.previous_protocol_parameters {
            Some(ProtocolParams {
                shelley: Some(sp), ..
            }) => sp,
            _ => match &self.protocol_parameters {
                Some(ProtocolParams {
                    shelley: Some(sp), ..
                }) => sp,
                _ => return Ok(vec![]),
            },
        }
        .clone();

        info!(
            epoch,
            reserves = self.pots.reserves,
            treasury = self.pots.treasury,
            "Entering"
        );

        // Filter the block counts for SPOs that are registered - treating any we don't know
        // as 'OBFT' style (the legacy nodes)
        let total_non_obft_blocks = spo_block_counts.values().sum();

        // Pay MIRs before snapshot, so reserves is correct for total_supply in rewards
        let mut reward_deltas = Vec::<StakeRewardDelta>::new();
        reward_deltas.extend(self.pay_mirs());

        // Capture a new snapshot for the end of the previous epoch and push it to state
        let snapshot = Snapshot::new(
            epoch - 1,
            &self.stake_addresses.lock().unwrap(),
            &self.spos,
            &spo_block_counts,
            &self.pots,
            total_non_obft_blocks,
            // Take and clear registration changes
            std::mem::take(&mut *self.current_epoch_registration_changes.lock().unwrap()),
            // Pass in two-previous epoch snapshot for capture of SPO reward accounts
            self.epoch_snapshots.set.clone(), // Will become 'go' in the next line!
        );
        self.epoch_snapshots.push(snapshot);

        // Pay the refunds after snapshot, so they don't appear in active_stake
        reward_deltas.extend(self.pay_pool_refunds());
        reward_deltas.extend(self.pay_stake_refunds());

        // Verify pots state
        verifier.verify_pots(epoch, &self.pots);

        // Update the reserves and treasury (monetary.rs)
        let monetary_change = calculate_monetary_change(
            &shelley_params,
            &self.pots,
            total_fees,
            total_non_obft_blocks,
        )?;
        self.pots = monetary_change.pots;

        info!(
            epoch,
            reserves = self.pots.reserves,
            treasury = self.pots.treasury,
            "After monetary change"
        );

        // Set up background task for rewards, capturing and emptying current deregistrations
        let performance = self.epoch_snapshots.mark.clone();
        let staking = self.epoch_snapshots.go.clone();

        // Calculate the sets of net registrations and deregistrations which happened between
        // staking and now
        // Note: We do this to save memory - although the 'mark' snapshot contains the
        // current registration status of each address, it is segmented by SPO and there's
        // no way to search by address (they may move SPO in between), so this saves another
        // huge map.  If the snapshot was ever changed to store addresses in a way where an
        // individual could be looked up, this could be simplified - but you still need to
        // handle the Shelley bug part!
        let mut registrations: HashSet<StakeAddress> = HashSet::new();
        let mut deregistrations: HashSet<StakeAddress> = HashSet::new();
        Self::apply_registration_changes(
            &self.epoch_snapshots.set.registration_changes,
            &mut registrations,
            &mut deregistrations,
        );
        Self::apply_registration_changes(
            &self.epoch_snapshots.mark.registration_changes,
            &mut registrations,
            &mut deregistrations,
        );

        let (start_rewards_tx, start_rewards_rx) = mpsc::channel::<()>();
        let current_epoch_registration_changes = self.current_epoch_registration_changes.clone();
        self.epoch_rewards_task = Arc::new(Mutex::new(Some(spawn_blocking(move || {
            // Wait for start signal
            let _ = start_rewards_rx.recv();

            // Additional deregistrations from current epoch - early Shelley bug
            // TODO - make optional, turn off after Allegra
            Self::apply_registration_changes(
                &current_epoch_registration_changes.lock().unwrap(),
                &mut registrations,
                &mut deregistrations,
            );

            if tracing::enabled!(Level::DEBUG) {
                registrations.iter().for_each(|addr| debug!("Registration {}", addr));
                deregistrations.iter().for_each(|addr| debug!("Deregistration {}", addr));
            }

            // Calculate reward payouts for previous epoch
            calculate_rewards(
                epoch - 1,
                performance,
                staking,
                &shelley_params,
                monetary_change.stake_rewards,
                &registrations,
                &deregistrations,
            )
        }))));

        // Delay starting calculation until 4k into epoch, to capture late deregistrations
        // wrongly counted in early Shelley, and also to put them out of reach of rollbacks
        self.start_rewards_tx = Some(start_rewards_tx);

        // Now retire the SPOs fully
        // TODO - wipe any delegations to retired pools
        for id in self.retiring_spos.drain(..) {
            self.spos.remove(&id);
        }

        Ok(reward_deltas)
    }

    /// Apply a registration change set to registration/deregistration lists
    /// registrations gets all registrations still in effect at the end of the changes
    /// deregistrations likewise for net deregistrations
    fn apply_registration_changes(
        changes: &Vec<RegistrationChange>,
        registrations: &mut HashSet<StakeAddress>,
        deregistrations: &mut HashSet<StakeAddress>,
    ) {
        for change in changes {
            match change.kind {
                RegistrationChangeKind::Registered => {
                    registrations.insert(change.address.clone());
                    deregistrations.remove(&change.address);
                }
                RegistrationChangeKind::Deregistered => {
                    registrations.remove(&change.address);
                    deregistrations.insert(change.address.clone());
                }
            };
        }
    }

    /// Notify of a new block
    pub fn notify_block(&mut self, block: &BlockInfo) {
        // Is the rewards task blocked on us reaching the 4 * k block?
        if let Some(tx) = &self.start_rewards_tx {
            if block.epoch_slot >= STABILITY_WINDOW_SLOT {
                info!(
                    "Starting rewards calculation at block {}, epoch slot {}",
                    block.number, block.epoch_slot
                );
                let _ = tx.send(());
                self.start_rewards_tx = None;
            }
        }
    }

    /// Pay pool refunds
    fn pay_pool_refunds(&mut self) -> Vec<StakeRewardDelta> {
        let mut reward_deltas = Vec::<StakeRewardDelta>::new();

        // Get pool deposit amount from parameters, or default
        let deposit = self
            .protocol_parameters
            .as_ref()
            .and_then(|pp| pp.shelley.as_ref())
            .map(|sp| sp.protocol_params.pool_deposit)
            .unwrap_or(DEFAULT_POOL_DEPOSIT);

        let refunds = take(&mut self.pool_refunds);
        if !refunds.is_empty() {
            info!(
                "{} retiring SPOs, total refunds {}",
                refunds.len(),
                (refunds.len() as u64) * deposit
            );
        }

        // Send them their deposits back
        for stake_address in refunds {
            // If their reward account has been deregistered, it goes to Treasury
            let mut stake_addresses = self.stake_addresses.lock().unwrap();
            if stake_addresses.is_registered(&stake_address) {
                reward_deltas.push(StakeRewardDelta {
                    stake_address: stake_address.clone(),
                    delta: deposit as i64,
                });
                stake_addresses.add_to_reward(&stake_address, deposit);
            } else {
                warn!(
                    "SPO reward account {} deregistered - paying refund to treasury",
                    stake_address
                );
                self.pots.treasury += deposit;
            }

            self.pots.deposits -= deposit;
        }

        reward_deltas
    }

    /// Pay stake address refunds
    fn pay_stake_refunds(&mut self) -> Vec<StakeRewardDelta> {
        let mut reward_deltas = Vec::<StakeRewardDelta>::new();

        let refunds = take(&mut self.stake_refunds);
        if !refunds.is_empty() {
            info!(
                "{} deregistered stake addresses, total refunds {}",
                refunds.len(),
                refunds.iter().map(|(_, n)| n).sum::<Lovelace>()
            );
        }

        // Send them their deposits back
        for (stake_address, deposit) in refunds {
            let mut stake_addresses = self.stake_addresses.lock().unwrap();
            reward_deltas.push(StakeRewardDelta {
                stake_address: stake_address.clone(), // Extract hash for delta
                delta: deposit as i64,
            });
            stake_addresses.add_to_reward(&stake_address, deposit);
            self.pots.deposits -= deposit;
        }

        reward_deltas
    }

    /// Pay MIRs
    fn pay_mirs(&mut self) -> Vec<StakeRewardDelta> {
        let mut reward_deltas = Vec::<StakeRewardDelta>::new();

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
                        let stake_address = credential.to_stake_address(None); // Need to convert credential to address

                        // Get old stake address state, or create one
                        let mut stake_addresses = self.stake_addresses.lock().unwrap();
                        let sas = stake_addresses.entry(stake_address.clone()).or_default();

                        // Add to this one
                        reward_deltas.push(StakeRewardDelta {
                            stake_address: stake_address.clone(),
                            delta: *value,
                        });
                        if let Err(e) = update_value_with_delta(&mut sas.rewards, *value) {
                            error!("MIR to stake address {}: {e}", stake_address);
                        }

                        // Update the source
                        if let Err(e) = update_value_with_delta(source, -*value) {
                            error!("MIR from {source_name}: {e}");
                        }

                        let _ = update_value_with_delta(&mut total_value, *value);
                    }

                    info!(
                        "MIR of {total_value} to {} stake addresses from {source_name}",
                        deltas.len()
                    );
                }

                InstantaneousRewardTarget::OtherAccountingPot(value) => {
                    // Transfer between pots
                    if let Err(e) = update_value_with_delta(source, -(*value as i64)) {
                        error!("MIR from {source_name}: {e}");
                    }
                    if let Err(e) = update_value_with_delta(other, *value as i64) {
                        error!("MIR to {other_name}: {e}");
                    }

                    info!("MIR of {value} from {source_name} to {other_name}");
                }
            }
        }

        reward_deltas
    }

    /// Derive the Stake Pool Delegation Distribution (SPDD) - a map of total stake values
    /// (both with and without rewards) for each active SPO
    /// And Stake Pool Reward State (rewards and delegators_count for each pool)
    /// Key of returned map is the SPO 'operator' ID
    pub fn generate_spdd(&self) -> BTreeMap<KeyHash, DelegatedStake> {
        let stake_addresses = self.stake_addresses.lock().unwrap();
        stake_addresses.generate_spdd()
    }

    /// Derive the DRep Delegation Distribution (DRDD) - the total amount
    /// delegated to each DRep, including the special "abstain" and "no confidence" dreps.
    pub fn generate_drdd(&self) -> DRepDelegationDistribution {
        let stake_addresses = self.stake_addresses.lock().unwrap();
        stake_addresses.generate_drdd(&self.dreps)
    }

    /// Handle an ProtocolParamsMessage with the latest parameters at the start of a new
    /// epoch
    pub fn handle_parameters(&mut self, params_msg: &ProtocolParamsMessage) -> Result<()> {
        let different = match &self.protocol_parameters {
            Some(old_params) => old_params != &params_msg.params,
            None => true,
        };

        if different {
            info!("New parameter set: {:?}", params_msg.params);
            self.previous_protocol_parameters = self.protocol_parameters.clone();
            self.protocol_parameters = Some(params_msg.params.clone());
        }

        Ok(())
    }

    /// Handle an EpochActivityMessage giving total fees and block counts by SPO for
    /// the just-ended epoch
    /// This also returns SPO rewards for publishing to the SPDD topic (For epoch N)
    /// and stake reward deltas for publishing to the StakeRewardDeltas topic (For epoch N)
    pub async fn handle_epoch_activity(
        &mut self,
        ea_msg: &EpochActivityMessage,
        verifier: &Verifier,
    ) -> Result<(Vec<(KeyHash, SPORewards)>, Vec<StakeRewardDelta>)> {
        let mut spo_rewards: Vec<(KeyHash, SPORewards)> = Vec::new();
        // Collect stake addresses reward deltas
        let mut reward_deltas = Vec::<StakeRewardDelta>::new();

        // Check previous epoch work is done
        let mut task = {
            match self.epoch_rewards_task.lock() {
                Ok(mut task) => task.take(),
                Err(_) => {
                    error!("Failed to lock epoch rewards task");
                    None
                }
            }
        };

        // If rewards have been calculated, save the results
        if let Some(task) = task.take() {
            match task.await {
                Ok(Ok(reward_result)) => {
                    // Collect rewards to stake addresses reward deltas
                    for (_, rewards) in &reward_result.rewards {
                        reward_deltas.extend(
                            rewards
                                .iter()
                                .map(|reward| StakeRewardDelta {
                                    stake_address: reward.account.clone(),
                                    delta: reward.amount as i64,
                                })
                                .collect::<Vec<_>>(),
                        );
                    }

                    // Verify them
                    verifier.verify_rewards(reward_result.epoch, &reward_result);

                    // Pay the rewards
                    let mut stake_addresses = self.stake_addresses.lock().unwrap();
                    for (_, rewards) in reward_result.rewards {
                        for reward in rewards {
                            stake_addresses.add_to_reward(&reward.account, reward.amount);
                        }
                    }

                    // save SPO rewards
                    spo_rewards = reward_result.spo_rewards.into_iter().collect();

                    // Adjust the reserves for next time with amount actually paid
                    self.pots.reserves -= reward_result.total_paid;
                }
                _ => (),
            }
        };

        // Map block counts, filtering out SPOs we don't know (OBFT in early Shelley)
        let spo_blocks: HashMap<KeyHash, usize> = ea_msg
            .spo_blocks
            .iter()
            .filter(|(hash, _)| self.spos.contains_key(hash))
            .map(|(hash, count)| (hash.clone(), *count))
            .collect();

        // Enter epoch - note the message specifies the epoch that has just *ended*
        reward_deltas.extend(self.enter_epoch(
            ea_msg.epoch + 1,
            ea_msg.total_fees,
            spo_blocks,
            verifier,
        )?);

        Ok((spo_rewards, reward_deltas))
    }

    /// Handle an SPOStateMessage with the full set of SPOs valid at the end of the last
    /// epoch
    pub fn handle_spo_state(&mut self, spo_msg: &SPOStateMessage) -> Result<()> {
        // Capture current SPOs, mapped by operator ID
        let new_spos: OrdMap<KeyHash, PoolRegistration> =
            spo_msg.spos.iter().cloned().map(|spo| (spo.operator.clone(), spo)).collect();

        // Get pool deposit amount from parameters, or default
        let deposit = self
            .protocol_parameters
            .as_ref()
            .and_then(|pp| pp.shelley.as_ref())
            .map(|sp| sp.protocol_params.pool_deposit)
            .unwrap_or(DEFAULT_POOL_DEPOSIT);

        // Check for how many new SPOs
        let new_count = new_spos.keys().filter(|id| !self.spos.contains_key(*id)).count();

        // Log new ones and pledge/cost/margin changes
        for (id, spo) in new_spos.iter() {
            match self.spos.get(id) {
                Some(old_spo) => {
                    if spo.pledge != old_spo.pledge
                        || spo.cost != old_spo.cost
                        || spo.margin != old_spo.margin
                    {
                        debug!(
                            epoch = spo_msg.epoch,
                            pledge = spo.pledge,
                            cost = spo.cost,
                            margin = ?spo.margin,
                            "Updated parameters for SPO {}",
                            hex::encode(id)
                        );
                    }
                }

                _ => {
                    debug!(
                        epoch = spo_msg.epoch,
                        pledge = spo.pledge,
                        cost = spo.cost,
                        margin = ?spo.margin,
                        "Registered new SPO {}",
                        hex::encode(id)
                    );
                }
            }
        }

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
                debug!(
                    "SPO {} has retired - refunding their deposit to {}",
                    hex::encode(id),
                    retired_spo.reward_account
                );
                self.pool_refunds.push(retired_spo.reward_account.clone()); // Store full StakeAddress
            }

            // Schedule to retire - we need them to still be in place when we count
            // blocks for the previous epoch
            self.retiring_spos.push(id.to_vec());
        }

        self.spos = new_spos;
        Ok(())
    }

    /// Register a stake address, with a specified deposit if known
    fn register_stake_address(&mut self, credential: &StakeCredential, deposit: Option<Lovelace>) {
        // TODO: Handle network
        let stake_address = credential.to_stake_address(None);

        // Stake addresses can be registered after being used in UTXOs
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        if stake_addresses.register_stake_address(&stake_address) {
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

        // Add to registration changes
        self.current_epoch_registration_changes.lock().unwrap().push(RegistrationChange {
            address: stake_address,
            kind: RegistrationChangeKind::Registered,
        });
    }

    /// Deregister a stake address, with specified refund if known
    fn deregister_stake_address(&mut self, credential: &StakeCredential, refund: Option<Lovelace>) {
        // TODO: Handle network
        let stake_address = credential.to_stake_address(None);

        // Check if it existed
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        if stake_addresses.deregister_stake_address(&stake_address) {
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

            // Schedule refund
            self.stake_refunds.push((stake_address.clone(), deposit));

            // Add to registration changes
            self.current_epoch_registration_changes.lock().unwrap().push(RegistrationChange {
                address: stake_address,
                kind: RegistrationChangeKind::Deregistered,
            });
        }
    }

    pub fn handle_drep_state(&mut self, drep_msg: &DRepStateMessage) {
        self.dreps = drep_msg.dreps.clone();
    }

    /// Record a stake delegation
    fn record_stake_delegation(&mut self, credential: &StakeCredential, spo: &KeyHash) {
        // TODO: Handle network
        let stake_address = credential.to_stake_address(None);
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        stake_addresses.record_stake_delegation(&stake_address, spo);
    }

    /// Handle an MoveInstantaneousReward (pre-Conway only)
    pub fn handle_mir(&mut self, mir: &MoveInstantaneousReward) -> Result<()> {
        self.mirs.push(mir.clone());
        Ok(())
    }

    /// record a drep delegation
    fn record_drep_delegation(&mut self, credential: &StakeCredential, drep: &DRepChoice) {
        // TODO: Handle network
        let stake_address = credential.to_stake_address(None);
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        stake_addresses.record_drep_delegation(&stake_address, drep);
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
            };
        }

        Ok(())
    }

    /// Handle withdrawals
    pub fn handle_withdrawals(&mut self, withdrawals_msg: &WithdrawalsMessage) -> Result<()> {
        for withdrawal in withdrawals_msg.withdrawals.iter() {
            let mut stake_addresses = self.stake_addresses.lock().unwrap();
            stake_addresses.process_withdrawal(withdrawal);
        }

        Ok(())
    }

    /// Handle stake deltas
    pub fn handle_stake_deltas(&mut self, deltas_msg: &StakeAddressDeltasMessage) -> Result<()> {
        // Handle deltas
        for delta in deltas_msg.deltas.iter() {
            let mut stake_addresses = self.stake_addresses.lock().unwrap();
            stake_addresses.process_stake_delta(delta);
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

            if let Err(e) = update_value_with_delta(pot, pot_delta.delta) {
                error!("Applying pot delta {pot_delta:?}: {e}");
            } else {
                info!(
                    "Pot delta for {:?} {} => {}",
                    pot_delta.pot, pot_delta.delta, *pot
                );
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
        protocol_params::ConwayParams, rational_number::RationalNumber, AddressNetwork, Anchor,
        Committee, Constitution, CostModel, Credential, DRepVotingThresholds, PoolVotingThresholds,
        Pot, PotDelta, Ratio, Registration, StakeAddress, StakeAddressDelta, StakeAddressPayload,
        StakeAndVoteDelegation, StakeRegistrationAndStakeAndVoteDelegation,
        StakeRegistrationAndVoteDelegation, VoteDelegation, Withdrawal,
    };

    // Helper to create a StakeAddress from a byte slice
    fn create_address(hash: &[u8]) -> StakeAddress {
        let mut full_hash = vec![0u8; 28];
        full_hash[..hash.len().min(28)].copy_from_slice(&hash[..hash.len().min(28)]);
        StakeAddress {
            network: AddressNetwork::Main,
            payload: StakeAddressPayload::StakeKeyHash(full_hash),
        }
    }

    fn create_stake_credential(hash: &[u8]) -> StakeCredential {
        StakeCredential::AddrKeyHash(hash.to_vec())
    }

    const STAKE_KEY_HASH: [u8; 3] = [0x99, 0x0f, 0x00];
    const DREP_HASH: [u8; 4] = [0xca, 0xfe, 0xd0, 0x0d];

    #[test]
    fn stake_addresses_initialise_to_first_delta_and_increment_subsequently() {
        let mut state = State::default();
        let stake_address = create_address(&STAKE_KEY_HASH);

        // Register first
        state.register_stake_address(&StakeCredential::AddrKeyHash(STAKE_KEY_HASH.to_vec()), None);

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(stake_addresses.len(), 1);
        }

        // Pass in deltas
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: stake_address.clone(),
                delta: 42,
            }],
        };

        state.handle_stake_deltas(&msg).unwrap();

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(stake_addresses.get(&stake_address).unwrap().utxo_value, 42);
        }

        state.handle_stake_deltas(&msg).unwrap();

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(stake_addresses.get(&stake_address).unwrap().utxo_value, 84);
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
        state
            .handle_spo_state(&SPOStateMessage {
                epoch: 1,
                spos: vec![
                    PoolRegistration {
                        operator: spo1.clone(),
                        vrf_key_hash: spo1.clone(),
                        pledge: 26,
                        cost: 0,
                        margin: Ratio {
                            numerator: 1,
                            denominator: 20,
                        },
                        reward_account: StakeAddress::default(),
                        pool_owners: Vec::new(),
                        relays: Vec::new(),
                        pool_metadata: None,
                    },
                    PoolRegistration {
                        operator: spo2.clone(),
                        vrf_key_hash: spo2.clone(),
                        pledge: 47,
                        cost: 10,
                        margin: Ratio {
                            numerator: 1,
                            denominator: 10,
                        },
                        reward_account: StakeAddress::default(),
                        pool_owners: Vec::new(),
                        relays: Vec::new(),
                        pool_metadata: None,
                    },
                ],
                retired_spos: vec![],
            })
            .unwrap();

        // Delegate
        let addr1 = create_address(&[0x11]);
        let cred1 = Credential::AddrKeyHash(addr1.get_hash().to_vec());
        state.register_stake_address(&cred1, None);
        state.record_stake_delegation(&cred1, &spo1);

        let addr2 = create_address(&[0x12]);
        let cred2 = Credential::AddrKeyHash(addr2.get_hash().to_vec());
        state.register_stake_address(&cred2, None);
        state.record_stake_delegation(&cred2, &spo2);

        // Put some value in
        let msg1 = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: addr1.clone(),
                delta: 42,
            }],
        };

        state.handle_stake_deltas(&msg1).unwrap();

        let msg2 = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: addr2.clone(),
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
        let reward_deltas = state.pay_mirs();
        assert_eq!(reward_deltas.len(), 0);
        assert_eq!(state.pots.reserves, 68);
        assert_eq!(state.pots.treasury, 32);
        assert_eq!(state.pots.deposits, 0);
    }

    #[test]
    fn mir_transfers_to_stake_addresses() {
        let mut state = State::default();
        let stake_address = create_address(&STAKE_KEY_HASH);
        let stake_credential = create_stake_credential(stake_address.get_hash());

        // Bootstrap with some in reserves
        state.pots.reserves = 100;

        // Set up one stake address
        state.register_stake_address(&stake_credential, None);

        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: stake_address.clone(),
                delta: 99,
            }],
        };
        state.handle_stake_deltas(&msg).unwrap();

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(stake_addresses.len(), 1);
            let sas = stake_addresses.get(&stake_address).unwrap();
            assert_eq!(sas.utxo_value, 99);
            assert_eq!(sas.rewards, 0);
        }

        // Send in a MIR reserves->{47,-5}->stake
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Reserves,
            target: InstantaneousRewardTarget::StakeCredentials(vec![
                (stake_credential.clone(), 47),
                (stake_credential, -5),
            ]),
        };

        state.handle_mir(&mir).unwrap();
        state.pay_mirs();
        assert_eq!(state.pots.reserves, 58);
        assert_eq!(state.pots.treasury, 0);
        assert_eq!(state.pots.deposits, 2_000_000); // Paid deposit

        let stake_addresses = state.stake_addresses.lock().unwrap();
        let sas = stake_addresses.get(&stake_address).unwrap();
        assert_eq!(sas.utxo_value, 99);
        assert_eq!(sas.rewards, 42);
    }

    #[test]
    fn withdrawal_transfers_from_stake_addresses() {
        let mut state = State::default();
        let stake_address = create_address(&STAKE_KEY_HASH);
        let stake_credential = create_stake_credential(stake_address.get_hash());

        // Bootstrap with some in reserves
        state.pots.reserves = 100;

        // Set up one stake address
        state.register_stake_address(&stake_credential, None);
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                address: stake_address.clone(),
                delta: 99,
            }],
        };

        state.handle_stake_deltas(&msg).unwrap();

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(stake_addresses.len(), 1);

            let sas = stake_addresses.get(&stake_address).unwrap();
            assert_eq!(sas.utxo_value, 99);
            assert_eq!(sas.rewards, 0);
        }

        // Send in a MIR reserves->42->stake
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Reserves,
            target: InstantaneousRewardTarget::StakeCredentials(vec![(stake_credential, 42)]),
        };

        state.handle_mir(&mir).unwrap();
        let diffs = state.pay_mirs();
        assert_eq!(state.pots.reserves, 58);
        assert_eq!(diffs.len(), 1);
        assert_eq!(diffs[0].stake_address.get_hash(), stake_address.get_hash());
        assert_eq!(diffs[0].delta, 42);

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            let sas = stake_addresses.get(&stake_address).unwrap();
            assert_eq!(sas.rewards, 42);
        }

        // Withdraw most of it
        let withdrawals = WithdrawalsMessage {
            withdrawals: vec![Withdrawal {
                address: stake_address.clone(),
                value: 39,
            }],
        };

        state.handle_withdrawals(&withdrawals).unwrap();

        let stake_addresses = state.stake_addresses.lock().unwrap();
        let sas = stake_addresses.get(&stake_address).unwrap();
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

        let spo1 = create_address(&[0x01]);
        let spo2 = create_address(&[0x02]);
        let spo3 = create_address(&[0x03]);
        let spo4 = create_address(&[0x04]);

        let spo1_credential = create_stake_credential(spo1.get_hash());
        let spo2_credential = create_stake_credential(spo2.get_hash());
        let spo3_credential = create_stake_credential(spo3.get_hash());
        let spo4_credential = create_stake_credential(spo4.get_hash());

        let certificates = vec![
            // register the first two SPOs separately from their delegation
            TxCertificate::Registration(Registration {
                credential: spo1_credential.clone(),
                deposit: 1,
            }),
            TxCertificate::Registration(Registration {
                credential: spo2_credential.clone(),
                deposit: 1,
            }),
            TxCertificate::VoteDelegation(VoteDelegation {
                credential: spo1_credential.clone(),
                drep: DRepChoice::Key(DREP_HASH.to_vec()),
            }),
            TxCertificate::StakeAndVoteDelegation(StakeAndVoteDelegation {
                credential: spo2_credential.clone(),
                operator: spo1.get_hash().to_vec(),
                drep: DRepChoice::Script(DREP_HASH.to_vec()),
            }),
            TxCertificate::StakeRegistrationAndVoteDelegation(StakeRegistrationAndVoteDelegation {
                credential: spo3_credential.clone(),
                drep: DRepChoice::Abstain,
                deposit: 1,
            }),
            TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(
                StakeRegistrationAndStakeAndVoteDelegation {
                    credential: spo4_credential,
                    operator: spo1.get_hash().to_vec(),
                    drep: DRepChoice::NoConfidence,
                    deposit: 1,
                },
            ),
        ];

        state.handle_tx_certificates(&TxCertificatesMessage { certificates })?;

        let deltas = vec![
            StakeAddressDelta {
                address: spo1,
                delta: 100,
            },
            StakeAddressDelta {
                address: spo2,
                delta: 1_000,
            },
            StakeAddressDelta {
                address: spo3,
                delta: 10_000,
            },
            StakeAddressDelta {
                address: spo4,
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
                plutus_v3_cost_model: CostModel::new(Vec::new()),
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
