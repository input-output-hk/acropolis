//! Acropolis SPOState: State storage

use acropolis_common::{
    ledger_state::SPOState,
    messages::{
        CardanoMessage, EpochActivityMessage, Message, SPORewardsMessage,
        SPOStakeDistributionMessage, SPOStateMessage, TxCertificatesMessage,
    },
    params::{SECURITY_PARAMETER_K, TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH},
    rational_number::RationalNumber,
    serialization::SerializeMapAs,
    state_history::StateHistory,
    BlockInfo, KeyHash, PoolEpochState, PoolRegistration, PoolRetirement, TxCertificate,
};
use anyhow::Result;
use dashmap::DashMap;
use imbl::HashMap;
use rayon::prelude::*;
use serde_with::{hex::Hex, serde_as};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::state_config::StateConfig;

#[serde_as]
#[derive(Default, Debug, Clone, serde::Serialize)]
pub struct BlockState {
    block: u64,

    epoch: u64,

    #[serde_as(as = "SerializeMapAs<Hex, _>")]
    spos: HashMap<Vec<u8>, PoolRegistration>,

    #[serde_as(as = "SerializeMapAs<_, Vec<Hex>>")]
    pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>,

    /// vrf_key_hash -> operator_hash mapping
    #[serde_as(as = "SerializeMapAs<Hex, Hex>")]
    vrf_key_hashes: HashMap<Vec<u8>, Vec<u8>>,
}

impl BlockState {
    pub fn new(
        block: u64,
        epoch: u64,
        spos: HashMap<Vec<u8>, PoolRegistration>,
        pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>,
        vrf_key_hashes: HashMap<Vec<u8>, Vec<u8>>,
    ) -> Self {
        Self {
            block,
            epoch,
            spos,
            pending_deregistrations,
            vrf_key_hashes,
        }
    }
}

impl From<SPOState> for BlockState {
    fn from(value: SPOState) -> Self {
        let spos: HashMap<KeyHash, PoolRegistration> = value.pools.into();
        let vrf_key_hashes =
            spos.iter().map(|(k, v)| (v.vrf_key_hash.clone(), k.clone())).collect();
        Self {
            block: 0,
            epoch: 0,
            spos,
            pending_deregistrations: value.retiring.into_iter().fold(
                HashMap::new(),
                |mut acc, (key_hash, epoch)| {
                    acc.entry(epoch).or_insert_with(Vec::new).push(key_hash);
                    acc
                },
            ),
            vrf_key_hashes,
        }
    }
}

// TODO: cleanup clones and into_iter, if possible
// It's not the end of the world here, as this is only used in testing, for now.
impl From<&BlockState> for SPOState {
    fn from(value: &BlockState) -> Self {
        Self {
            pools: value.spos.clone().into_iter().fold(BTreeMap::new(), |mut acc, (key, value)| {
                acc.insert(key, value);
                acc
            }),
            retiring: value.pending_deregistrations.clone().into_iter().fold(
                BTreeMap::new(),
                |mut acc, (epoch, key_hashes)| {
                    key_hashes.into_iter().for_each(|key_hash| {
                        acc.insert(key_hash, epoch);
                    });

                    acc
                },
            ),
        }
    }
}

/// Epoch State for certain pool
/// Store active_stake, delegators_count, rewards
///
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct EpochState {
    /// epoch number N
    epoch: u64,
    /// blocks minted during the epoch
    blocks_minted: Option<u64>,
    /// active stake of the epoch N (taken boundary from epoch N-2 to N-1)
    active_stake: Option<u64>,
    /// active size = active_stake / total_active_stake
    active_size: Option<RationalNumber>,
    /// delegators count by the end of the epoch
    delegators_count: Option<u64>,
    /// Total rewards pool has received during epoch
    pool_reward: Option<u64>,
    /// pool's operator's reward
    spo_reward: Option<u64>,
}

impl EpochState {
    fn new(epoch: u64) -> Self {
        Self {
            epoch,
            blocks_minted: None,
            active_stake: None,
            active_size: None,
            delegators_count: None,
            pool_reward: None,
            spo_reward: None,
        }
    }

    fn to_pool_epoch_state(&self) -> PoolEpochState {
        PoolEpochState {
            epoch: self.epoch,
            blocks_minted: self.blocks_minted.unwrap_or(0),
            active_stake: self.active_stake.unwrap_or(0),
            active_size: self.active_size.unwrap_or(RationalNumber::from(0)),
            delegators_count: self.delegators_count.unwrap_or(0),
            pool_reward: self.pool_reward.unwrap_or(0),
            spo_reward: self.spo_reward.unwrap_or(0),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TotalBlocksMintedState {
    /// block number of Epoch N
    block: u64,
    /// epoch number N
    epoch: u64,
    /// total blocks minted for each pool operator keyed by vrf_key_hash
    /// until the end of Epoch N-1
    total_blocks_minted: HashMap<KeyHash, u64>,
}

impl TotalBlocksMintedState {
    pub fn new() -> Self {
        Self {
            block: 0,
            epoch: 0,
            total_blocks_minted: HashMap::new(),
        }
    }
}

/// Overall module state
pub struct State {
    /// Volatile states, one per volatile block
    history: StateHistory<BlockState>,

    /// Epoch History for each pool operator
    epochs_history: Option<Arc<DashMap<KeyHash, BTreeMap<u64, EpochState>>>>,

    /// Active stakes for each pool operator
    /// (epoch number, active stake)
    /// Pop on first element when epoch number is greater than the epoch number
    pub active_stakes: DashMap<KeyHash, VecDeque<(u64, u64)>>,

    /// Volatile total blocks minted state, one per epoch
    /// Pop on first element when block number is smaller than `current block - SECURITY_PARAMETER_K`
    pub total_blocks_minted_history: VecDeque<TotalBlocksMintedState>,
}

impl State {
    // Construct with optional publisher
    pub fn new(state_config: StateConfig) -> Self {
        Self {
            history: StateHistory::new("spo-states/block-state"),
            epochs_history: if state_config.store_history {
                Some(Arc::new(DashMap::new()))
            } else {
                None
            },
            active_stakes: DashMap::new(),
            total_blocks_minted_history: VecDeque::new(),
        }
    }

    pub fn current(&self) -> Option<&BlockState> {
        self.history.current()
    }

    pub fn current_total_blocks_minted_state(&self) -> Option<&TotalBlocksMintedState> {
        self.total_blocks_minted_history.back()
    }

    #[allow(dead_code)]
    pub fn get(&self, operator: &KeyHash) -> Option<&PoolRegistration> {
        if let Some(current) = self.current() {
            current.spos.get(operator)
        } else {
            None
        }
    }

    /// Get SPO from vrf_key_hash
    pub fn get_spo_from_vrf_key_hash(&self, vrf_key_hash: &KeyHash) -> Option<KeyHash> {
        self.current().and_then(|state| state.vrf_key_hashes.get(vrf_key_hash).cloned())
    }

    /// Get Epoch State for certain pool operator at certain epoch
    #[allow(dead_code)]
    pub fn get_epoch_state(&self, spo: &KeyHash, epoch: u64) -> Option<EpochState> {
        self.epochs_history.as_ref().and_then(|epochs_history| {
            epochs_history.get(spo).and_then(|epochs| epochs.get(&epoch).cloned())
        })
    }

    /// Get all Stake Pool operators' operator hashes
    pub fn list_pool_operators(&self) -> Vec<KeyHash> {
        self.current().map(|state| state.spos.keys().cloned().collect()).unwrap_or_default()
    }

    /// Get all Stake Pool Operators' operator hashes and their registration information
    pub fn list_pools_with_info(&self) -> Vec<(KeyHash, PoolRegistration)> {
        self.current()
            .map(|state| state.spos.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default()
    }

    /// Get Pools Active Stakes by epoch and total active stake
    /// ## Arguments
    /// * `pools_operators` - A vector of pool operator hashes
    /// * `epoch` - The epoch to get the active stakes for
    /// ## Returns
    /// `(Vec<u64>, u64)` - a vector of active stakes for each pool operator and the total active stake.
    pub fn get_pools_active_stakes(
        &self,
        pools_operators: &Vec<KeyHash>,
        epoch: u64,
    ) -> (Vec<u64>, u64) {
        let active_stakes = pools_operators
            .par_iter()
            .map(|spo| self.get_active_stake(spo, epoch).unwrap_or(0))
            .collect::<Vec<u64>>();
        let total_active_stake = active_stakes.iter().sum();
        (active_stakes, total_active_stake)
    }

    fn get_active_stake(&self, spo: &KeyHash, epoch: u64) -> Option<u64> {
        self.active_stakes
            .get(spo)
            .map(|stakes| stakes.iter().find(|(e, _)| *e == epoch).map(|(_, stake)| *stake))
            .flatten()
    }

    /// Get total blocks minted for each vrf vkey hash
    /// ## Arguments
    /// * `vrf_vkey_hashes` - A vector of vrf vkey hashes
    /// ## Returns
    /// `Vec<u64>` - a vector of total blocks minted for each vrf vkey hash.
    pub fn get_total_blocks_minted(&self, vrf_vkey_hashes: &Vec<KeyHash>) -> Vec<u64> {
        let Some(current) = self.current_total_blocks_minted_state() else {
            return vec![0; vrf_vkey_hashes.len()];
        };
        let total_blocks_minted = vrf_vkey_hashes
            .iter()
            .map(|vrf_vkey_hash| {
                current.total_blocks_minted.get(vrf_vkey_hash).cloned().unwrap_or(0)
            })
            .collect();
        total_blocks_minted
    }

    /// Get pools that will be retired in the upcoming epochs
    pub fn get_retiring_pools(&self) -> Vec<PoolRetirement> {
        self.current().map_or(Vec::new(), |state: &BlockState| {
            let current_epoch = state.epoch;
            state
                .pending_deregistrations
                .iter()
                .filter(|(&epoch, _)| epoch > current_epoch)
                .flat_map(|(&epoch, retiring_operators)| {
                    retiring_operators.iter().map(move |operator| PoolRetirement {
                        operator: operator.clone(),
                        epoch,
                    })
                })
                .collect()
        })
    }

    /// Get pools that have been retired so far
    pub fn get_retired_pools(&self) -> Vec<PoolRetirement> {
        vec![]
    }

    /// Get Pool History by SPO
    pub fn get_pool_history(&self, spo: &KeyHash) -> Option<Vec<PoolEpochState>> {
        self.epochs_history.as_ref().and_then(|epochs_history| {
            epochs_history
                .get(spo)
                .map(|epochs| epochs.values().map(|state| state.to_pool_epoch_state()).collect())
        })
    }

    async fn log_stats(&self) {
        if let Some(current) = self.current() {
            info!(
                num_spos = current.spos.keys().len(),
                num_pending_deregistrations =
                    current.pending_deregistrations.values().map(|d| d.len()).sum::<usize>(),
            );
        } else {
            info!(num_spos = 0, num_pending_deregistrations = 0);
        }
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats().await;
        Ok(())
    }

    fn get_previous_total_blocks_minted_state(
        &mut self,
        block: &BlockInfo,
    ) -> TotalBlocksMintedState {
        loop {
            match self.total_blocks_minted_history.back() {
                Some(state) => {
                    if state.block >= block.number || state.epoch >= block.epoch {
                        info!(
                            "Rolling back SPO total blocks minted state for block {}",
                            state.block
                        );
                        self.total_blocks_minted_history.pop_back();
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        if let Some(current) = self.total_blocks_minted_history.back() {
            current.clone()
        } else {
            TotalBlocksMintedState::new()
        }
    }

    /// Handle TxCertificates with SPO registrations / de-registrations
    /// Returns an optional state message for end of epoch
    pub fn handle_tx_certs(
        &mut self,
        block: &BlockInfo,
        tx_certs_msg: &TxCertificatesMessage,
    ) -> Result<Option<Arc<Message>>> {
        let mut message: Option<Arc<Message>> = None;
        let mut current = self.history.get_rolled_back_state(block);
        current.block = block.number;

        // Handle end of epoch
        if block.epoch > current.epoch {
            current.epoch = block.epoch;

            debug!(epoch = current.epoch, "New epoch");

            // Flatten into vector of registrations, before retirement so retiring ones
            // are still included
            let spos = current.spos.values().cloned().collect();

            // Deregister any pending
            let mut retired_spos: Vec<KeyHash> = Vec::new();
            let deregistrations = current.pending_deregistrations.remove(&current.epoch);
            match deregistrations {
                Some(deregistrations) => {
                    for dr in deregistrations {
                        debug!("Retiring SPO {}", hex::encode(&dr));
                        match current.spos.remove(&dr) {
                            None => error!(
                                "Retirement requested for unregistered SPO {}",
                                hex::encode(&dr),
                            ),
                            Some(de_reg) => {
                                retired_spos.push(dr.clone());
                                current.vrf_key_hashes.remove(&de_reg.vrf_key_hash);
                            }
                        };
                    }
                }
                None => (),
            };

            message = Some(Arc::new(Message::Cardano((
                block.clone(),
                CardanoMessage::SPOState(SPOStateMessage {
                    epoch: block.epoch - 1,
                    spos,
                    retired_spos,
                }),
            ))));
        }

        // Handle certificates
        for tx_cert in tx_certs_msg.certificates.iter() {
            match tx_cert {
                TxCertificate::PoolRegistration(reg) => {
                    debug!(
                        block = block.number,
                        "Registering SPO {}",
                        hex::encode(&reg.operator)
                    );
                    current.spos.insert(reg.operator.clone(), reg.clone());
                    current.vrf_key_hashes.insert(reg.vrf_key_hash.clone(), reg.operator.clone());

                    // Remove any existing queued deregistrations
                    for (epoch, deregistrations) in &mut current.pending_deregistrations.iter_mut()
                    {
                        let old_len = deregistrations.len();
                        deregistrations.retain(|d| *d != reg.operator);
                        if deregistrations.len() != old_len {
                            debug!(
                                "Removed pending deregistration of SPO {} from epoch {}",
                                hex::encode(&reg.operator),
                                epoch
                            );
                        }
                    }
                }
                TxCertificate::PoolRetirement(ret) => {
                    debug!(
                        "SPO {} wants to retire at the end of epoch {} (cert in block number {})",
                        hex::encode(&ret.operator),
                        ret.epoch,
                        block.number
                    );
                    if ret.epoch <= current.epoch {
                        error!(
                            "SPO retirement received for current or past epoch {} for SPO {}",
                            ret.epoch,
                            hex::encode(&ret.operator)
                        );
                    } else if ret.epoch > current.epoch + TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH
                    {
                        error!("SPO retirement received for epoch {} that exceeds future limit for SPO {}", ret.epoch, hex::encode(&ret.operator));
                    } else {
                        // Replace any existing queued deregistrations
                        for (epoch, deregistrations) in
                            &mut current.pending_deregistrations.iter_mut()
                        {
                            let old_len = deregistrations.len();
                            deregistrations.retain(|d| *d != ret.operator);
                            if deregistrations.len() != old_len {
                                debug!(
                                    "Replaced pending deregistration of SPO {} from epoch {}",
                                    hex::encode(&ret.operator),
                                    epoch
                                );
                            }
                        }
                        current
                            .pending_deregistrations
                            .entry(ret.epoch)
                            .or_default()
                            .push(ret.operator.clone());
                    }
                }
                _ => (),
            }
        }

        // Commit the new state
        self.history.commit(block, current);

        Ok(message)
    }

    /// Handle SPO Stake Distribution
    /// Live stake snapshots taken at Epoch N - 1 to N boundary (Mark at Epoch N)
    /// Active stake is valid from Epoch N + 1 (Set at Epoch N + 1)
    ///
    pub fn handle_spdd(&mut self, block: &BlockInfo, spdd_message: &SPOStakeDistributionMessage) {
        let SPOStakeDistributionMessage { epoch, spos } = spdd_message;
        if *epoch != block.epoch - 1 {
            error!(
                "SPO Stake Distribution Message's epoch {} is wrong against current block's epoch {}",
                *epoch, block.epoch
            )
        }

        let total_active_stake: u64 = spos.par_iter().map(|(_, value)| value.active).sum();

        // update active stakes
        spos.par_iter().for_each(|(spo, value)| {
            let mut active_stakes =
                self.active_stakes.entry(spo.clone()).or_insert_with(VecDeque::new);

            // pop active stake of epoch which is less than current epoch
            loop {
                let Some((front_epoch, _)) = active_stakes.front() else {
                    break;
                };
                if *front_epoch < block.epoch {
                    active_stakes.pop_front();
                } else {
                    break;
                }
            }
            active_stakes.push_back((*epoch + 2, value.active));
        });

        // update epochs history if set
        if let Some(epochs_history) = self.epochs_history.as_ref() {
            spos.par_iter().for_each(|(spo, value)| {
                Self::update_epochs_history_with(epochs_history, spo, *epoch + 2, |epoch_state| {
                    epoch_state.active_stake = Some(value.active);
                    epoch_state.delegators_count = Some(value.active_delegators_count);
                    if total_active_stake > 0 {
                        epoch_state.active_size =
                            Some(RationalNumber::new(value.active, total_active_stake));
                    }
                });
            });
        }
    }

    /// Handle Epoch Activity
    pub fn handle_epoch_activity(
        &mut self,
        block: &BlockInfo,
        epoch_activity_message: &EpochActivityMessage,
    ) {
        let EpochActivityMessage {
            epoch,
            vrf_vkey_hashes,
            ..
        } = epoch_activity_message;
        if *epoch != block.epoch - 1 {
            error!(
                "Epoch Activity Message's epoch {} is wrong against current block's epoch {}",
                *epoch, block.epoch
            )
        }

        let current_total_blocks_minted_state = self.get_previous_total_blocks_minted_state(block);
        let mut total_blocks_minted = current_total_blocks_minted_state.total_blocks_minted.clone();

        // handle blocks_minted state
        vrf_vkey_hashes.iter().for_each(|(vrf_vkey_hash, amount)| {
            *(total_blocks_minted.entry(vrf_vkey_hash.clone()).or_insert(0)) += *amount as u64;
        });

        // update epochs history if set
        if let Some(epochs_history) = self.epochs_history.as_ref() {
            vrf_vkey_hashes.iter().for_each(|(vrf_vkey_hash, amount)| {
                let spo = self.get_spo_from_vrf_key_hash(vrf_vkey_hash);
                if let Some(spo) = spo {
                    Self::update_epochs_history_with(epochs_history, &spo, *epoch, |epoch_state| {
                        epoch_state.blocks_minted = Some(*amount as u64);
                    });
                }
            })
        }

        let new_state = TotalBlocksMintedState {
            block: block.number,
            epoch: block.epoch,
            total_blocks_minted,
        };

        // Prune old history which can not be rolled back to
        if let Some(front) = self.total_blocks_minted_history.front() {
            if current_total_blocks_minted_state.block > front.block + SECURITY_PARAMETER_K as u64 {
                self.total_blocks_minted_history.pop_front();
            }
        }
        self.total_blocks_minted_history.push_back(new_state);
    }

    /// Handle SPO rewards data calculated from accounts-state
    /// NOTE:
    /// The calculated result is one epoch off against blockfrost's response.
    pub fn handle_spo_rewards(
        &mut self,
        block: &BlockInfo,
        spo_rewards_message: &SPORewardsMessage,
    ) {
        let SPORewardsMessage { epoch, spos } = spo_rewards_message;
        if *epoch != block.epoch - 1 {
            error!(
                "SPO Rewards Message's epoch {} is wrong against current block's epoch {}",
                *epoch, block.epoch
            )
        }

        // update epochs history if set
        if let Some(epochs_history) = self.epochs_history.as_ref() {
            spos.par_iter().for_each(|(spo, value)| {
                Self::update_epochs_history_with(epochs_history, spo, *epoch, |epoch_state| {
                    epoch_state.pool_reward = Some(value.total_rewards);
                    epoch_state.spo_reward = Some(value.operator_rewards);
                });
            });
        }
    }

    pub fn bootstrap(&mut self, state: SPOState) {
        self.history.clear();
        self.history.commit_forced(state.into());
    }

    pub fn dump(&self, block_height: u64) -> Option<SPOState> {
        self.history.inspect_previous_state(block_height).map(SPOState::from)
    }

    fn update_epochs_history_with(
        epochs_history: &Arc<DashMap<KeyHash, BTreeMap<u64, EpochState>>>,
        spo: &KeyHash,
        epoch: u64,
        update_fn: impl FnOnce(&mut EpochState),
    ) {
        let mut epochs = epochs_history.entry(spo.clone()).or_insert_with(|| BTreeMap::new());
        let epoch_state = epochs.entry(epoch).or_insert_with(|| EpochState::new(epoch));
        update_fn(epoch_state);
    }
}

// -- Tests --
#[cfg(test)]
pub mod tests {
    use crate::state_config::StateConfig;

    use super::*;
    use acropolis_common::{
        BlockInfo, BlockStatus, DelegatedStake, Era, PoolRetirement, Ratio, SPORewards,
        TxCertificate,
    };

    fn default_state_config() -> StateConfig {
        StateConfig {
            store_history: false,
            store_retired_pools: false,
        }
    }

    fn save_history_state_config() -> StateConfig {
        StateConfig {
            store_history: true,
            store_retired_pools: false,
        }
    }

    fn save_all_state_config() -> StateConfig {
        StateConfig {
            store_history: true,
            store_retired_pools: true,
        }
    }

    #[tokio::test]
    async fn new_state_is_empty() {
        let state = State::new(default_state_config());
        assert_eq!(0, state.history.len());
        assert_eq!(0, state.active_stakes.len());
        assert!(state.epochs_history.is_none());

        let state_with_history = State::new(StateConfig::new(true, true));
        assert_eq!(0, state_with_history.history.len());
        assert_eq!(0, state_with_history.active_stakes.len());
        assert_eq!(0, state_with_history.epochs_history.unwrap().len());
    }

    #[tokio::test]
    async fn current_on_new_state_returns_none() {
        let state = State::new(StateConfig::new(false, false));
        assert!(state.current().is_none());
    }

    fn new_msg() -> TxCertificatesMessage {
        TxCertificatesMessage {
            certificates: Vec::<TxCertificate>::new(),
        }
    }

    fn new_spdd_message(epoch: u64) -> SPOStakeDistributionMessage {
        SPOStakeDistributionMessage {
            spos: Vec::new(),
            epoch,
        }
    }

    fn new_epoch_activity_message(epoch: u64) -> EpochActivityMessage {
        EpochActivityMessage {
            epoch,
            total_blocks: 0,
            total_fees: 0,
            vrf_vkey_hashes: Vec::new(),
        }
    }

    fn new_spo_rewards_message(epoch: u64) -> SPORewardsMessage {
        SPORewardsMessage {
            spos: Vec::new(),
            epoch,
        }
    }

    fn new_block(epoch: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            slot: 0,
            number: 10 * epoch,
            hash: Vec::<u8>::new(),
            epoch,
            new_epoch: true,
            era: Era::Byron,
        }
    }

    #[tokio::test]
    async fn state_is_not_empty_after_handle_tx_certs() {
        let mut state = State::new(default_state_config());
        let msg = new_msg();
        let block = new_block(1);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        assert_eq!(1, state.history.len());
    }

    #[tokio::test]
    async fn active_stakes_is_not_empty_after_handle_spdd() {
        let mut state = State::new(save_all_state_config());
        let mut block = new_block(2);
        let mut msg = new_spdd_message(1);
        msg.spos = vec![(
            vec![1],
            DelegatedStake {
                active: 1,
                active_delegators_count: 1,
                live: 1,
            },
        )];
        state.handle_spdd(&block, &msg);
        assert_eq!(1, state.active_stakes.len());
        assert_eq!(
            VecDeque::from(vec![(3, 1)]),
            state.active_stakes.get(&vec![1]).unwrap().clone()
        );

        block = new_block(3);
        msg = new_spdd_message(2);
        msg.spos = vec![(
            vec![1],
            DelegatedStake {
                active: 2,
                active_delegators_count: 1,
                live: 1,
            },
        )];
        state.handle_spdd(&block, &msg);

        assert_eq!(1, state.active_stakes.len());
        assert_eq!(
            VecDeque::from(vec![(3, 1), (4, 2)]),
            state.active_stakes.get(&vec![1]).unwrap().clone()
        );

        block = new_block(4);
        msg = new_spdd_message(3);
        msg.spos = vec![(
            vec![1],
            DelegatedStake {
                active: 3,
                active_delegators_count: 1,
                live: 1,
            },
        )];
        state.handle_spdd(&block, &msg);

        assert_eq!(1, state.active_stakes.len());
        assert_eq!(
            VecDeque::from(vec![(4, 2), (5, 3)]),
            state.active_stakes.get(&vec![1]).unwrap().clone()
        );
    }

    #[tokio::test]
    async fn get_total_blocks_minted_returns_zero_when_state_is_new() {
        let state = State::new(default_state_config());
        let total_blocks_minted = state.get_total_blocks_minted(&vec![vec![1], vec![2]]);
        assert_eq!(2, total_blocks_minted.len());
        assert_eq!(0, total_blocks_minted[0]);
        assert_eq!(0, total_blocks_minted[1]);
    }

    #[tokio::test]
    async fn get_total_blocks_minted_returns_data_after_handle_epoch_activity() {
        let mut state = State::new(default_state_config());
        let block = new_block(2);
        let mut epoch_activity_message = new_epoch_activity_message(1);
        epoch_activity_message.vrf_vkey_hashes = vec![(vec![2], 1)];
        epoch_activity_message.total_blocks = 1;
        epoch_activity_message.total_fees = 10;
        state.handle_epoch_activity(&block, &epoch_activity_message);

        assert_eq!(1, state.total_blocks_minted_history.len());
        assert_eq!(
            vec![1, 0],
            state.get_total_blocks_minted(&vec![vec![2], vec![3]])
        );
    }

    #[tokio::test]
    async fn total_blocks_minted_history_is_pruned_after_rollback() {
        let mut state = State::new(default_state_config());
        let mut block = new_block(2);
        let mut epoch_activity_message = new_epoch_activity_message(1);
        epoch_activity_message.vrf_vkey_hashes = vec![(vec![2], 1)];
        epoch_activity_message.total_blocks = 1;
        epoch_activity_message.total_fees = 10;
        state.handle_epoch_activity(&block, &epoch_activity_message);

        block = new_block(3);
        epoch_activity_message = new_epoch_activity_message(2);
        epoch_activity_message.vrf_vkey_hashes = vec![(vec![2], 2), (vec![3], 3)];
        epoch_activity_message.total_blocks = 5;
        epoch_activity_message.total_fees = 50;
        state.handle_epoch_activity(&block, &epoch_activity_message);

        assert_eq!(2, state.total_blocks_minted_history.len());
        assert_eq!(
            vec![3, 3],
            state.get_total_blocks_minted(&vec![vec![2], vec![3]])
        );

        // roll back here
        epoch_activity_message = new_epoch_activity_message(2);
        epoch_activity_message.vrf_vkey_hashes = vec![(vec![2], 2)];
        epoch_activity_message.total_blocks = 2;
        epoch_activity_message.total_fees = 20;
        state.handle_epoch_activity(&block, &epoch_activity_message);

        assert_eq!(2, state.total_blocks_minted_history.len());
        assert_eq!(
            vec![3, 0],
            state.get_total_blocks_minted(&vec![vec![2], vec![3]])
        );
    }

    #[tokio::test]
    async fn get_pool_history_returns_none_when_store_history_disabled() {
        let state = State::new(default_state_config());
        let pool_history = state.get_pool_history(&vec![1]);
        assert!(pool_history.is_none());
    }

    #[tokio::test]
    async fn get_pool_history_returns_none_when_spo_is_not_found() {
        let state = State::new(save_history_state_config());
        let pool_history = state.get_pool_history(&vec![1]);
        assert!(pool_history.is_none());
    }

    #[tokio::test]
    async fn get_pool_history_returns_data() {
        let mut state = State::new(save_history_state_config());

        let mut block = new_block(1);
        let mut cert_msg = new_msg();
        cert_msg.certificates.push(TxCertificate::PoolRegistration(PoolRegistration {
            operator: vec![1],
            vrf_key_hash: vec![11],
            pledge: 0,
            cost: 0,
            margin: Ratio {
                numerator: 0,
                denominator: 1,
            },
            reward_account: vec![0],
            pool_owners: vec![vec![0]],
            relays: vec![],
            pool_metadata: None,
        }));
        assert!(state.handle_tx_certs(&block, &cert_msg).is_ok());

        block = new_block(2);
        let mut spdd_msg = new_spdd_message(1);
        spdd_msg.spos = vec![(
            vec![1],
            DelegatedStake {
                active: 1,
                active_delegators_count: 1,
                live: 1,
            },
        )];
        state.handle_spdd(&block, &spdd_msg);

        let mut epoch_activity_msg = new_epoch_activity_message(1);
        epoch_activity_msg.vrf_vkey_hashes = vec![(vec![11], 1)];
        epoch_activity_msg.total_blocks = 1;
        epoch_activity_msg.total_fees = 10;
        state.handle_epoch_activity(&block, &epoch_activity_msg);

        let mut spo_rewards_msg = new_spo_rewards_message(1);
        spo_rewards_msg.spos = vec![(
            vec![1],
            SPORewards {
                total_rewards: 100,
                operator_rewards: 10,
            },
        )];
        state.handle_spo_rewards(&block, &spo_rewards_msg);

        let pool_history = state.get_pool_history(&vec![1]).unwrap();
        assert_eq!(2, pool_history.len());
        let first_epoch = pool_history.get(0).unwrap();
        let third_epoch = pool_history.get(1).unwrap();
        assert_eq!(1, first_epoch.epoch);
        assert_eq!(1, first_epoch.blocks_minted);
        assert_eq!(3, third_epoch.epoch);
        assert_eq!(1, third_epoch.active_stake);
        assert_eq!(RationalNumber::new(1, 1), third_epoch.active_size);
        assert_eq!(1, third_epoch.delegators_count);
        assert_eq!(100, first_epoch.pool_reward);
        assert_eq!(10, first_epoch.spo_reward);
    }

    #[tokio::test]
    async fn spo_gets_registered() {
        let mut state = State::new(default_state_config());
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRegistration(PoolRegistration {
            operator: vec![0],
            vrf_key_hash: vec![0],
            pledge: 0,
            cost: 0,
            margin: Ratio {
                numerator: 0,
                denominator: 0,
            },
            reward_account: vec![0],
            pool_owners: vec![vec![0]],
            relays: vec![],
            pool_metadata: None,
        }));
        let block = new_block(1);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.spos.len());
            let spo = current.spos.get(&vec![0u8]);
            assert!(!spo.is_none());
        };
    }

    #[tokio::test]
    async fn pending_deregistration_gets_queued() {
        let mut state = State::new(default_state_config());
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 1,
        }));
        let block = new_block(0);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.pending_deregistrations.len());
            let drs = current.pending_deregistrations.get(&1);
            assert!(!drs.is_none());
            if let Some(drs) = drs {
                assert_eq!(1, drs.len());
                assert!(drs.contains(&vec![0u8]));
            }
        };
    }

    #[tokio::test]
    async fn second_pending_deregistration_gets_queued() {
        let mut state = State::new(default_state_config());
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        let mut block = new_block(0);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let mut msg = new_msg();
        block.number = 1;
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![1],
            epoch: 2,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.pending_deregistrations.len());
            let drs = current.pending_deregistrations.get(&2);
            assert!(!drs.is_none());
            if let Some(drs) = drs {
                assert_eq!(2, drs.len());
                assert!(drs.contains(&vec![0u8]));
                assert!(drs.contains(&vec![1u8]));
            }
        };
    }

    #[tokio::test]
    async fn rollback_removes_second_pending_deregistration() {
        let mut state = State::new(default_state_config());
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        let mut block = new_block(0);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let mut msg = new_msg();
        block.number = 1;
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![1],
            epoch: 2,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let msg = new_msg();
        block.number = 1;
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.pending_deregistrations.len());
            let drs = current.pending_deregistrations.get(&2);
            assert!(!drs.is_none());
            if let Some(drs) = drs {
                assert_eq!(1, drs.len());
                assert!(drs.contains(&vec![0u8]));
            }
        };
    }

    #[tokio::test]
    async fn spo_gets_deregistered() {
        let mut state = State::new(default_state_config());
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRegistration(PoolRegistration {
            operator: vec![0],
            vrf_key_hash: vec![0],
            pledge: 0,
            cost: 0,
            margin: Ratio {
                numerator: 0,
                denominator: 0,
            },
            reward_account: vec![0],
            pool_owners: vec![vec![0]],
            relays: vec![],
            pool_metadata: None,
        }));
        let mut block = new_block(0);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.spos.len());
            let spo = current.spos.get(&vec![0u8]);
            assert!(!spo.is_none());
        };
        let mut msg = new_msg();
        block.number = 1;
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 1,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let msg = new_msg();
        block.number = 2;
        block.epoch = 1; // SPO get retired at the start of the epoch it requests
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert!(current.spos.is_empty());
        };
    }

    #[tokio::test]
    async fn get_pools_active_stakes_returns_zero_when_state_is_new() {
        let state = State::new(default_state_config());
        let (active_stakes, total) = state.get_pools_active_stakes(&vec![vec![1], vec![2]], 0);
        assert_eq!(2, active_stakes.len());
        assert_eq!(0, active_stakes[0]);
        assert_eq!(0, active_stakes[1]);
        assert_eq!(0, total);
    }

    #[tokio::test]
    async fn get_pools_active_stakes_returns_zero_when_epoch_is_not_found() {
        let mut state = State::new(default_state_config());
        let block = new_block(2);
        let msg = new_spdd_message(1);
        state.handle_spdd(&block, &msg);
        let (active_stakes, total) =
            state.get_pools_active_stakes(&vec![vec![1], vec![2]], block.epoch);
        assert_eq!(2, active_stakes.len());
        assert_eq!(0, active_stakes[0]);
        assert_eq!(0, active_stakes[1]);
        assert_eq!(0, total);
    }

    #[tokio::test]
    async fn get_pools_active_stakes_returns_zero_when_active_stakes_not_found() {
        let mut state = State::new(default_state_config());
        let block = new_block(2);
        let msg = new_spdd_message(1);
        state.handle_spdd(&block, &msg);
        let (active_stakes, total) =
            state.get_pools_active_stakes(&vec![vec![1], vec![2]], block.epoch + 1);
        assert_eq!(2, active_stakes.len());
        assert_eq!(0, active_stakes[0]);
        assert_eq!(0, active_stakes[1]);
        assert_eq!(0, total);
    }

    #[tokio::test]
    async fn get_pools_active_stakes_returns_data() {
        let mut state = State::new(default_state_config());
        let block = new_block(2);
        let mut msg = new_spdd_message(1);
        msg.spos = vec![
            (
                vec![1],
                DelegatedStake {
                    active: 10,
                    active_delegators_count: 1,
                    live: 10,
                },
            ),
            (
                vec![2],
                DelegatedStake {
                    active: 20,
                    active_delegators_count: 1,
                    live: 20,
                },
            ),
        ];
        state.handle_spdd(&block, &msg);

        let (active_stakes, total) =
            state.get_pools_active_stakes(&vec![vec![1], vec![2]], block.epoch + 1);
        assert_eq!(2, active_stakes.len());
        assert_eq!(10, active_stakes[0]);
        assert_eq!(20, active_stakes[1]);
        assert_eq!(30, total);
    }

    #[tokio::test]
    async fn spo_gets_restored_on_rollback() {
        let mut state = State::new(default_state_config());
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRegistration(PoolRegistration {
            operator: vec![0],
            vrf_key_hash: vec![0],
            pledge: 0,
            cost: 0,
            margin: Ratio {
                numerator: 0,
                denominator: 0,
            },
            reward_account: vec![0],
            pool_owners: vec![vec![0]],
            relays: vec![],
            pool_metadata: None,
        }));
        let mut block = new_block(0);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        println!(
            "{}",
            serde_json::to_string_pretty(&state.history.values()).unwrap()
        );
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.spos.len());
            let spo = current.spos.get(&vec![0u8]);
            assert!(!spo.is_none());
        };
        let mut msg = new_msg();
        block.number = 1;
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 1,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        println!(
            "{}",
            serde_json::to_string_pretty(&state.history.values()).unwrap()
        );
        let msg = new_msg();
        block.number = 2;
        block.epoch = 1;
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        println!(
            "{}",
            serde_json::to_string_pretty(&state.history.values()).unwrap()
        );
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert!(current.spos.is_empty());
        };
        let msg = new_msg();
        block.number = 2;
        block.epoch = 0;
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        println!(
            "{}",
            serde_json::to_string_pretty(&state.history.values()).unwrap()
        );
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.spos.len());
            let spo = current.spos.get(&vec![0u8]);
            assert!(!spo.is_none());
        };
    }

    #[tokio::test]
    async fn get_retiring_pools_returns_empty_when_state_is_new() {
        let state = State::new(default_state_config());
        assert!(state.get_retiring_pools().is_empty());
    }

    #[tokio::test]
    async fn get_retiring_pools_returns_pools() {
        let mut state = State::new(default_state_config());
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        let mut block = new_block(0);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let mut msg = new_msg();
        block.number = 1;
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![1],
            epoch: 3,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let current = state.current();
        assert!(!current.is_none());
        let mut retiring_pools = state.get_retiring_pools();
        retiring_pools.sort_by_key(|p| p.epoch);
        assert_eq!(2, retiring_pools.len());
        assert_eq!(vec![0], retiring_pools[0].operator);
        assert_eq!(2, retiring_pools[0].epoch);
        assert_eq!(vec![1], retiring_pools[1].operator);
        assert_eq!(3, retiring_pools[1].epoch);
    }
}
