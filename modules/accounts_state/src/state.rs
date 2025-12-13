//! Acropolis AccountsState: State storage
use crate::monetary::calculate_monetary_change;
use crate::rewards::{calculate_rewards, RewardsResult};
use crate::verifier::Verifier;
use acropolis_common::epoch_snapshot::EpochSnapshot;
use acropolis_common::queries::accounts::OptimalPoolSizing;
use acropolis_common::{
    math::update_value_with_delta,
    messages::{
        AccountsBootstrapMessage, DRepDelegationDistribution, DRepStateMessage,
        EpochActivityMessage, PotDeltasMessage, ProtocolParamsMessage, SPOStateMessage,
        StakeAddressDeltasMessage, TxCertificatesMessage, WithdrawalsMessage,
    },
    protocol_params::ProtocolParams,
    stake_addresses::{StakeAddressMap, StakeAddressState},
    BlockInfo, DRepChoice, DRepCredential, DelegatedStake, InstantaneousRewardSource,
    InstantaneousRewardTarget, Lovelace, MoveInstantaneousReward, PoolId, PoolLiveStakeInfo,
    PoolRegistration, Pot, RegistrationChange, RegistrationChangeKind, SPORewards, StakeAddress,
    StakeRewardDelta, TxCertificate,
};
pub(crate) use acropolis_common::{Pots, RewardType};
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

/// State for rewards calculation
#[derive(Debug, Default, Clone)]
pub struct EpochSnapshots {
    /// Latest snapshot (epoch i)
    pub mark: Arc<EpochSnapshot>,

    /// Previous snapshot (epoch i-1)
    pub set: Arc<EpochSnapshot>,

    /// One before that (epoch i-2)
    pub go: Arc<EpochSnapshot>,
}

impl EpochSnapshots {
    /// Push a new snapshot
    pub fn push(&mut self, latest: EpochSnapshot) {
        self.go = self.set.clone();
        self.set = self.mark.clone();
        self.mark = Arc::new(latest);
    }
}

/// Overall state - stored per block
#[derive(Debug, Default, Clone)]
pub struct State {
    /// Map of active SPOs by pool ID
    spos: OrdMap<PoolId, PoolRegistration>,

    /// List of SPOs (by pool ID) retiring in the current epoch
    retiring_spos: Vec<PoolId>,

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
    pool_refunds: Vec<(PoolId, StakeAddress)>,

    /// Addresses registration changes in current epoch
    current_epoch_registration_changes: Arc<Mutex<Vec<RegistrationChange>>>,

    /// Task for rewards calculation if necessary
    epoch_rewards_task: Arc<Mutex<Option<JoinHandle<Result<RewardsResult>>>>>,

    /// Signaller to start the above - delayed in early Shelley to replicate bug
    start_rewards_tx: Option<mpsc::Sender<()>>,
}

impl State {
    /// Bootstrap state from snapshot data (consumes the message to avoid cloning)
    pub fn bootstrap(&mut self, bootstrap_msg: AccountsBootstrapMessage) {
        let num_accounts = bootstrap_msg.accounts.len();
        let num_pools = bootstrap_msg.pools.len();
        let num_retiring = bootstrap_msg.retiring_pools.len();
        let num_dreps = bootstrap_msg.dreps.len();

        info!(
            "Bootstrapping accounts state for epoch {} with {} accounts, {} pools ({} retiring), {} dreps",
            bootstrap_msg.epoch, num_accounts, num_pools, num_retiring, num_dreps
        );

        // Load stake addresses
        {
            let mut stake_addresses = self.stake_addresses.lock().unwrap();
            for account in bootstrap_msg.accounts {
                stake_addresses.insert(account.stake_address, account.address_state);
            }
        }
        info!("Loaded {} stake addresses", num_accounts);

        // Load pools
        for pool_reg in bootstrap_msg.pools {
            let operator = pool_reg.operator;
            self.spos.insert(operator, pool_reg);
        }
        info!("Loaded {} pools", self.spos.len());

        // Load retiring pools
        self.retiring_spos = bootstrap_msg.retiring_pools;
        info!("Loaded {} retiring pools", self.retiring_spos.len());

        // Load DReps
        self.dreps = bootstrap_msg.dreps;
        info!("Loaded {} DReps", self.dreps.len());

        // Load pots
        self.pots = bootstrap_msg.pots;
        info!(
            "Loaded pots: reserves={}, treasury={}, deposits={}",
            self.pots.reserves, self.pots.treasury, self.pots.deposits
        );

        // Load mark/set/go snapshots if available
        if let Some(snapshots) = bootstrap_msg.bootstrap_snapshots {
            self.epoch_snapshots = EpochSnapshots {
                mark: Arc::new(snapshots.mark),
                set: Arc::new(snapshots.set),
                go: Arc::new(snapshots.go),
            };

            info!(
                "Loaded epoch snapshots: mark(epoch {}, {} SPOs), set(epoch {}, {} SPOs), go(epoch {}, {} SPOs)",
                self.epoch_snapshots.mark.epoch,
                self.epoch_snapshots.mark.spos.len(),
                self.epoch_snapshots.set.epoch,
                self.epoch_snapshots.set.spos.len(),
                self.epoch_snapshots.go.epoch,
                self.epoch_snapshots.go.spos.len(),
            );
        } else {
            info!("No bootstrap snapshot data available");
        }

        info!(
            "Accounts state bootstrap complete for epoch {}: {} accounts, {} pools, {} DReps, \
             pots(reserves={}, treasury={}, deposits={})",
            bootstrap_msg.epoch,
            num_accounts,
            self.spos.len(),
            self.dreps.len(),
            self.pots.reserves,
            self.pots.treasury,
            self.pots.deposits,
        );
    }

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
    pub fn get_pool_live_stake_info(&self, pool_operator: &PoolId) -> PoolLiveStakeInfo {
        self.stake_addresses.lock().unwrap().get_pool_live_stake_info(pool_operator)
    }

    /// Get Pools Live stake
    pub fn get_pools_live_stakes(&self, pool_operators: &[PoolId]) -> Vec<u64> {
        self.stake_addresses.lock().unwrap().get_pools_live_stakes(pool_operators)
    }

    /// Get Pool Delegators with live_stakes
    pub fn get_pool_delegators(&self, pool_operator: &PoolId) -> Vec<(StakeAddress, u64)> {
        self.stake_addresses.lock().unwrap().get_pool_delegators(pool_operator)
    }

    /// Get Drep Delegators with live_stakes
    pub fn get_drep_delegators(&self, drep: &DRepChoice) -> Vec<(StakeAddress, u64)> {
        self.stake_addresses.lock().unwrap().get_drep_delegators(drep)
    }

    /// Map stake_keys to their utxo_values
    pub fn get_accounts_utxo_values_map(
        &self,
        stake_keys: &[StakeAddress],
    ) -> Option<HashMap<StakeAddress, u64>> {
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
    ) -> Option<HashMap<StakeAddress, u64>> {
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
    ) -> Option<HashMap<StakeAddress, Option<DRepChoice>>> {
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
        spo_block_counts: HashMap<PoolId, usize>,
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

        // Capture a new snapshot for the end of the previous epoch and push it to state
        let snapshot = EpochSnapshot::new(
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
                registrations.iter().for_each(|addr| debug!(epoch, "Registration {}", addr));
                deregistrations.iter().for_each(|addr| debug!(epoch, "Deregistration {}", addr));
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
        for id in self.retiring_spos.drain(..) {
            info!(epoch, "SPO {id} has retired");
            self.spos.remove(&id);

            // Wipe any delegations to this pool
            self.stake_addresses.lock().unwrap().remove_all_delegations_to(&id);
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
        for (pool, stake_address) in refunds {
            // If their reward account has been deregistered, it goes to Treasury
            let mut stake_addresses = self.stake_addresses.lock().unwrap();
            if stake_addresses.is_registered(&stake_address) {
                reward_deltas.push(StakeRewardDelta {
                    stake_address: stake_address.clone(),
                    delta: deposit,
                    reward_type: RewardType::PoolRefund,
                    pool,
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

    /// Pay MIRs
    fn pay_mir(&mut self, mir: &MoveInstantaneousReward) {
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
            InstantaneousRewardTarget::StakeAddresses(deltas) => {
                // Transfer to a stake addresses from a pot
                let mut total_value: u64 = 0;
                for (stake_address, value) in deltas.iter() {
                    // Get old stake address state, or create one
                    let mut stake_addresses = self.stake_addresses.lock().unwrap();
                    let sas = stake_addresses.entry(stake_address.clone()).or_default();

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

    /// Derive the Stake Pool Delegation Distribution (SPDD) - a map of total stake values
    /// (both with and without rewards) for each active SPO
    /// And Stake Pool Reward State (rewards and delegators_count for each pool)
    /// Key of returned map is the SPO 'operator' ID
    pub fn generate_spdd(&self) -> BTreeMap<PoolId, DelegatedStake> {
        let stake_addresses = self.stake_addresses.lock().unwrap();
        stake_addresses.generate_spdd()
    }

    pub fn dump_spdd_state(&self) -> HashMap<PoolId, Vec<(StakeAddress, u64)>> {
        let stake_addresses = self.stake_addresses.lock().unwrap();
        stake_addresses.dump_spdd_state()
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

    /// Complete the previous epoch rewards calculation
    /// And apply the rewards to the stake_addresses
    /// This function is called at NEWEPOCH tick from epoch N-1 to N
    ///
    /// This also returns SPO rewards (from epoch N-1) for publishing to the SPDD topic
    /// and stake reward deltas for publishing to the StakeRewardDeltas topic
    pub async fn complete_previous_epoch_rewards_calculation(
        &mut self,
        verifier: &Verifier,
    ) -> Result<(Vec<(PoolId, SPORewards)>, Vec<StakeRewardDelta>)> {
        // Collect stake addresses reward deltas
        let mut spo_rewards: Vec<(PoolId, SPORewards)> = Vec::new();
        let mut reward_deltas = Vec::<StakeRewardDelta>::new();

        // Check previous epoch rewards calculation is done
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
            if let Ok(Ok(rewards_result)) = task.await {
                // Pay the rewards
                let mut stake_addresses = self.stake_addresses.lock().unwrap();
                let mut filtered_rewards_result = rewards_result.clone();
                for (spo, rewards) in rewards_result.rewards {
                    for reward in rewards {
                        if stake_addresses.is_registered(&reward.account) {
                            stake_addresses.add_to_reward(&reward.account, reward.amount);
                            reward_deltas.push(StakeRewardDelta {
                                stake_address: reward.account.clone(),
                                delta: reward.amount,
                                reward_type: reward.rtype.clone(),
                                pool: reward.pool,
                            });
                        } else {
                            warn!(
                                "Reward account {} deregistered - paying reward {} to treasury",
                                reward.account, reward.amount
                            );
                            self.pots.treasury += reward.amount;

                            // Remove from filtered version for comparison and result
                            if let Some(rewards) = filtered_rewards_result.rewards.get_mut(&spo) {
                                rewards.retain(|r| r.account != reward.account);
                            }

                            if let Some((_, spor)) = filtered_rewards_result
                                .spo_rewards
                                .iter_mut()
                                .find(|(fspo, _)| *fspo == spo)
                            {
                                spor.total_rewards -= reward.amount;
                                if reward.rtype == RewardType::Leader {
                                    spor.operator_rewards -= reward.amount;
                                }
                            }
                        }
                    }
                }

                // Verify them
                verifier.verify_rewards(&filtered_rewards_result);

                // save SPO rewards
                spo_rewards = filtered_rewards_result.spo_rewards.clone();

                // Adjust the reserves for next time with amount actually paid
                self.pots.reserves -= rewards_result.total_paid;
            }
        };

        Ok((spo_rewards, reward_deltas))
    }

    /// Handle an EpochActivityMessage giving total fees and block counts by SPO for
    /// the just-ended epoch
    ///
    /// This returns stake reward deltas (Refund for pools retiring at epoch N) for publishing to the StakeRewardDeltas topic
    pub async fn handle_epoch_activity(
        &mut self,
        ea_msg: &EpochActivityMessage,
        verifier: &Verifier,
    ) -> Result<Vec<StakeRewardDelta>> {
        let mut reward_deltas = Vec::<StakeRewardDelta>::new();

        // Map block counts, filtering out SPOs we don't know (OBFT in early Shelley)
        let spo_blocks: HashMap<PoolId, usize> = ea_msg
            .spo_blocks
            .iter()
            .filter(|(hash, _)| self.spos.contains_key(hash))
            .map(|(hash, count)| (*hash, *count))
            .collect();

        // Enter epoch - note the message specifies the epoch that has just *ended*
        reward_deltas.extend(self.enter_epoch(
            ea_msg.epoch + 1,
            ea_msg.total_fees,
            spo_blocks,
            verifier,
        )?);

        Ok(reward_deltas)
    }

    /// Handle an SPOStateMessage with the full set of SPOs valid at the end of the last
    /// epoch
    pub fn handle_spo_state(&mut self, spo_msg: &SPOStateMessage) -> Result<()> {
        // Capture current SPOs, mapped by operator ID
        let new_spos: OrdMap<PoolId, PoolRegistration> =
            spo_msg.spos.iter().cloned().map(|spo| (spo.operator, spo)).collect();

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
                        || spo.reward_account != old_spo.reward_account
                    {
                        debug!(
                            epoch = spo_msg.epoch,
                            pledge = spo.pledge,
                            cost = spo.cost,
                            margin = ?spo.margin,
                            reward = %spo.reward_account,
                            "Updated parameters for SPO {}",
                            id
                        );
                    }
                }

                _ => {
                    debug!(
                        epoch = spo_msg.epoch,
                        pledge = spo.pledge,
                        cost = spo.cost,
                        margin = ?spo.margin,
                        reward = %spo.reward_account,
                        "Registered new SPO {}",
                        id
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
                    id, retired_spo.reward_account
                );
                self.pool_refunds.push((retired_spo.operator, retired_spo.reward_account.clone()));
                // Store full StakeAddress
            }

            // Schedule to retire - we need them to still be in place when we count
            // blocks for the previous epoch
            self.retiring_spos.push(*id);
        }

        self.spos = new_spos;
        Ok(())
    }

    /// Register a stake address, with a specified deposit if known
    fn register_stake_address(&mut self, stake_address: &StakeAddress, deposit: Option<Lovelace>) {
        debug!("Register stake address {stake_address}");
        // Stake addresses can be registered after being used in UTXOs
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        if stake_addresses.register_stake_address(stake_address) {
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
            address: stake_address.clone(),
            kind: RegistrationChangeKind::Registered,
        });
    }

    /// Deregister a stake address, with specified refund if known
    fn deregister_stake_address(&mut self, stake_address: &StakeAddress, refund: Option<Lovelace>) {
        debug!("Deregister stake address {stake_address}");

        // Check if it existed
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        if stake_addresses.deregister_stake_address(stake_address) {
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

            // Add to registration changes
            self.current_epoch_registration_changes.lock().unwrap().push(RegistrationChange {
                address: stake_address.clone(),
                kind: RegistrationChangeKind::Deregistered,
            });
        }
    }

    pub fn handle_drep_state(&mut self, drep_msg: &DRepStateMessage) {
        self.dreps = drep_msg.dreps.clone();
    }

    /// Record a stake delegation
    fn record_stake_delegation(&mut self, stake_address: &StakeAddress, spo: &PoolId) {
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        debug!("Delegation of {} to {}", stake_address, spo);
        stake_addresses.record_stake_delegation(stake_address, spo);
    }

    /// record a drep delegation
    fn record_drep_delegation(&mut self, stake_address: &StakeAddress, drep: &DRepChoice) {
        let mut stake_addresses = self.stake_addresses.lock().unwrap();
        stake_addresses.record_drep_delegation(stake_address, drep);
    }

    /// Handle TxCertificates
    pub fn handle_tx_certificates(&mut self, tx_certs_msg: &TxCertificatesMessage) -> Result<()> {
        // Handle certificates
        for tx_cert in tx_certs_msg.certificates.iter() {
            match &tx_cert.cert {
                TxCertificate::StakeRegistration(reg) => {
                    self.register_stake_address(reg, None);
                }

                TxCertificate::StakeDeregistration(dreg) => {
                    self.deregister_stake_address(dreg, None);
                }

                TxCertificate::MoveInstantaneousReward(mir) => {
                    self.pay_mir(mir);
                }

                TxCertificate::Registration(reg) => {
                    self.register_stake_address(&reg.stake_address, Some(reg.deposit));
                }

                TxCertificate::Deregistration(dreg) => {
                    self.deregister_stake_address(&dreg.stake_address, Some(dreg.refund));
                }

                TxCertificate::StakeDelegation(delegation) => {
                    self.record_stake_delegation(&delegation.stake_address, &delegation.operator);
                }

                TxCertificate::VoteDelegation(delegation) => {
                    self.record_drep_delegation(&delegation.stake_address, &delegation.drep);
                }

                TxCertificate::StakeAndVoteDelegation(delegation) => {
                    self.record_stake_delegation(&delegation.stake_address, &delegation.operator);
                    self.record_drep_delegation(&delegation.stake_address, &delegation.drep);
                }

                TxCertificate::StakeRegistrationAndDelegation(delegation) => {
                    self.register_stake_address(
                        &delegation.stake_address,
                        Some(delegation.deposit),
                    );
                    self.record_stake_delegation(&delegation.stake_address, &delegation.operator);
                }

                TxCertificate::StakeRegistrationAndVoteDelegation(delegation) => {
                    self.register_stake_address(
                        &delegation.stake_address,
                        Some(delegation.deposit),
                    );
                    self.record_drep_delegation(&delegation.stake_address, &delegation.drep);
                }

                TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(delegation) => {
                    self.register_stake_address(
                        &delegation.stake_address,
                        Some(delegation.deposit),
                    );
                    self.record_stake_delegation(&delegation.stake_address, &delegation.operator);
                    self.record_drep_delegation(&delegation.stake_address, &delegation.drep);
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
    use acropolis_common::crypto::{keyhash_224, keyhash_256};
    use acropolis_common::{
        protocol_params::ConwayParams, rational_number::RationalNumber, Anchor, Committee,
        Constitution, CostModel, DRepVotingThresholds, KeyHash, NetworkId, PoolVotingThresholds,
        Pot, PotDelta, Ratio, Registration, StakeAddress, StakeAddressDelta,
        StakeAndVoteDelegation, StakeCredential, StakeRegistrationAndStakeAndVoteDelegation,
        StakeRegistrationAndVoteDelegation, TxCertificateWithPos, TxIdentifier, VoteDelegation,
        VrfKeyHash, Withdrawal,
    };

    // Helper to create a StakeAddress from a byte slice
    fn create_address(hash: &[u8]) -> StakeAddress {
        let mut full_hash = vec![0u8; 28];
        full_hash[..hash.len().min(28)].copy_from_slice(&hash[..hash.len().min(28)]);
        StakeAddress {
            network: NetworkId::Mainnet,
            credential: StakeCredential::AddrKeyHash(full_hash.try_into().unwrap()),
        }
    }

    fn test_keyhash(byte: u8) -> KeyHash {
        keyhash_224(&[byte])
    }

    fn test_keyhash_from_bytes(bytes: &[u8]) -> KeyHash {
        keyhash_224(bytes)
    }

    fn test_vrf_keyhash(byte: u8) -> VrfKeyHash {
        keyhash_256(&[byte]).into()
    }

    const STAKE_KEY_HASH: [u8; 3] = [0x99, 0x0f, 0x00];
    const DREP_HASH: [u8; 4] = [0xca, 0xfe, 0xd0, 0x0d];

    #[test]
    fn stake_addresses_initialise_to_first_delta_and_increment_subsequently() {
        let mut state = State::default();
        let stake_address = create_address(&STAKE_KEY_HASH);

        // Register first
        state.register_stake_address(&stake_address, None);

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            assert_eq!(stake_addresses.len(), 1);
        }

        // Pass in deltas
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                stake_address: stake_address.clone(),
                addresses: Vec::new(),
                tx_count: 1,
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

        let spo1 = test_keyhash(0x01).into();
        let spo2 = test_keyhash(0x02).into();

        let vrf_key_hash_1 = test_vrf_keyhash(0x03);
        let vrf_key_hash_2 = test_vrf_keyhash(0x04);

        // Create the SPOs
        state
            .handle_spo_state(&SPOStateMessage {
                epoch: 1,
                spos: vec![
                    PoolRegistration {
                        operator: spo1,
                        vrf_key_hash: vrf_key_hash_1,
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
                        operator: spo2,
                        vrf_key_hash: vrf_key_hash_2,
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
        state.register_stake_address(&addr1, None);
        state.record_stake_delegation(&addr1, &spo1);

        let addr2 = create_address(&[0x12]);
        state.register_stake_address(&addr2, None);
        state.record_stake_delegation(&addr2, &spo2);

        // Put some value in
        let msg1 = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                stake_address: addr1.clone(),
                addresses: Vec::new(),
                tx_count: 1,
                delta: 42,
            }],
        };

        state.handle_stake_deltas(&msg1).unwrap();

        let msg2 = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                stake_address: addr2.clone(),
                addresses: Vec::new(),
                tx_count: 1,
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

        state.pay_mir(&mir);
        assert_eq!(state.pots.reserves, 58);
        assert_eq!(state.pots.treasury, 42);
        assert_eq!(state.pots.deposits, 0);

        // Send some of it back
        let mir = MoveInstantaneousReward {
            source: InstantaneousRewardSource::Treasury,
            target: InstantaneousRewardTarget::OtherAccountingPot(10),
        };

        state.pay_mir(&mir);
        assert_eq!(state.pots.reserves, 68);
        assert_eq!(state.pots.treasury, 32);
        assert_eq!(state.pots.deposits, 0);
    }

    #[test]
    fn mir_transfers_to_stake_addresses() {
        let mut state = State::default();
        let stake_address = create_address(&STAKE_KEY_HASH);

        // Bootstrap with some in reserves
        state.pots.reserves = 100;

        // Set up one stake address
        state.register_stake_address(&stake_address, None);

        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                stake_address: stake_address.clone(),
                addresses: Vec::new(),
                tx_count: 1,
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
            target: InstantaneousRewardTarget::StakeAddresses(vec![
                (stake_address.clone(), 47),
                (stake_address.clone(), -5),
            ]),
        };

        state.pay_mir(&mir);
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

        // Bootstrap with some in reserves
        state.pots.reserves = 100;

        // Set up one stake address
        state.register_stake_address(&stake_address, None);
        let msg = StakeAddressDeltasMessage {
            deltas: vec![StakeAddressDelta {
                stake_address: stake_address.clone(),
                addresses: Vec::new(),
                tx_count: 1,
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
            target: InstantaneousRewardTarget::StakeAddresses(vec![(stake_address.clone(), 42)]),
        };

        state.pay_mir(&mir);

        {
            let stake_addresses = state.stake_addresses.lock().unwrap();
            let sas = stake_addresses.get(&stake_address).unwrap();
            assert_eq!(state.pots.reserves, 58);
            assert_eq!(sas.rewards, 42);
        }

        // Withdraw most of it
        let withdrawals = WithdrawalsMessage {
            withdrawals: vec![Withdrawal {
                address: stake_address.clone(),
                value: 39,
                tx_identifier: TxIdentifier::default(),
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

        let drep_addr_cred = DRepCredential::AddrKeyHash(test_keyhash_from_bytes(&DREP_HASH));
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

        let drep_addr_cred = DRepCredential::AddrKeyHash(test_keyhash_from_bytes(&DREP_HASH));
        let drep_script_cred = DRepCredential::ScriptHash(test_keyhash_from_bytes(&DREP_HASH));
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

        let tx_identifier = TxIdentifier::default();

        // Get the KeyHash once
        let spo1_hash = spo1.get_hash();
        let drep_key_hash = test_keyhash_from_bytes(&DREP_HASH);
        let pool_id_1 = (*spo1_hash).into();

        let certificates = vec![
            // register the first two SPOs separately from their delegation
            TxCertificateWithPos {
                cert: TxCertificate::Registration(Registration {
                    stake_address: spo1.clone(),
                    deposit: 1,
                }),
                tx_identifier,
                cert_index: 0,
            },
            TxCertificateWithPos {
                cert: TxCertificate::Registration(Registration {
                    stake_address: spo2.clone(),
                    deposit: 1,
                }),
                tx_identifier,
                cert_index: 0,
            },
            TxCertificateWithPos {
                cert: TxCertificate::VoteDelegation(VoteDelegation {
                    stake_address: spo1.clone(),
                    drep: DRepChoice::Key(drep_key_hash),
                }),
                tx_identifier,
                cert_index: 0,
            },
            TxCertificateWithPos {
                cert: TxCertificate::StakeAndVoteDelegation(StakeAndVoteDelegation {
                    stake_address: spo2.clone(),
                    operator: pool_id_1,
                    drep: DRepChoice::Script(drep_key_hash),
                }),
                tx_identifier,
                cert_index: 0,
            },
            TxCertificateWithPos {
                cert: TxCertificate::StakeRegistrationAndVoteDelegation(
                    StakeRegistrationAndVoteDelegation {
                        stake_address: spo3.clone(),
                        drep: DRepChoice::Abstain,
                        deposit: 1,
                    },
                ),
                tx_identifier,
                cert_index: 0,
            },
            TxCertificateWithPos {
                cert: TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(
                    StakeRegistrationAndStakeAndVoteDelegation {
                        stake_address: spo4.clone(),
                        operator: pool_id_1,
                        drep: DRepChoice::NoConfidence,
                        deposit: 1,
                    },
                ),
                tx_identifier,
                cert_index: 0,
            },
        ];

        state.handle_tx_certificates(&TxCertificatesMessage { certificates })?;

        let deltas = vec![
            StakeAddressDelta {
                stake_address: spo1,
                addresses: Vec::new(),
                tx_count: 1,
                delta: 100,
            },
            StakeAddressDelta {
                stake_address: spo2,
                addresses: Vec::new(),
                tx_count: 1,
                delta: 1_000,
            },
            StakeAddressDelta {
                stake_address: spo3,
                addresses: Vec::new(),
                tx_count: 1,
                delta: 10_000,
            },
            StakeAddressDelta {
                stake_address: spo4,
                addresses: Vec::new(),
                tx_count: 1,
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
                dreps: vec![(drep_script_cred, 2_001_000), (drep_addr_cred, 1_000_100)],
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
