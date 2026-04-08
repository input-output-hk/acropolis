use crate::rewards::RewardsResult;
use crate::state::PendingRewardsPlan;
use acropolis_common::stake_addresses::{StakeAddressMap, StakeAddressState};
use acropolis_common::{
    math::update_value_with_delta, params::SECURITY_PARAMETER_K, BlockInfo, DRepChoice, PoolId,
    RegistrationChange, StakeAddress,
};
use std::collections::{HashMap, VecDeque};
use std::sync::{mpsc, Arc, Mutex};
use tokio::task::JoinHandle;
use tracing::{error, info};

#[derive(Debug, Default)]
pub(crate) struct AccountsRuntime {
    pub(crate) stake_address_undo_history: StakeAddressUndoHistory,
    pub(crate) rewards: RewardRuntime,
}

#[derive(Debug, Default)]
pub(crate) struct RewardRuntime {
    epoch_rewards_task: Option<JoinHandle<anyhow::Result<RewardsResult>>>,
    start_rewards_tx: Option<mpsc::Sender<()>>,
    current_epoch_registration_changes: Option<Arc<Mutex<Vec<RegistrationChange>>>>,
    active_epoch: Option<u64>,
    active_rewards_plan: Option<PendingRewardsPlan>,
    previous_rewards_plan: Option<PendingRewardsPlan>,
    previous_rewards: Option<RewardsResult>,
}

impl RewardRuntime {
    pub(crate) fn set_epoch_rewards_task(
        &mut self,
        task: JoinHandle<anyhow::Result<RewardsResult>>,
    ) {
        self.epoch_rewards_task = Some(task);
    }

    pub(crate) fn set_rewards_plan(&mut self, plan: PendingRewardsPlan) {
        self.previous_rewards_plan = self.active_rewards_plan.take();
        self.active_rewards_plan = Some(plan);
    }

    pub(crate) fn set_epoch_rewards_result(&mut self, result: RewardsResult) {
        tracing::info!("rollback validation: setting epoch reward result");
        self.previous_rewards = Some(result);
    }

    pub(crate) fn get_rewards_plan(&self) -> Option<PendingRewardsPlan> {
        self.active_rewards_plan.clone()
    }

    pub(crate) fn take_epoch_rewards_task(
        &mut self,
    ) -> Option<JoinHandle<anyhow::Result<RewardsResult>>> {
        let task = self.epoch_rewards_task.take();
        if task.is_some() {
            self.start_rewards_tx = None;
            self.current_epoch_registration_changes = None;
            self.active_epoch = None;
        }
        task
    }

    pub(crate) fn set_start_rewards_tx(&mut self, tx: mpsc::Sender<()>) {
        self.start_rewards_tx = Some(tx);
    }

    pub(crate) fn notify_block(
        &mut self,
        block_number: u64,
        epoch_slot: u64,
        stability_window: u64,
    ) {
        if let Some(tx) = &self.start_rewards_tx {
            if epoch_slot >= stability_window {
                info!(
                    "Starting rewards calculation at block {}, epoch slot {}",
                    block_number, epoch_slot
                );
                let _ = tx.send(());
                self.start_rewards_tx = None;
            }
        }
    }

    pub(crate) fn begin_epoch_registration_changes(
        &mut self,
        epoch: u64,
    ) -> Arc<Mutex<Vec<RegistrationChange>>> {
        let current_epoch_registration_changes = Arc::new(Mutex::new(Vec::new()));
        self.active_epoch = Some(epoch);
        self.current_epoch_registration_changes = Some(current_epoch_registration_changes.clone());
        current_epoch_registration_changes
    }

    pub(crate) fn push_registration_change(&mut self, change: RegistrationChange) {
        if let Some(current_epoch_registration_changes) = &self.current_epoch_registration_changes {
            if let Ok(mut changes) = current_epoch_registration_changes.lock() {
                changes.push(change);
            }
        }
    }

    pub(crate) fn rollback_to(
        &mut self,
        rollback_block: &BlockInfo,
        current_epoch_registration_changes: &[RegistrationChange],
        stability_window_slot: u64,
    ) {
        if let Some(runtime_changes) = &self.current_epoch_registration_changes {
            if let Ok(mut changes) = runtime_changes.lock() {
                *changes = current_epoch_registration_changes.to_vec();
            }
        }

        tracing::info!(
            "rollback validation: epoch boundary rollback; active: {:?} actual: {}",
            self.active_epoch,
            rollback_block.epoch
        );

        if self.active_epoch != Some(rollback_block.epoch) {
            self.active_rewards_plan = self.previous_rewards_plan.take();

            if rollback_block.epoch_slot <= stability_window_slot {
                // Recalculate rewards using new registration changes if rollback to previous epoch is
                // before the stability window slot.
                tracing::info!(
                    "rollback validation: rollback before stability window, creating fresh calculation"
                );
                self.clear_on_rollback();
            } else if let Some(result) = self.previous_rewards.clone() {
                // Recreate the rewards task using the finished reward result if rollback to previous epoch
                // is not before the stability window slot.
                tracing::info!("rollback validation: reusing existing reward result");
                self.epoch_rewards_task = Some(tokio::spawn(async move { Ok(result) }));
                self.active_epoch = Some(rollback_block.epoch);
            }
        }
    }

    pub(crate) fn clear_on_rollback(&mut self) {
        if let Some(task) = self.epoch_rewards_task.take() {
            task.abort();
        }
        self.start_rewards_tx = None;
        self.current_epoch_registration_changes = None;
        self.active_epoch = None;
    }
}

#[derive(Debug, Clone)]
struct StakeAddressUndoDelta {
    existed_before: bool,
    inverse_utxo_delta: i64,
    inverse_rewards_delta: i64,
    previous_registered: bool,
    previous_delegated_spo: Option<PoolId>,
    previous_delegated_drep: Option<DRepChoice>,
}

impl StakeAddressUndoDelta {
    fn from_previous(previous: Option<&StakeAddressState>) -> Self {
        Self {
            existed_before: previous.is_some(),
            inverse_utxo_delta: 0,
            inverse_rewards_delta: 0,
            previous_registered: previous.map(|state| state.registered).unwrap_or(false),
            previous_delegated_spo: previous.and_then(|state| state.delegated_spo),
            previous_delegated_drep: previous.and_then(|state| state.delegated_drep.clone()),
        }
    }

    fn accumulate(
        &mut self,
        previous: Option<&StakeAddressState>,
        current: Option<&StakeAddressState>,
    ) {
        self.inverse_utxo_delta += value_delta(
            previous.map(|state| state.utxo_value),
            current.map(|state| state.utxo_value),
        );
        self.inverse_rewards_delta += value_delta(
            previous.map(|state| state.rewards),
            current.map(|state| state.rewards),
        );
    }

    fn rollback(&self, stake_address: &StakeAddress, stake_addresses: &mut StakeAddressMap) {
        if !self.existed_before {
            stake_addresses.remove(stake_address);
            return;
        }

        let mut restored = stake_addresses.get(stake_address).unwrap_or_default();

        if let Err(error) =
            update_value_with_delta(&mut restored.utxo_value, self.inverse_utxo_delta)
        {
            error!(
                stake_address = %stake_address,
                inverse_utxo_delta = self.inverse_utxo_delta,
                "Failed to roll back stake address utxo value: {error}"
            );
        }

        if let Err(error) =
            update_value_with_delta(&mut restored.rewards, self.inverse_rewards_delta)
        {
            error!(
                stake_address = %stake_address,
                inverse_rewards_delta = self.inverse_rewards_delta,
                "Failed to roll back stake address rewards: {error}"
            );
        }

        restored.registered = self.previous_registered;
        restored.delegated_spo = self.previous_delegated_spo;
        restored.delegated_drep = self.previous_delegated_drep.clone();
        stake_addresses.insert(stake_address.clone(), restored);
    }
}

fn value_delta(previous: Option<u64>, current: Option<u64>) -> i64 {
    let previous = i128::from(previous.unwrap_or_default());
    let current = i128::from(current.unwrap_or_default());
    let delta = previous - current;
    let clamped_delta = delta.clamp(i128::from(i64::MIN), i128::from(i64::MAX));

    if clamped_delta != delta {
        error!(
            previous,
            current, delta, clamped_delta, "Stake address delta exceeded i64 range and was clamped"
        );
    }

    clamped_delta as i64
}

#[derive(Debug, Default)]
pub(crate) struct BlockStakeAddressUndoRecorder {
    changes: HashMap<StakeAddress, StakeAddressUndoDelta>,
    reward_deltas: HashMap<StakeAddress, i64>,
}

impl BlockStakeAddressUndoRecorder {
    pub(crate) fn record_change(
        &mut self,
        stake_address: &StakeAddress,
        previous: Option<&StakeAddressState>,
        current: Option<&StakeAddressState>,
    ) {
        if previous == current && !self.changes.contains_key(stake_address) {
            return;
        }

        let change = self
            .changes
            .entry(stake_address.clone())
            .or_insert_with(|| StakeAddressUndoDelta::from_previous(previous));
        change.accumulate(previous, current);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.reward_deltas.is_empty()
    }

    pub(crate) fn record_reward_delta(&mut self, stake_address: &StakeAddress, inverse_delta: i64) {
        if inverse_delta == 0 {
            return;
        }

        *self.reward_deltas.entry(stake_address.clone()).or_default() += inverse_delta;
    }
}

#[derive(Debug)]
struct StakeAddressUndoEntry {
    index: u64,
    changes: HashMap<StakeAddress, StakeAddressUndoDelta>,
    reward_deltas: HashMap<StakeAddress, i64>,
}

#[derive(Debug)]
pub(crate) struct StakeAddressUndoHistory {
    history: VecDeque<StakeAddressUndoEntry>,
    retention: u64,
}

impl Default for StakeAddressUndoHistory {
    fn default() -> Self {
        Self::new(SECURITY_PARAMETER_K)
    }
}

impl StakeAddressUndoHistory {
    pub(crate) fn new(retention: u64) -> Self {
        Self {
            history: VecDeque::new(),
            retention,
        }
    }

    pub(crate) fn commit(&mut self, index: u64, recorder: BlockStakeAddressUndoRecorder) {
        if recorder.is_empty() {
            return;
        }

        while let Some(entry) = self.history.front() {
            if index.saturating_sub(entry.index) > self.retention {
                self.history.pop_front();
            } else {
                break;
            }
        }

        self.history.push_back(StakeAddressUndoEntry {
            index,
            changes: recorder.changes,
            reward_deltas: recorder.reward_deltas,
        });
    }

    pub(crate) fn rollback_to(&mut self, index: u64, stake_addresses: &mut StakeAddressMap) {
        while let Some(entry) = self.history.back() {
            if entry.index >= index {
                let entry = self.history.pop_back().expect("checked back above");
                for (stake_address, change) in entry.changes {
                    change.rollback(&stake_address, stake_addresses);
                }
                for (stake_address, inverse_delta) in entry.reward_deltas {
                    let Some(stake_address_state) = stake_addresses.get_mut(&stake_address) else {
                        error!(
                            stake_address = %stake_address,
                            inverse_reward_delta = inverse_delta,
                            "Failed to roll back compact reward delta: unknown stake address"
                        );
                        continue;
                    };

                    if let Err(error) =
                        update_value_with_delta(&mut stake_address_state.rewards, inverse_delta)
                    {
                        error!(
                            stake_address = %stake_address,
                            inverse_reward_delta = inverse_delta,
                            "Failed to roll back compact reward delta: {error}"
                        );
                    }
                }
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{
        hash::Hash, BlockHash, BlockIntent, BlockStatus, DRepCredential, Era, KeyHash, NetworkId,
        RegistrationChangeKind, StakeCredential,
    };

    fn stake_address(seed: u8) -> StakeAddress {
        StakeAddress::new(
            StakeCredential::AddrKeyHash(KeyHash::new([seed; 28])),
            NetworkId::Mainnet,
        )
    }

    fn pool_id(seed: u8) -> PoolId {
        PoolId::new(Hash::new([seed; 28]))
    }

    fn drep(seed: u8) -> DRepChoice {
        match DRepCredential::AddrKeyHash(KeyHash::new([seed; 28])) {
            DRepCredential::AddrKeyHash(hash) => DRepChoice::Key(hash),
            DRepCredential::ScriptHash(hash) => DRepChoice::Script(hash),
        }
    }

    fn block_info(epoch: u64, epoch_slot: u64, new_epoch: bool) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            intent: BlockIntent::Apply,
            slot: epoch_slot,
            number: epoch_slot,
            hash: BlockHash::new([epoch as u8; 32]),
            epoch,
            epoch_slot,
            new_epoch,
            is_new_era: false,
            tip_slot: None,
            timestamp: 0,
            era: Era::Byron,
        }
    }

    #[test]
    fn recorder_accumulates_first_touch_per_block() {
        let stake_address = stake_address(1);
        let mut recorder = BlockStakeAddressUndoRecorder::default();
        let previous = StakeAddressState {
            registered: true,
            utxo_value: 10,
            rewards: 5,
            delegated_spo: Some(pool_id(1)),
            delegated_drep: Some(drep(1)),
        };
        let current = StakeAddressState {
            registered: true,
            utxo_value: 12,
            rewards: 6,
            delegated_spo: Some(pool_id(2)),
            delegated_drep: Some(drep(2)),
        };
        let final_state = StakeAddressState {
            registered: false,
            utxo_value: 9,
            rewards: 4,
            delegated_spo: None,
            delegated_drep: None,
        };

        recorder.record_change(&stake_address, Some(&previous), Some(&current));
        recorder.record_change(&stake_address, Some(&current), Some(&final_state));

        let change = recorder.changes.get(&stake_address).unwrap();
        assert!(change.existed_before);
        assert_eq!(change.inverse_utxo_delta, 1);
        assert_eq!(change.inverse_rewards_delta, 1);
        assert!(change.previous_registered);
        assert_eq!(change.previous_delegated_spo, Some(pool_id(1)));
        assert_eq!(change.previous_delegated_drep, Some(drep(1)));
    }

    #[test]
    fn undo_history_rolls_back_created_addresses() {
        let stake_address = stake_address(2);
        let mut stake_addresses = StakeAddressMap::default();
        let mut history = StakeAddressUndoHistory::new(10);
        let mut recorder = BlockStakeAddressUndoRecorder::default();

        let created = StakeAddressState {
            registered: true,
            utxo_value: 22,
            rewards: 7,
            delegated_spo: Some(pool_id(3)),
            delegated_drep: Some(drep(3)),
        };
        recorder.record_change(&stake_address, None, Some(&created));
        stake_addresses.insert(stake_address.clone(), created);

        history.commit(11, recorder);
        history.rollback_to(11, &mut stake_addresses);

        assert!(stake_addresses.get(&stake_address).is_none());
    }

    #[test]
    fn undo_history_restores_field_deltas() {
        let stake_address = stake_address(3);
        let mut stake_addresses = StakeAddressMap::default();
        let mut history = StakeAddressUndoHistory::new(10);
        let mut recorder = BlockStakeAddressUndoRecorder::default();
        let previous = StakeAddressState {
            registered: true,
            utxo_value: 100,
            rewards: 25,
            delegated_spo: Some(pool_id(4)),
            delegated_drep: Some(drep(4)),
        };
        stake_addresses.insert(stake_address.clone(), previous.clone());

        let current = StakeAddressState {
            registered: false,
            utxo_value: 60,
            rewards: 15,
            delegated_spo: None,
            delegated_drep: None,
        };
        recorder.record_change(&stake_address, Some(&previous), Some(&current));
        stake_addresses.insert(stake_address.clone(), current);

        history.commit(12, recorder);
        history.rollback_to(12, &mut stake_addresses);

        assert_eq!(stake_addresses.get(&stake_address), Some(previous));
    }

    #[test]
    fn undo_history_restores_compact_reward_deltas() {
        let stake_address = stake_address(9);
        let mut stake_addresses = StakeAddressMap::default();
        let mut history = StakeAddressUndoHistory::new(10);
        let mut recorder = BlockStakeAddressUndoRecorder::default();

        stake_addresses.insert(
            stake_address.clone(),
            StakeAddressState {
                registered: true,
                utxo_value: 10,
                rewards: 100,
                delegated_spo: Some(pool_id(1)),
                delegated_drep: Some(drep(1)),
            },
        );

        stake_addresses.get_mut(&stake_address).unwrap().rewards += 25;
        recorder.record_reward_delta(&stake_address, -25);

        history.commit(13, recorder);
        history.rollback_to(13, &mut stake_addresses);

        assert_eq!(stake_addresses.get(&stake_address).unwrap().rewards, 100);
    }

    #[test]
    fn undo_history_applies_generic_then_compact_reward_rollback() {
        let stake_address = stake_address(10);
        let mut stake_addresses = StakeAddressMap::default();
        let mut history = StakeAddressUndoHistory::new(10);
        let mut recorder = BlockStakeAddressUndoRecorder::default();

        let previous = StakeAddressState {
            registered: true,
            utxo_value: 0,
            rewards: 100,
            delegated_spo: Some(pool_id(1)),
            delegated_drep: Some(drep(1)),
        };
        stake_addresses.insert(stake_address.clone(), previous.clone());

        // Rewards are applied first at epoch boundary.
        stake_addresses.get_mut(&stake_address).unwrap().rewards += 25;
        recorder.record_reward_delta(&stake_address, -25);

        // Later in the same block, a withdrawal spends part of that balance.
        let after_withdrawal = StakeAddressState {
            rewards: 110,
            ..stake_addresses.get(&stake_address).unwrap()
        };
        recorder.record_change(
            &stake_address,
            stake_addresses.get(&stake_address).as_ref(),
            Some(&after_withdrawal),
        );
        stake_addresses.insert(stake_address.clone(), after_withdrawal);

        history.commit(14, recorder);
        history.rollback_to(14, &mut stake_addresses);

        assert_eq!(stake_addresses.get(&stake_address), Some(previous));
    }

    #[test]
    fn value_delta_clamps_large_ranges() {
        assert_eq!(value_delta(Some(u64::MAX), Some(0)), i64::MAX);
        assert_eq!(value_delta(Some(0), Some(u64::MAX)), i64::MIN);
    }

    #[tokio::test]
    async fn reward_runtime_clears_on_rollback() {
        let mut runtime = RewardRuntime::default();
        let (tx, _rx) = mpsc::channel();

        runtime.set_start_rewards_tx(tx);
        runtime.begin_epoch_registration_changes(4);
        runtime.set_epoch_rewards_task(tokio::spawn(async { Ok(RewardsResult::default()) }));

        runtime.clear_on_rollback();

        assert!(runtime.start_rewards_tx.is_none());
        assert!(runtime.current_epoch_registration_changes.is_none());
        assert!(runtime.epoch_rewards_task.is_none());
        assert!(runtime.active_epoch.is_none());
    }

    #[test]
    fn reward_runtime_keeps_same_epoch_work_and_rewinds_tracker() {
        let mut runtime = RewardRuntime::default();
        runtime.begin_epoch_registration_changes(10);
        runtime.push_registration_change(RegistrationChange {
            address: stake_address(1),
            kind: RegistrationChangeKind::Registered,
            epoch_slot: 11,
        });

        runtime.rollback_to(
            &block_info(10, 12, false),
            &[RegistrationChange {
                address: stake_address(2),
                kind: RegistrationChangeKind::Deregistered,
                epoch_slot: 9,
            }],
            8,
        );

        assert_eq!(runtime.active_epoch, Some(10));
        let changes =
            runtime.current_epoch_registration_changes.as_ref().unwrap().lock().unwrap().clone();
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].address, stake_address(2));
    }

    #[test]
    fn reward_runtime_clears_if_rollback_crosses_rewards_epoch_boundary() {
        let mut runtime = RewardRuntime::default();
        let (tx, _rx) = mpsc::channel();

        runtime.set_start_rewards_tx(tx);
        runtime.begin_epoch_registration_changes(10);

        runtime.rollback_to(&block_info(10, 0, true), &[], 8);

        assert!(runtime.start_rewards_tx.is_none());
        assert!(runtime.current_epoch_registration_changes.is_none());
        assert!(runtime.active_epoch.is_none());
    }

    #[tokio::test]
    async fn reward_runtime_clears_if_same_epoch_rollback_rewinds_to_rewards_capture_point() {
        let mut runtime = RewardRuntime::default();
        let (tx, _rx) = mpsc::channel();

        runtime.begin_epoch_registration_changes(10);
        runtime.set_epoch_rewards_task(tokio::spawn(async {
            std::future::pending::<anyhow::Result<RewardsResult>>().await
        }));
        runtime.set_start_rewards_tx(tx);
        runtime.notify_block(42, 8, 8);

        runtime.rollback_to(&block_info(10, 8, false), &[], 8);

        assert!(runtime.start_rewards_tx.is_none());
        assert!(runtime.current_epoch_registration_changes.is_none());
        assert!(runtime.epoch_rewards_task.is_none());
        assert!(runtime.active_epoch.is_none());
    }
}
