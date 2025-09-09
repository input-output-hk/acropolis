//! Acropolis SPOState: State storage

use acropolis_common::{
    ledger_state::SPOState,
    messages::{
        CardanoMessage, Message, SPOStateMessage, StakeAddressDeltasMessage,
        StakeRewardDeltasMessage, TxCertificatesMessage, WithdrawalsMessage,
    },
    params::TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH,
    BlockInfo, KeyHash, PoolMetadata, PoolRegistration, PoolRetirement, Relay, StakeCredential,
    TxCertificate,
};
use anyhow::{bail, Result};
use dashmap::DashMap;
use imbl::HashMap;
use serde::Serialize;
use serde_with::{hex::Hex, serde_as};
use std::sync::Arc;
use tracing::{debug, error, info};

use crate::{historical_spo_state::HistoricalSPOState, store_config::StoreConfig};

/// State of an individual stake address
/// We don't care about DRep they are delegated to
#[serde_as]
#[derive(Debug, Default, Clone, Serialize)]
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
}

#[derive(Default, Debug, Clone)]
pub struct State {
    store_config: StoreConfig,

    block: u64,

    epoch: u64,

    spos: HashMap<Vec<u8>, PoolRegistration>,

    pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>,

    /// vrf_key_hash -> pool_id mapping
    vrf_key_to_pool_id_map: HashMap<Vec<u8>, Vec<u8>>,

    /// historical spo state
    /// keyed by pool operator id
    historical_spos: Option<HashMap<KeyHash, HistoricalSPOState>>,

    /// stake_addresses (We save stake_addresses according to store_config)
    stake_addresses: Option<Arc<DashMap<KeyHash, StakeAddressState>>>,
}

impl State {
    pub fn new(config: &StoreConfig) -> Self {
        Self {
            store_config: config.clone(),
            block: 0,
            epoch: 0,
            spos: HashMap::new(),
            pending_deregistrations: HashMap::new(),
            vrf_key_to_pool_id_map: HashMap::new(),
            historical_spos: if config.store_historical_state() {
                Some(HashMap::new())
            } else {
                None
            },
            stake_addresses: if config.store_stake_addresses {
                Some(Arc::new(DashMap::new()))
            } else {
                None
            },
        }
    }

    pub fn is_historical_state_enabled(&self) -> bool {
        self.historical_spos.is_some()
    }

    pub fn is_historical_delegators_enabled(&self) -> bool {
        self.store_config.store_delegators
    }

    pub fn is_stake_address_enabled(&self) -> bool {
        self.store_config.store_stake_addresses
    }
}

impl From<SPOState> for State {
    fn from(value: SPOState) -> Self {
        let spos: HashMap<KeyHash, PoolRegistration> = value.pools.into();
        let vrf_key_to_pool_id_map =
            spos.iter().map(|(k, v)| (v.vrf_key_hash.clone(), k.clone())).collect();
        let pending_deregistrations =
            value.retiring.into_iter().fold(HashMap::new(), |mut acc, (key_hash, epoch)| {
                acc.entry(epoch).or_insert_with(Vec::new).push(key_hash);
                acc
            });
        Self {
            store_config: StoreConfig::default(),
            block: 0,
            epoch: 0,
            spos,
            pending_deregistrations,
            vrf_key_to_pool_id_map,
            historical_spos: None,
            stake_addresses: None,
        }
    }
}

impl From<&State> for SPOState {
    fn from(state: &State) -> Self {
        Self {
            pools: state.spos.iter().map(|(key, value)| (key.clone(), value.clone())).collect(),
            retiring: state
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

impl State {
    #[allow(dead_code)]
    pub fn get(&self, pool_id: &KeyHash) -> Option<&PoolRegistration> {
        self.spos.get(pool_id)
    }

    /// Get SPO from vrf_key_hash
    pub fn get_pool_id_from_vrf_key_hash(&self, vrf_key_hash: &KeyHash) -> Option<KeyHash> {
        self.vrf_key_to_pool_id_map.get(vrf_key_hash).cloned()
    }

    /// Get vrf_key_to_pool_id_map
    pub fn get_blocks_minted_by_spos(
        &self,
        vrf_key_hashes: &Vec<(KeyHash, usize)>,
    ) -> Vec<(KeyHash, usize)> {
        vrf_key_hashes
            .iter()
            .filter_map(|(vrf_key_hash, amount)| {
                self.vrf_key_to_pool_id_map.get(vrf_key_hash).map(|spo| (spo.clone(), *amount))
            })
            .collect()
    }

    /// Get all Stake Pool operators' operator hashes
    pub fn list_pool_operators(&self) -> Vec<KeyHash> {
        self.spos.keys().cloned().collect()
    }

    /// Get all Stake Pool Operators' operator hashes and their registration information
    pub fn list_pools_with_info(&self) -> Vec<(KeyHash, PoolRegistration)> {
        self.spos.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
    }

    /// Get pool metadata
    pub fn get_pool_metadata(&self, pool_id: &KeyHash) -> Option<PoolMetadata> {
        self.spos.get(pool_id).map(|p| p.pool_metadata.clone()).flatten()
    }

    /// Get Pool Delegators
    pub fn get_pool_delegators(&self, pool_id: &KeyHash) -> Option<Vec<(KeyHash, u64)>> {
        let Some(stake_addresses) = self.stake_addresses.as_ref() else {
            return None;
        };
        let Some(historical_spos) = self.historical_spos.as_ref() else {
            return None;
        };

        let delegators = historical_spos.get(pool_id).map(|s| s.delegators.clone()).flatten();
        let Some(delegators) = delegators.as_ref() else {
            return None;
        };

        let mut delegators_with_live_stakes = Vec::<(KeyHash, u64)>::new();
        for delegator in delegators {
            let account = stake_addresses.get(delegator)?;
            let balance = account.utxo_value + account.rewards;
            delegators_with_live_stakes.push((delegator.clone(), balance));
        }
        Some(delegators_with_live_stakes)
    }

    /// Get pool relay
    pub fn get_pool_relays(&self, pool_id: &KeyHash) -> Option<Vec<Relay>> {
        self.spos.get(pool_id).map(|p| p.relays.clone())
    }

    /// Get pools that will be retired in the upcoming epochs
    pub fn get_retiring_pools(&self) -> Vec<PoolRetirement> {
        let current_epoch = self.epoch;
        self.pending_deregistrations
            .iter()
            .filter(|(&epoch, _)| epoch > current_epoch)
            .flat_map(|(&epoch, retiring_operators)| {
                retiring_operators.iter().map(move |operator| PoolRetirement {
                    operator: operator.clone(),
                    epoch,
                })
            })
            .collect()
    }

    fn log_stats(&self) {
        info!(
            num_spos = self.spos.keys().len(),
            num_pending_deregistrations =
                self.pending_deregistrations.values().map(|d| d.len()).sum::<usize>(),
        );
    }

    pub fn tick(&self) -> Result<()> {
        self.log_stats();
        Ok(())
    }

    fn handle_new_epoch(&mut self, block: &BlockInfo) -> Arc<Message> {
        self.epoch = block.epoch;
        debug!(epoch = self.epoch, "New epoch");

        // Flatten into vector of registrations, before retirement so retiring ones
        // are still included
        let spos = self.spos.values().cloned().collect();

        // Deregister any pending
        let mut retired_spos: Vec<KeyHash> = Vec::new();
        let deregistrations = self.pending_deregistrations.remove(&self.epoch);
        if let Some(deregistrations) = deregistrations {
            for dr in deregistrations {
                debug!("Retiring SPO {}", hex::encode(&dr));
                match self.spos.remove(&dr) {
                    None => error!(
                        "Retirement requested for unregistered SPO {}",
                        hex::encode(&dr),
                    ),
                    Some(de_reg) => {
                        retired_spos.push(dr.clone());
                        self.vrf_key_to_pool_id_map.remove(&de_reg.vrf_key_hash);
                    }
                };
            }
        }

        Arc::new(Message::Cardano((
            block.clone(),
            CardanoMessage::SPOState(SPOStateMessage {
                epoch: block.epoch - 1,
                spos,
                retired_spos,
            }),
        )))
    }

    fn handle_pool_registration(&mut self, block: &BlockInfo, reg: &PoolRegistration) {
        debug!(
            block = block.number,
            "Registering SPO {}",
            hex::encode(&reg.operator)
        );
        self.spos.insert(reg.operator.clone(), reg.clone());
        self.vrf_key_to_pool_id_map.insert(reg.vrf_key_hash.clone(), reg.operator.clone());

        // Remove any existing queued deregistrations
        for (epoch, deregistrations) in &mut self.pending_deregistrations.iter_mut() {
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

    fn handle_pool_retirement(&mut self, block: &BlockInfo, ret: &PoolRetirement) {
        debug!(
            "SPO {} wants to retire at the end of epoch {} (cert in block number {})",
            hex::encode(&ret.operator),
            ret.epoch,
            block.number
        );
        if ret.epoch <= self.epoch {
            error!(
                "SPO retirement received for current or past epoch {} for SPO {}",
                ret.epoch,
                hex::encode(&ret.operator)
            );
        } else if ret.epoch > self.epoch + TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH {
            error!(
                "SPO retirement received for epoch {} that exceeds future limit for SPO {}",
                ret.epoch,
                hex::encode(&ret.operator)
            );
        } else {
            // Replace any existing queued deregistrations
            for (epoch, deregistrations) in &mut self.pending_deregistrations.iter_mut() {
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
            self.pending_deregistrations.entry(ret.epoch).or_default().push(ret.operator.clone());
        }
    }

    fn register_stake_address(&mut self, credential: &StakeCredential) {
        let Some(stake_addresses) = self.stake_addresses.as_mut() else {
            return;
        };

        let hash = credential.get_hash();
        let mut sas = stake_addresses.entry(hash.clone()).or_default();
        if sas.registered {
            error!(
                "Stake address hash {} registered when already registered",
                hex::encode(&hash)
            );
            return;
        } else {
            sas.registered = true;
        }
    }

    fn deregister_stake_address(&mut self, credential: &StakeCredential) {
        let Some(stake_addresses) = self.stake_addresses.as_mut() else {
            return;
        };

        let hash = credential.get_hash();
        if let Some(mut sas) = stake_addresses.get_mut(&hash) {
            if sas.registered {
                sas.registered = false;
                // update historical_spos
                if let Some(historical_spos) = self.historical_spos.as_mut() {
                    if let Some(old_spo) = sas.delegated_spo.as_ref() {
                        // remove delegators from old_spo
                        if let Some(historical_spo) = historical_spos.get_mut(old_spo) {
                            if let Some(removed) = historical_spo.remove_delegator(&hash) {
                                if !removed {
                                    error!(
                                        "Historical SPO state for {} does not contain delegator {}",
                                        hex::encode(old_spo),
                                        hex::encode(&hash)
                                    );
                                }
                            }
                        }
                    }
                }
            } else {
                error!(
                    "Deregistration of unregistered stake address hash {}",
                    hex::encode(hash)
                );
            }
        } else {
            error!(
                "Deregistration of unknown stake address hash {}",
                hex::encode(hash)
            );
        }
    }

    /// Record a stake delegation
    /// Update historical_spo_state's delegators
    fn record_stake_delegation(&mut self, credential: &StakeCredential, spo: &KeyHash) {
        let Some(stake_addresses) = self.stake_addresses.as_mut() else {
            return;
        };

        let hash = credential.get_hash();
        // Get old stake address state, or create one
        if let Some(mut sas) = stake_addresses.get_mut(&hash) {
            if sas.registered {
                let old_spo = sas.delegated_spo.take();
                sas.delegated_spo = Some(spo.clone());
                // update historical_spos
                if let Some(historical_spos) = self.historical_spos.as_mut() {
                    // Remove old delegator
                    if let Some(old_spo) = old_spo {
                        match historical_spos.get_mut(&old_spo) {
                            Some(historical_spo) => {
                                if let Some(removed) = historical_spo.remove_delegator(&hash) {
                                    if !removed {
                                        error!(
                                            "Historical SPO state for {} does not contain delegator {}",
                                            hex::encode(old_spo),
                                            hex::encode(&hash)
                                        );
                                    }
                                }
                            }
                            _ => {
                                error!("Missing Historical SPO state for {}", hex::encode(old_spo));
                            }
                        }
                    }

                    // get old one or create from store_config
                    let historical_spo = historical_spos
                        .entry(spo.clone())
                        .or_insert_with(|| HistoricalSPOState::new(&self.store_config));
                    if let Some(added) = historical_spo.add_delegator(&hash) {
                        if !added {
                            error!(
                                "Historical SPO state for {} already contains delegator {}",
                                hex::encode(spo),
                                hex::encode(&hash)
                            );
                        }
                    }
                }
            } else {
                error!(
                    "Unregistered stake address in stake delegation: {}",
                    hex::encode(hash)
                );
            }
        } else {
            error!(
                "Unknown stake address in stake delegation: {}",
                hex::encode(hash)
            );
        }
    }

    /// Handle TxCertificates with SPO registrations / de-registrations
    /// Returns an optional state message for end of epoch
    pub fn handle_tx_certs(
        &mut self,
        block: &BlockInfo,
        tx_certs_msg: &TxCertificatesMessage,
    ) -> Result<Option<Arc<Message>>> {
        let mut maybe_message: Option<Arc<Message>> = None;
        if block.epoch > self.epoch {
            // handle new epoch
            maybe_message = Some(self.handle_new_epoch(block));
        }
        // Handle certificates
        for tx_cert in tx_certs_msg.certificates.iter() {
            match tx_cert {
                // for spo_state
                TxCertificate::PoolRegistration(reg) => {
                    self.handle_pool_registration(block, reg);
                }
                TxCertificate::PoolRetirement(ret) => {
                    self.handle_pool_retirement(block, ret);
                }

                // for stake addresses
                TxCertificate::StakeRegistration(sc_with_pos) => {
                    self.register_stake_address(&sc_with_pos.stake_credential);
                }
                TxCertificate::StakeDeregistration(sc) => {
                    self.deregister_stake_address(&sc);
                }
                TxCertificate::Registration(reg) => {
                    self.register_stake_address(&reg.credential);
                    // we don't care deposite
                }
                TxCertificate::Deregistration(dreg) => {
                    self.deregister_stake_address(&dreg.credential);
                    // we don't care refund
                }
                TxCertificate::StakeDelegation(delegation) => {
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                }
                TxCertificate::StakeAndVoteDelegation(delegation) => {
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                    // don't care about vote delegation
                }
                TxCertificate::StakeRegistrationAndDelegation(delegation) => {
                    self.register_stake_address(&delegation.credential);
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                }
                TxCertificate::StakeRegistrationAndVoteDelegation(delegation) => {
                    self.register_stake_address(&delegation.credential);
                    // don't care about vote delegation
                }
                TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(delegation) => {
                    self.register_stake_address(&delegation.credential);
                    self.record_stake_delegation(&delegation.credential, &delegation.operator);
                    // don't care about vote delegation
                }
                _ => (),
            }
        }
        Ok(maybe_message)
    }

    /// Update an unsigned value with a signed delta, with fences
    fn update_value_with_delta(value: &mut u64, delta: i64) -> Result<()> {
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

    /// Add a reward to a reward account (by hash)
    fn update_reward_with_delta(&mut self, account: &KeyHash, delta: i64) {
        let Some(stake_addresses) = self.stake_addresses.as_mut() else {
            return;
        };

        // Get old stake address state, or create one
        let mut sas = match stake_addresses.get_mut(account) {
            Some(existing) => existing,
            None => {
                stake_addresses.insert(account.clone(), StakeAddressState::default());
                stake_addresses.get_mut(account).unwrap()
            }
        };

        if let Err(e) = Self::update_value_with_delta(&mut sas.rewards, delta) {
            error!("Adding to reward account {}: {e}", hex::encode(account));
        }
    }

    /// Handle withdrawals
    pub fn handle_withdrawals(&mut self, withdrawals_msg: &WithdrawalsMessage) -> Result<()> {
        let Some(stake_addresses) = self.stake_addresses.as_mut() else {
            return Ok(());
        };

        for withdrawal in withdrawals_msg.withdrawals.iter() {
            let hash = withdrawal.address.get_hash();
            // Get old stake address state - which must exist
            if let Some(sas) = stake_addresses.get(hash) {
                // Zero withdrawals are expected, as a way to validate stake addresses (per Pi)
                if withdrawal.value != 0 {
                    let mut sas = sas.clone();
                    if let Err(e) =
                        Self::update_value_with_delta(&mut sas.rewards, -(withdrawal.value as i64))
                    {
                        error!(
                            "Withdrawing from stake address {} hash {}: {e}",
                            withdrawal.address.to_string().unwrap_or("???".to_string()),
                            hex::encode(hash)
                        );
                    } else {
                        // Update the stake address
                        stake_addresses.insert(hash.to_vec(), sas);
                    }
                }
            } else {
                error!(
                    "Unknown stake address in withdrawal: {}",
                    withdrawal.address.to_string().unwrap_or("???".to_string())
                );
            }
        }

        Ok(())
    }

    /// Handle stake deltas
    pub fn handle_stake_deltas(&mut self, deltas_msg: &StakeAddressDeltasMessage) -> Result<()> {
        let Some(stake_addresses) = self.stake_addresses.as_mut() else {
            return Ok(());
        };

        // Handle deltas
        for delta in deltas_msg.deltas.iter() {
            // Fold both stake key and script hashes into one - assuming the chance of
            // collision is negligible
            let hash = delta.address.get_hash();

            // Stake addresses don't need to be registered if they aren't used for
            // stake or drep delegation, but we need to track them in case they are later
            let mut sas = stake_addresses.entry(hash.to_vec()).or_default();

            if let Err(e) = Self::update_value_with_delta(&mut sas.utxo_value, delta.delta) {
                error!("Applying delta to stake hash {}: {e}", hex::encode(hash));
            }
        }

        Ok(())
    }

    /// Handle Stake Reward Deltas
    pub fn handle_stake_reward_deltas(
        &mut self,
        _block_info: &BlockInfo,
        reward_deltas_msg: &StakeRewardDeltasMessage,
    ) -> Result<()> {
        let Some(_) = self.stake_addresses.as_mut() else {
            return Ok(());
        };

        // Handle deltas
        for delta in reward_deltas_msg.deltas.iter() {
            self.update_reward_with_delta(&delta.hash, delta.delta);
        }

        Ok(())
    }

    pub fn dump(&self) -> SPOState {
        SPOState::from(self)
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::*;
    use acropolis_common::{
        state_history::{StateHistory, StateHistoryStore},
        PoolRetirement, Ratio, TxCertificate,
    };
    use tokio::sync::Mutex;

    #[test]
    fn get_returns_none_on_empty_state() {
        let state = State::default();
        assert!(state.get(&vec![0]).is_none());
    }

    #[test]
    fn vrf_key_to_pool_id_map_is_none_on_empty_state() {
        let state = State::default();
        assert!(state.vrf_key_to_pool_id_map.is_empty());
    }

    #[test]
    fn list_pool_operators_returns_empty_on_empty_state() {
        let state = State::default();
        assert!(state.list_pool_operators().is_empty());
    }

    #[tokio::test]
    async fn handle_tx_certs_returns_message_on_new_epoch() {
        let mut state = State::default();
        let msg = new_certs_msg();
        let block = new_block(1);
        let maybe_message = state.handle_tx_certs(&block, &msg).unwrap();
        assert!(maybe_message.is_some());
    }

    #[tokio::test]
    async fn spo_gets_registered() {
        let mut state = State::default();
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
        assert_eq!(1, state.spos.len());
        let spo = state.spos.get(&vec![0]);
        assert!(!spo.is_none());
    }

    #[tokio::test]
    async fn pending_deregistration_gets_queued() {
        let mut state = State::default();
        let mut msg = new_certs_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 1,
        }));
        let block = new_block(0);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        assert_eq!(1, state.pending_deregistrations.len());
        let drs = state.pending_deregistrations.get(&1);
        assert!(!drs.is_none());
        if let Some(drs) = drs {
            assert_eq!(1, drs.len());
            assert!(drs.contains(&vec![0]));
        }
    }

    #[tokio::test]
    async fn second_pending_deregistration_gets_queued() {
        let mut state = State::default();
        let mut block = new_block(0);
        let mut msg = new_certs_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());

        block.number = 1;
        msg = new_certs_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![1],
            epoch: 2,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());

        assert_eq!(1, state.pending_deregistrations.len());
        let drs = state.pending_deregistrations.get(&2);
        assert!(!drs.is_none());
        if let Some(drs) = drs {
            assert_eq!(2, drs.len());
            assert!(drs.contains(&vec![0u8]));
            assert!(drs.contains(&vec![1u8]));
        }
    }

    #[tokio::test]
    async fn rollback_removes_second_pending_deregistration() {
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "spo_state",
            StateHistoryStore::default_block_store(),
        )));
        let mut state = history.lock().await.get_current_state();
        let mut block = new_block(0);
        let mut msg = new_certs_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        history.lock().await.commit(block.number, state);

        let mut state = history.lock().await.get_current_state();
        block.number = 1;
        msg = new_certs_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![1],
            epoch: 2,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        history.lock().await.commit(block.number, state);

        block.number = 1;
        let mut state = history.lock().await.get_rolled_back_state(block.number);
        msg = new_certs_msg();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        assert_eq!(1, state.pending_deregistrations.len());
        let drs = state.pending_deregistrations.get(&2);
        assert!(!drs.is_none());
        if let Some(drs) = drs {
            assert_eq!(1, drs.len());
            assert!(drs.contains(&vec![0]));
        }
    }

    #[test]
    fn spo_gets_deregistered() {
        let mut state = State::default();
        let mut block = new_block(0);
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
        assert!(state.handle_tx_certs(&block, &msg).is_ok());

        assert_eq!(1, state.spos.len());
        let spo = state.spos.get(&vec![0u8]);
        assert!(!spo.is_none());

        block.number = 1;
        let mut msg = new_certs_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 1,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());

        block.epoch = 1; // SPO get retired at the start of the epoch it requests
        block.number = 2;
        let msg = new_certs_msg();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        assert!(state.spos.is_empty());
    }

    #[tokio::test]
    async fn spo_gets_restored_on_rollback() {
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "spo_state",
            StateHistoryStore::default_block_store(),
        )));
        let mut state = history.lock().await.get_current_state();
        let mut block = new_block(0);
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
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        assert_eq!(1, state.spos.len());
        let spo = state.spos.get(&vec![0u8]);
        assert!(!spo.is_none());
        history.lock().await.commit(block.number, state);

        let mut state = history.lock().await.get_current_state();
        block.number = 1;
        msg = new_certs_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 1,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        history.lock().await.commit(block.number, state);

        let mut state = history.lock().await.get_current_state();
        block.epoch = 1;
        block.number = 2;
        msg = new_certs_msg();
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        assert!(state.spos.is_empty());
        history.lock().await.commit(block.number, state);

        block.number = 2;
        block.epoch = 0;
        let msg = new_certs_msg();
        let mut state = history.lock().await.get_rolled_back_state(block.number);
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        assert_eq!(1, state.spos.len());
        let spo = state.spos.get(&vec![0]);
        assert!(!spo.is_none());
    }

    #[tokio::test]
    async fn get_retiring_pools_returns_empty_when_state_is_new() {
        let state = State::default();
        assert!(state.get_retiring_pools().is_empty());
    }

    #[tokio::test]
    async fn get_retiring_pools_returns_pools() {
        let mut state = State::default();
        let mut block = new_block(0);
        let mut msg = new_certs_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![0],
            epoch: 2,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());

        block.number = 1;
        msg = new_certs_msg();
        msg.certificates.push(TxCertificate::PoolRetirement(PoolRetirement {
            operator: vec![1],
            epoch: 3,
        }));
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let mut retiring_pools = state.get_retiring_pools();
        retiring_pools.sort_by_key(|p| p.epoch);
        assert_eq!(2, retiring_pools.len());
        assert_eq!(vec![0], retiring_pools[0].operator);
        assert_eq!(2, retiring_pools[0].epoch);
        assert_eq!(vec![1], retiring_pools[1].operator);
        assert_eq!(3, retiring_pools[1].epoch);
    }
}
