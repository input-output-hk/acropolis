//! Acropolis SPOState: State storage

use acropolis_common::{
    ledger_state::SPOState,
    messages::{
        CardanoMessage, EpochActivityMessage, Message, SPOStakeDistributionMessage,
        SPOStateMessage, TxCertificatesMessage,
    },
    params::{SECURITY_PARAMETER_K, TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH},
    serialization::SerializeMapAs,
    BlockInfo, KeyHash, PoolRegistration, PoolRetirement, TxCertificate,
};
use anyhow::Result;
use imbl::HashMap;
use serde_with::{hex::Hex, serde_as};
use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use tracing::{debug, error, info};

#[serde_as]
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockState {
    block: u64,

    epoch: u64,

    #[serde_as(as = "SerializeMapAs<Hex, _>")]
    spos: HashMap<Vec<u8>, PoolRegistration>,

    #[serde_as(as = "SerializeMapAs<_, Vec<Hex>>")]
    pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>,
}

impl BlockState {
    pub fn new(
        block: u64,
        epoch: u64,
        spos: HashMap<Vec<u8>, PoolRegistration>,
        pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>,
    ) -> Self {
        Self {
            block,
            epoch,
            spos,
            pending_deregistrations,
        }
    }
}

impl From<SPOState> for BlockState {
    fn from(value: SPOState) -> Self {
        Self {
            block: 0,
            epoch: 0,
            spos: value.pools.into(),
            pending_deregistrations: value.retiring.into_iter().fold(
                HashMap::new(),
                |mut acc, (key_hash, epoch)| {
                    acc.entry(epoch).or_insert_with(Vec::new).push(key_hash);
                    acc
                },
            ),
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

#[serde_as]
#[derive(Debug, Clone, serde::Serialize)]
pub struct ActiveStakesState {
    /// block number when Active Stakes is taken
    block: u64,
    /// epoch number when Active Stakes is taken (Epoch N + 1)
    epoch: u64,
    /// active stakes for each pool operator for each epoch (Until Epoch N)
    #[serde_as(as = "SerializeMapAs<_, SerializeMapAs<Hex, _>>")]
    active_stakes: HashMap<u64, HashMap<KeyHash, u64>>,
}

impl ActiveStakesState {
    pub fn new() -> Self {
        Self {
            block: 0,
            epoch: 0,
            active_stakes: HashMap::new(),
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, serde::Serialize)]
pub struct TotalBlocksMintedState {
    /// block number when Total Blocks Minted is taken
    block: u64,
    /// epoch number when Total Blocks Minted is taken (Epoch N + 1)
    epoch: u64,
    /// total blocks minted for each pool operator (Until Epoch N)
    /// KeyHash is VRF Vkey hash
    #[serde_as(as = "SerializeMapAs<Hex, _>")]
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
    history: VecDeque<BlockState>,

    /// Volatile active stakes state, one per epoch (in case new epoch block is rolled back)
    active_stakes_history: VecDeque<ActiveStakesState>,

    /// Volatile total blocks minted state, one per epoch (in case new epoch block is rolled back)
    total_blocks_minted_history: VecDeque<TotalBlocksMintedState>,
}

impl State {
    // Construct with optional publisher
    pub fn new() -> Self {
        Self {
            history: VecDeque::<BlockState>::new(),
            active_stakes_history: VecDeque::<ActiveStakesState>::new(),
            total_blocks_minted_history: VecDeque::<TotalBlocksMintedState>::new(),
        }
    }

    pub fn current(&self) -> Option<&BlockState> {
        self.history.back()
    }

    pub fn current_active_stakes_state(&self) -> Option<&ActiveStakesState> {
        self.active_stakes_history.back()
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
    /// `Option<(Vec<u64>, u64)>` - a vector of active stakes for each pool operator and the total active stake.
    pub fn get_pools_active_stakes(
        &self,
        pools_operators: &Vec<KeyHash>,
        epoch: u64,
    ) -> Option<(Vec<u64>, u64)> {
        let current = self.current_active_stakes_state()?;
        current.active_stakes.get(&epoch).map(|stakes| {
            let total_active_stake = stakes.values().sum();
            let pools_active_stakes =
                pools_operators.iter().map(|spo| stakes.get(spo).cloned().unwrap_or(0)).collect();
            (pools_active_stakes, total_active_stake)
        })
    }

    /// Get total blocks minted for each pool operator
    /// ## Arguments
    /// * `pools_operator` - A vector of pool operator hashes
    /// ## Returns
    /// `Option<Vec<u64>>` - a vector of total blocks minted for each pool operator.
    pub fn get_total_blocks_minted(&self, pools_operator: &Vec<KeyHash>) -> Option<Vec<u64>> {
        let current = self.current_total_blocks_minted_state()?;
        let total_blocks_minted = pools_operator
            .iter()
            .map(|spo| current.total_blocks_minted.get(spo).cloned().unwrap_or(0))
            .collect();
        Some(total_blocks_minted)
    }

    /// Get pools that will be retired in the upcoming epochs
    #[allow(dead_code)]
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

    fn get_previous_state(&mut self, block_number: u64) -> BlockState {
        loop {
            match self.history.back() {
                Some(state) => {
                    if state.block >= block_number {
                        info!("Rolling back state for block {}", state.block);
                        self.history.pop_back();
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        if let Some(current) = self.history.back() {
            current.clone()
        } else {
            BlockState::new(0, 0, HashMap::new(), HashMap::new())
        }
    }

    fn get_previous_active_stakes_state(&mut self, block_number: u64) -> ActiveStakesState {
        loop {
            match self.active_stakes_history.back() {
                Some(state) => {
                    if state.block >= block_number {
                        info!(
                            "Rolling back SPO active stakes state for block {}",
                            state.block
                        );
                        self.active_stakes_history.pop_back();
                    } else {
                        break;
                    }
                }
                _ => break,
            }
        }
        if let Some(current) = self.active_stakes_history.back() {
            current.clone()
        } else {
            ActiveStakesState::new()
        }
    }

    fn get_previous_total_blocks_minted_state(
        &mut self,
        block_number: u64,
    ) -> TotalBlocksMintedState {
        loop {
            match self.total_blocks_minted_history.back() {
                Some(state) => {
                    if state.block >= block_number {
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

    /// Returns a reference to the block state at a specified height, if applicable
    pub fn inspect_previous_state(&self, block_height: u64) -> Option<&BlockState> {
        for state in self.history.iter().rev() {
            if state.block == block_height {
                return Some(state);
            }
        }
        None
    }

    /// Handle TxCertificates with SPO registrations / de-registrations
    /// Returns an optional state message for end of epoch
    pub fn handle_tx_certs(
        &mut self,
        block: &BlockInfo,
        tx_certs_msg: &TxCertificatesMessage,
    ) -> Result<Option<Arc<Message>>> {
        let mut message: Option<Arc<Message>> = None;
        let mut current = self.get_previous_state(block.number);
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
                                hex::encode(&dr)
                            ),
                            _ => retired_spos.push(dr.clone()),
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

        // Prune and add to state history
        if self.history.len() >= SECURITY_PARAMETER_K as usize {
            self.history.pop_front();
        }
        self.history.push_back(current);

        Ok(message)
    }

    /// Handle SPO Stake Distribution
    /// Live stake snapshots taken before the first block of Epoch N
    /// Active stake is valid from Epoch N + 2
    ///
    pub fn handle_spdd(&mut self, block: &BlockInfo, spdd_message: &SPOStakeDistributionMessage) {
        info!(
            "Processing SPO Stake Distribution for epoch {} at block {}, epoch {}",
            spdd_message.epoch, block.number, block.epoch
        );
        let SPOStakeDistributionMessage { epoch, spos, .. } = spdd_message;
        let current = self.get_previous_active_stakes_state(block.number);
        let mut active_stakes = current.active_stakes.clone();
        active_stakes.insert(
            *epoch,
            HashMap::from_iter(spos.iter().map(|(key, value)| (key.clone(), value.active))),
        );

        let new_state = ActiveStakesState {
            block: block.number,
            epoch: block.epoch,
            active_stakes,
        };

        // Prune old history which can not be rolled back to
        if let Some(front) = self.active_stakes_history.front() {
            if current.block > front.block + SECURITY_PARAMETER_K as u64 {
                self.active_stakes_history.pop_front();
            }
        }
        self.active_stakes_history.push_back(new_state);
    }

    /// Handle Epoch Activity
    pub fn handle_epoch_activity(
        &mut self,
        block: &BlockInfo,
        epoch_activity_message: &EpochActivityMessage,
    ) {
        info!(
            "Processing Epoch Activity for epoch {} at block {}, epoch {}",
            epoch_activity_message.epoch, block.number, block.epoch
        );
        let EpochActivityMessage {
            vrf_vkey_hashes, ..
        } = epoch_activity_message;
        let current = self.get_previous_total_blocks_minted_state(block.number);
        let mut total_blocks_minted = current.total_blocks_minted.clone();

        vrf_vkey_hashes.iter().for_each(|(vrf_vkey_hash, amount)| {
            let Some(v) = total_blocks_minted.get_mut(vrf_vkey_hash) else {
                total_blocks_minted.insert(vrf_vkey_hash.clone(), *amount as u64);
                return;
            };
            *v += *amount as u64;
        });

        let new_state = TotalBlocksMintedState {
            block: block.number,
            epoch: block.epoch,
            total_blocks_minted,
        };

        // Prune old history which can not be rolled back to
        if let Some(front) = self.total_blocks_minted_history.front() {
            if current.block > front.block + SECURITY_PARAMETER_K as u64 {
                self.total_blocks_minted_history.pop_front();
            }
        }
        self.total_blocks_minted_history.push_back(new_state);
    }

    pub fn bootstrap(&mut self, state: SPOState) {
        self.history.clear();
        self.history.push_back(state.into());
    }

    pub fn dump(&self, block_height: u64) -> Option<SPOState> {
        self.inspect_previous_state(block_height).map(SPOState::from)
    }
}

// -- Tests --
#[cfg(test)]
pub mod tests {
    use super::*;
    use acropolis_common::{
        BlockInfo, BlockStatus, DelegatedStake, Era, PoolRetirement, Ratio, TxCertificate,
    };

    #[tokio::test]
    async fn new_state_is_empty() {
        let state = State::new();
        assert_eq!(0, state.history.len());
        assert_eq!(0, state.active_stakes_history.len());
    }

    #[tokio::test]
    async fn current_on_new_state_returns_none() {
        let state = State::new();
        assert!(state.current().is_none());
        assert!(state.current_active_stakes_state().is_none());
    }

    fn new_msg() -> TxCertificatesMessage {
        TxCertificatesMessage {
            certificates: Vec::<TxCertificate>::new(),
        }
    }

    fn new_spdd_message() -> SPOStakeDistributionMessage {
        SPOStakeDistributionMessage {
            spos: Vec::new(),
            epoch: 0,
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

    #[tokio::test]
    async fn state_is_not_empty_after_handle_tx_certs() {
        let mut state = State::new();
        let msg = new_msg();
        let block = new_block();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        assert_eq!(1, state.history.len());
    }

    #[tokio::test]
    async fn active_stakes_state_is_not_empty_after_handle_spdd() {
        let mut state = State::new();
        let msg = new_spdd_message();
        let block = new_block();
        state.handle_spdd(&block, &msg);
        assert_eq!(1, state.active_stakes_history.len());
    }

    #[tokio::test]
    async fn spo_gets_registered() {
        let mut state = State::new();
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
        let block = new_block();
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
        let mut state = State::new();
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 1,
        }));
        let block = new_block();
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
        let mut state = State::new();
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        let mut block = new_block();
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
        let mut state = State::new();
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        let mut block = new_block();
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
        let mut state = State::new();
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
        let mut block = new_block();
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
    async fn get_pools_active_stakes_returns_none_when_state_is_new() {
        let state = State::new();
        assert!(state.get_pools_active_stakes(&vec![vec![1], vec![2]], 0).is_none());
    }

    #[tokio::test]
    async fn get_pools_active_stakes_returns_none_when_epoch_is_not_found() {
        let mut state = State::new();
        let mut msg = new_spdd_message();
        msg.epoch = 1;
        let mut block = new_block();
        block.epoch = 1;
        state.handle_spdd(&block, &msg);
        assert!(state.get_pools_active_stakes(&vec![vec![1], vec![2]], 2).is_none());
    }

    #[tokio::test]
    async fn get_pools_active_stakes_returns_zero_when_active_stakes_not_found() {
        let mut state = State::new();
        let mut msg = new_spdd_message();
        msg.epoch = 1;
        let mut block = new_block();
        block.epoch = 1;
        state.handle_spdd(&block, &msg);
        let (active_stakes, total) =
            state.get_pools_active_stakes(&vec![vec![1], vec![2]], 1).unwrap();
        assert_eq!(2, active_stakes.len());
        assert_eq!(0, active_stakes[0]);
        assert_eq!(0, active_stakes[1]);
        assert_eq!(0, total);
    }

    #[tokio::test]
    async fn get_pools_active_stakes_returns_data() {
        let mut state = State::new();
        let mut msg = new_spdd_message();
        msg.spos = vec![
            (
                vec![1],
                DelegatedStake {
                    active: 10,
                    live: 10,
                },
            ),
            (
                vec![2],
                DelegatedStake {
                    active: 20,
                    live: 20,
                },
            ),
        ];
        msg.epoch = 1;
        let mut block = new_block();
        block.number = 1;
        block.epoch = 1;
        state.handle_spdd(&block, &msg);

        let (active_stakes, total) =
            state.get_pools_active_stakes(&vec![vec![1], vec![2]], 1).unwrap();
        assert_eq!(2, active_stakes.len());
        assert_eq!(10, active_stakes[0]);
        assert_eq!(20, active_stakes[1]);
        assert_eq!(30, total);
    }

    #[tokio::test]
    async fn spo_gets_restored_on_rollback() {
        let mut state = State::new();
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
        let mut block = new_block();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        println!("{}", serde_json::to_string_pretty(&state.history).unwrap());
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
        println!("{}", serde_json::to_string_pretty(&state.history).unwrap());
        let msg = new_msg();
        block.number = 2;
        block.epoch = 1;
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        println!("{}", serde_json::to_string_pretty(&state.history).unwrap());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert!(current.spos.is_empty());
        };
        let msg = new_msg();
        block.number = 2;
        block.epoch = 0;
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        println!("{}", serde_json::to_string_pretty(&state.history).unwrap());
        let current = state.current();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.spos.len());
            let spo = current.spos.get(&vec![0u8]);
            assert!(!spo.is_none());
        };
    }

    #[tokio::test]
    async fn active_stakes_get_restored_on_rollback() {
        let mut state = State::new();
        let mut msg = new_spdd_message();
        msg.spos = vec![
            (
                vec![1],
                DelegatedStake {
                    active: 10,
                    live: 10,
                },
            ),
            (
                vec![2],
                DelegatedStake {
                    active: 20,
                    live: 20,
                },
            ),
            (
                vec![3],
                DelegatedStake {
                    active: 30,
                    live: 30,
                },
            ),
        ];
        msg.epoch = 1;
        let mut block = new_block();
        block.number = 1;
        block.epoch = 1;
        state.handle_spdd(&block, &msg);
        println!(
            "{}",
            serde_json::to_string_pretty(&state.active_stakes_history).unwrap()
        );

        let current = state.current_active_stakes_state();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(1, current.active_stakes.len());
            assert_eq!(
                10,
                *current.active_stakes.get(&1).unwrap().get(&vec![1]).unwrap()
            );
            assert_eq!(
                20,
                *current.active_stakes.get(&1).unwrap().get(&vec![2]).unwrap()
            );
            assert_eq!(
                30,
                *current.active_stakes.get(&1).unwrap().get(&vec![3]).unwrap()
            );
        };

        msg.spos = vec![
            (
                vec![1],
                DelegatedStake {
                    active: 30,
                    live: 30,
                },
            ),
            (
                vec![2],
                DelegatedStake {
                    active: 40,
                    live: 40,
                },
            ),
        ];
        msg.epoch = 2;
        block.number = 11;
        block.epoch = 2;
        state.handle_spdd(&block, &msg);
        println!(
            "{}",
            serde_json::to_string_pretty(&state.active_stakes_history).unwrap()
        );

        let current = state.current_active_stakes_state();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(2, current.active_stakes.len());
            assert_eq!(
                30,
                *current.active_stakes.get(&2).unwrap().get(&vec![1]).unwrap()
            );
            assert_eq!(
                40,
                *current.active_stakes.get(&2).unwrap().get(&vec![2]).unwrap()
            );
        };

        let mut msg = new_spdd_message();
        msg.epoch = 3;
        block.number = 11;
        block.epoch = 3;
        state.handle_spdd(&block, &msg);
        println!(
            "{}",
            serde_json::to_string_pretty(&state.active_stakes_history).unwrap()
        );

        let current = state.current_active_stakes_state();
        assert!(!current.is_none());
        if let Some(current) = current {
            assert_eq!(2, current.active_stakes.len());
            assert_eq!(
                10,
                *current.active_stakes.get(&1).unwrap().get(&vec![1]).unwrap()
            );
            assert_eq!(
                20,
                *current.active_stakes.get(&1).unwrap().get(&vec![2]).unwrap()
            );
            assert_eq!(
                30,
                *current.active_stakes.get(&1).unwrap().get(&vec![3]).unwrap()
            );

            assert!(current.active_stakes.get(&3).unwrap().is_empty());
        };
    }

    #[tokio::test]
    async fn get_retiring_pools_returns_empty_when_state_is_new() {
        let state = State::new();
        assert!(state.get_retiring_pools().is_empty());
    }

    #[tokio::test]
    async fn get_retiring_pools_returns_pools() {
        let mut state = State::new();
        let mut msg = new_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        let mut block = new_block();
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
