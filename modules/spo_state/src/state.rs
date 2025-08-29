//! Acropolis SPOState: State storage

use acropolis_common::{
    ledger_state::SPOState,
    messages::{
        CardanoMessage, EpochActivityMessage, Message, SPOStakeDistributionMessage,
        SPOStateMessage, TxCertificatesMessage,
    },
    params::TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH,
    serialization::SerializeMapAs,
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, KeyHash, PoolMetadata, PoolRegistration, PoolRetirement, TxCertificate,
};
use anyhow::Result;
use dashmap::DashMap;
use imbl::HashMap;
use rayon::prelude::*;
use serde_with::{hex::Hex, serde_as};
use std::collections::BTreeMap;
use std::sync::Arc;
use tracing::{debug, error, info};

#[serde_as]
#[derive(Default, Debug, Clone, serde::Serialize)]
pub struct BlockState {
    block: u64,

    epoch: u64,

    #[serde_as(as = "SerializeMapAs<Hex, _>")]
    spos: HashMap<Vec<u8>, PoolRegistration>,

    #[serde_as(as = "SerializeMapAs<_, Vec<Hex>>")]
    pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>,

    /// vrf_key_hash -> pool_id mapping
    #[serde_as(as = "SerializeMapAs<Hex, Hex>")]
    vrf_key_to_pool_id_map: HashMap<Vec<u8>, Vec<u8>>,
}

impl BlockState {
    pub fn new(
        block: u64,
        epoch: u64,
        spos: HashMap<Vec<u8>, PoolRegistration>,
        pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>,
        vrf_key_to_pool_id_map: HashMap<Vec<u8>, Vec<u8>>,
    ) -> Self {
        Self {
            block,
            epoch,
            spos,
            pending_deregistrations,
            vrf_key_to_pool_id_map,
        }
    }
}

impl From<SPOState> for BlockState {
    fn from(value: SPOState) -> Self {
        let spos: HashMap<KeyHash, PoolRegistration> = value.pools.into();
        let vrf_key_to_pool_id_map =
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
            vrf_key_to_pool_id_map,
        }
    }
}

impl From<&BlockState> for SPOState {
    fn from(value: &BlockState) -> Self {
        Self {
            pools: value.spos.iter().map(|(key, value)| (key.clone(), value.clone())).collect(),
            retiring: value
                .pending_deregistrations
                .iter()
                .map(|(epoch, key_hashes)| {
                    key_hashes
                        .iter()
                        .map(|key_hash| (key_hash.clone(), *epoch))
                        .collect::<Vec<(Vec<u8>, u64)>>()
                })
                .flatten()
                .collect(),
        }
    }
}

#[derive(Default, Debug, Clone, serde::Serialize)]
pub struct TotalBlocksMintedState {
    /// block number of Epoch Boundary from N-1 to N
    block: u64,
    /// total blocks minted for each pool operator keyed by vrf_key_hash
    /// until the end of Epoch N-1
    total_blocks_minted: HashMap<KeyHash, u64>,
}

/// Overall module state
pub struct State {
    /// Volatile states, one per volatile block
    history: StateHistory<BlockState>,

    /// Active stakes for each pool operator
    /// (epoch number, active stake)
    /// Remove elements when epoch number is less than current epoch number
    pub active_stakes: DashMap<KeyHash, BTreeMap<u64, u64>>,

    /// Volatile total blocks minted state, one per epoch
    /// Pop on first element when block number is smaller than `current block - SECURITY_PARAMETER_K`
    pub total_blocks_minted_history: StateHistory<TotalBlocksMintedState>,
}

impl State {
    // Construct with optional publisher
    pub fn new() -> Self {
        Self {
            history: StateHistory::new(
                "spo-states/block-state",
                StateHistoryStore::default_block_store(),
            ),
            active_stakes: DashMap::new(),
            total_blocks_minted_history: StateHistory::new(
                "spo-states/total-blocks-minted",
                StateHistoryStore::default_block_store(),
            ),
        }
    }

    pub fn current(&self) -> Option<&BlockState> {
        self.history.current()
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
        self.current().and_then(|state| state.vrf_key_to_pool_id_map.get(vrf_key_hash).cloned())
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

    /// Get pool metadata
    pub fn get_pool_metadata(&self, pool_id: &KeyHash) -> Option<PoolMetadata> {
        self.current()
            .and_then(|state| state.spos.get(pool_id).map(|p| p.pool_metadata.clone()))
            .flatten()
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
        let total_active_stake = self.get_total_active_stake(epoch);
        (active_stakes, total_active_stake)
    }

    fn get_active_stake(&self, spo: &KeyHash, epoch: u64) -> Option<u64> {
        self.active_stakes.get(spo).map(|stakes| stakes.get(&epoch).cloned()).flatten()
    }

    fn get_total_active_stake(&self, epoch: u64) -> u64 {
        self.active_stakes.iter().map(|entry| entry.value().get(&epoch).cloned().unwrap_or(0)).sum()
    }

    /// Get total blocks minted for each vrf vkey hash
    /// ## Arguments
    /// * `vrf_vkey_hashes` - A vector of vrf vkey hashes
    /// ## Returns
    /// `Vec<u64>` - a vector of total blocks minted for each vrf vkey hash.
    pub fn get_total_blocks_minted(&self, vrf_vkey_hashes: &Vec<KeyHash>) -> Vec<u64> {
        let Some(current) = self.total_blocks_minted_history.current() else {
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

    /// Handle TxCertificates with SPO registrations / de-registrations
    /// Returns an optional state message for end of epoch
    pub fn handle_tx_certs(
        &mut self,
        block: &BlockInfo,
        tx_certs_msg: &TxCertificatesMessage,
    ) -> Result<Option<Arc<Message>>> {
        let mut message: Option<Arc<Message>> = None;
        let mut current = self.history.get_rolled_back_state(block.number);
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
            if let Some(deregistrations) = deregistrations {
                for dr in deregistrations {
                    debug!("Retiring SPO {}", hex::encode(&dr));
                    match current.spos.remove(&dr) {
                        None => error!(
                            "Retirement requested for unregistered SPO {}",
                            hex::encode(&dr),
                        ),
                        Some(de_reg) => {
                            retired_spos.push(dr.clone());
                            current.vrf_key_to_pool_id_map.remove(&de_reg.vrf_key_hash);
                        }
                    };
                }
            }

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
                    current
                        .vrf_key_to_pool_id_map
                        .insert(reg.vrf_key_hash.clone(), reg.operator.clone());

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
        self.history.commit(block.number, current);

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
        let epoch_to_update = *epoch + 2;

        // update active stakes
        spos.par_iter().for_each(|(spo, value)| {
            let mut active_stakes = self
                .active_stakes
                .entry(spo.clone())
                .and_modify(|stakes| stakes.retain(|k, _| *k >= block.epoch))
                .or_insert_with(BTreeMap::new);

            active_stakes.insert(epoch_to_update, value.active);
        });
    }

    /// Handle Epoch Activity
    /// Returns blocks minted amount keyed by spo
    ///
    pub fn handle_epoch_activity(
        &mut self,
        block: &BlockInfo,
        epoch_activity_message: &EpochActivityMessage,
    ) -> Vec<(KeyHash, u64)> {
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

        let mut total_blocks_minted = self
            .total_blocks_minted_history
            .get_rolled_back_state(block.number)
            .total_blocks_minted;

        // handle blocks_minted state
        vrf_vkey_hashes.iter().for_each(|(vrf_vkey_hash, amount)| {
            total_blocks_minted
                .entry(vrf_vkey_hash.clone())
                .and_modify(|v| *v += *amount as u64)
                .or_insert(*amount as u64);
        });

        let new_state = TotalBlocksMintedState {
            block: block.number,
            total_blocks_minted,
        };

        self.total_blocks_minted_history.commit(block.number, new_state);

        let spos = vrf_vkey_hashes
            .iter()
            .filter_map(|(vrf_vkey_hash, amount)| {
                self.get_spo_from_vrf_key_hash(vrf_vkey_hash).map(|spo| (spo, *amount as u64))
            })
            .collect::<Vec<(KeyHash, u64)>>();
        spos
    }

    pub fn bootstrap(&mut self, state: SPOState) {
        self.history.clear();
        self.history.commit_forced(state.into());
    }

    pub fn dump(&self, block_height: u64) -> Option<SPOState> {
        self.history.get_by_index_reverse(block_height).map(SPOState::from)
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use acropolis_common::{DelegatedStake, PoolRetirement, Ratio, TxCertificate};

    #[tokio::test]
    async fn new_state_is_empty() {
        let state = State::new();
        assert_eq!(0, state.history.len());
        assert_eq!(0, state.active_stakes.len());
    }

    #[tokio::test]
    async fn current_on_new_state_returns_none() {
        let state = State::new();
        assert!(state.current().is_none());
    }

    #[tokio::test]
    async fn state_is_not_empty_after_handle_tx_certs() {
        let mut state = State::new();
        let msg = new_certs_msg();
        let block = new_block(1);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        assert_eq!(1, state.history.len());
    }

    #[tokio::test]
    async fn active_stakes_is_not_empty_after_handle_spdd() {
        let mut state = State::new();
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
            BTreeMap::from([(3, 1)]),
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
            BTreeMap::from([(3, 1), (4, 2)]),
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
            BTreeMap::from([(4, 2), (5, 3)]),
            state.active_stakes.get(&vec![1]).unwrap().clone()
        );
    }

    #[tokio::test]
    async fn get_total_blocks_minted_returns_zero_when_state_is_new() {
        let state = State::new();
        let total_blocks_minted = state.get_total_blocks_minted(&vec![vec![1], vec![2]]);
        assert_eq!(2, total_blocks_minted.len());
        assert_eq!(0, total_blocks_minted[0]);
        assert_eq!(0, total_blocks_minted[1]);
    }

    #[tokio::test]
    async fn get_total_blocks_minted_returns_data_after_handle_epoch_activity() {
        let mut state = State::new();
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
        let mut state = State::new();
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
    async fn spo_gets_registered() {
        let mut state = State::new();
        let mut msg = new_certs_msg();
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
        let mut state = State::new();
        let mut msg = new_certs_msg();
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
        let mut state = State::new();
        let mut msg = new_certs_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        let mut block = new_block(0);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let mut msg = new_certs_msg();
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
        let mut msg = new_certs_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        let mut block = new_block(0);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let mut msg = new_certs_msg();
        block.number = 1;
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![1],
            epoch: 2,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let msg = new_certs_msg();
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
        let mut msg = new_certs_msg();
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
        let mut msg = new_certs_msg();
        block.number = 1;
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 1,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let msg = new_certs_msg();
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
        let state = State::new();
        let (active_stakes, total) = state.get_pools_active_stakes(&vec![vec![1], vec![2]], 0);
        assert_eq!(2, active_stakes.len());
        assert_eq!(0, active_stakes[0]);
        assert_eq!(0, active_stakes[1]);
        assert_eq!(0, total);
    }

    #[tokio::test]
    async fn get_pools_active_stakes_returns_zero_when_epoch_is_not_found() {
        let mut state = State::new();
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
        let mut state = State::new();
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
        let mut state = State::new();
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
        let mut state = State::new();
        let mut msg = new_certs_msg();
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
        let mut msg = new_certs_msg();
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
        let msg = new_certs_msg();
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
        let msg = new_certs_msg();
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
        let state = State::new();
        assert!(state.get_retiring_pools().is_empty());
    }

    #[tokio::test]
    async fn get_retiring_pools_returns_pools() {
        let mut state = State::new();
        let mut msg = new_certs_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        let mut block = new_block(0);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let mut msg = new_certs_msg();
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
