//! Acropolis SPOState: State storage

use acropolis_common::{
    crypto::keyhash_224,
    ledger_state::SPOState,
    messages::{
        CardanoMessage, Message, SPOStateMessage, StakeAddressDeltasMessage,
        StakeRewardDeltasMessage, TxCertificatesMessage, WithdrawalsMessage,
    },
    params::TECHNICAL_PARAMETER_POOL_RETIRE_MAX_EPOCH,
    queries::governance::VoteRecord,
    stake_addresses::StakeAddressMap,
    BlockInfo, KeyHash, PoolMetadata, PoolRegistration, PoolRetirement, PoolUpdateEvent, Relay,
    StakeAddress, TxCertificate, TxHash, TxIdentifier, Voter, VotingProcedures,
};
use anyhow::Result;
use imbl::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, error, info};

use crate::{historical_spo_state::HistoricalSPOState, store_config::StoreConfig};

#[derive(Default, Debug, Clone)]
pub struct State {
    store_config: StoreConfig,

    #[allow(dead_code)]
    block: u64,

    epoch: u64,

    spos: HashMap<Vec<u8>, PoolRegistration>,

    pending_updates: HashMap<Vec<u8>, PoolRegistration>,

    pending_deregistrations: HashMap<u64, Vec<Vec<u8>>>,

    // Total blocks minted till block number
    // Keyed by pool_id
    total_blocks_minted: HashMap<KeyHash, u64>,

    /// historical spo state
    /// keyed by pool operator id
    historical_spos: Option<HashMap<KeyHash, HistoricalSPOState>>,

    /// stake_addresses (We save stake_addresses according to store_config)
    stake_addresses: Option<Arc<Mutex<StakeAddressMap>>>,
}

impl State {
    pub fn new(config: &StoreConfig) -> Self {
        Self {
            store_config: config.clone(),
            block: 0,
            epoch: 0,
            spos: HashMap::new(),
            pending_updates: HashMap::new(),
            pending_deregistrations: HashMap::new(),
            total_blocks_minted: HashMap::new(),
            historical_spos: if config.store_historical_state() {
                Some(HashMap::new())
            } else {
                None
            },
            stake_addresses: if config.store_stake_addresses {
                Some(Arc::new(Mutex::new(StakeAddressMap::new())))
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

    pub fn is_historical_updates_enabled(&self) -> bool {
        self.store_config.store_updates
    }

    pub fn is_historical_votes_enabled(&self) -> bool {
        self.store_config.store_votes
    }

    pub fn is_historical_blocks_enabled(&self) -> bool {
        self.store_config.store_blocks
    }

    pub fn is_stake_address_enabled(&self) -> bool {
        self.store_config.store_stake_addresses
    }
}

impl From<SPOState> for State {
    fn from(value: SPOState) -> Self {
        let spos: HashMap<KeyHash, PoolRegistration> = value.pools.into();
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
            pending_updates: value.updates.into(),
            pending_deregistrations,
            total_blocks_minted: HashMap::new(),
            historical_spos: None,
            stake_addresses: None,
        }
    }
}

impl From<&State> for SPOState {
    fn from(state: &State) -> Self {
        Self {
            pools: state.spos.iter().map(|(key, value)| (key.clone(), value.clone())).collect(),
            updates: state
                .pending_updates
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect(),
            retiring: state
                .pending_deregistrations
                .iter()
                .flat_map(|(epoch, key_hashes)| {
                    key_hashes
                        .iter()
                        .map(|key_hash| (key_hash.clone(), *epoch))
                        .collect::<Vec<(Vec<u8>, u64)>>()
                })
                .collect(),
        }
    }
}

impl State {
    #[allow(dead_code)]
    pub fn get(&self, pool_id: &KeyHash) -> Option<&PoolRegistration> {
        self.spos.get(pool_id)
    }

    /// Get total blocks minted by pools
    pub fn get_total_blocks_minted_by_pools(&self, pools_operators: &[KeyHash]) -> Vec<u64> {
        pools_operators
            .iter()
            .map(|pool_operator| *self.total_blocks_minted.get(pool_operator).unwrap_or(&0))
            .collect()
    }

    /// Get total blocks minted by pool
    pub fn get_total_blocks_minted_by_pool(&self, pool_operator: &KeyHash) -> u64 {
        *self.total_blocks_minted.get(pool_operator).unwrap_or(&0)
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
        self.spos.get(pool_id).and_then(|p| p.pool_metadata.clone())
    }

    /// Get Pool Delegators
    pub fn get_pool_delegators(&self, pool_operator: &KeyHash) -> Option<Vec<(KeyHash, u64)>> {
        let stake_addresses = self.stake_addresses.as_ref()?;
        let historical_spos = self.historical_spos.as_ref()?;

        let stake_addresses = stake_addresses.lock().unwrap();
        let delegators = historical_spos
            .get(pool_operator)
            .and_then(|s| s.delegators.clone())
            .map(|s| s.into_iter().collect::<Vec<StakeAddress>>())?;

        let delegators_map = stake_addresses.get_accounts_balances_map(&delegators);
        delegators_map.map(|map| map.into_iter().collect())
    }

    /// Get Blocks by Pool
    /// Return Vector of block heights
    /// Return None when store_blocks not enabled
    pub fn get_blocks_by_pool(&self, pool_id: &KeyHash) -> Option<Vec<u64>> {
        self.historical_spos.as_ref()?.get(pool_id).and_then(|s| s.get_all_blocks())
    }

    /// Get Blocks by Pool and Epoch
    /// Return None when store_blocks not enabled
    pub fn get_blocks_by_pool_and_epoch(&self, pool_id: &KeyHash, epoch: u64) -> Option<Vec<u64>> {
        self.historical_spos.as_ref()?.get(pool_id).and_then(|s| s.get_blocks_by_epoch(epoch))
    }

    /// Get Pool Updates
    pub fn get_pool_updates(&self, pool_id: &KeyHash) -> Option<Vec<PoolUpdateEvent>> {
        self.historical_spos.as_ref()?.get(pool_id).and_then(|s| s.updates.clone())
    }

    /// Get Pool Votes
    pub fn get_pool_votes(&self, pool_id: &KeyHash) -> Option<Vec<VoteRecord>> {
        self.historical_spos.as_ref()?.get(pool_id).and_then(|s| s.votes.clone())
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

    // Handle block's minting.
    pub fn handle_mint(&mut self, block_info: &BlockInfo, issuer_vkey: &[u8]) -> bool {
        let pool_id = keyhash_224(issuer_vkey);
        if self.spos.get(&pool_id).is_none() {
            return false;
        }

        *(self.total_blocks_minted.entry(pool_id.clone()).or_insert(0)) += 1;
        // if store_blocks is enabled
        if self.is_historical_blocks_enabled() {
            if let Some(historical_spos) = self.historical_spos.as_mut() {
                if let Some(historical_spo) = historical_spos.get_mut(&pool_id) {
                    historical_spo.add_block(block_info.epoch, block_info.number);
                }
            }
        }
        true
    }

    fn handle_new_epoch(&mut self, block: &BlockInfo) -> Arc<Message> {
        self.epoch = block.epoch;
        debug!(epoch = self.epoch, "New epoch");

        // Flatten into vector of registrations, before retirement so retiring ones
        // are still included
        let spos = self.spos.values().cloned().collect();

        // Update any pending
        for (operator, reg) in &self.pending_updates {
            self.spos.insert(operator.clone(), reg.clone());
        }
        self.pending_updates.clear();

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
                    Some(_de_reg) => {
                        retired_spos.push(dr.clone());
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

    fn handle_pool_registration(
        &mut self,
        block: &BlockInfo,
        reg: &PoolRegistration,
        tx_identifier: &TxIdentifier,
        cert_index: &u64,
    ) {
        if self.spos.contains_key(&reg.operator) {
            debug!(
                block = block.number,
                "New pending SPO update {} {:?}",
                hex::encode(&reg.operator),
                reg
            );
            self.pending_updates.insert(reg.operator.clone(), reg.clone());
        } else {
            debug!(
                block = block.number,
                "Registering SPO {} {:?}",
                hex::encode(&reg.operator),
                reg
            );
            self.spos.insert(reg.operator.clone(), reg.clone());
        }

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

        // update historical spos
        if let Some(historical_spos) = self.historical_spos.as_mut() {
            // Don't check there was registration already or not
            // because we don't remove registration when pool is retired.
            let historical_spo = historical_spos
                .entry(reg.operator.clone())
                .or_insert_with(|| HistoricalSPOState::new(&self.store_config));
            historical_spo.add_pool_registration(reg);
            historical_spo
                .add_pool_updates(PoolUpdateEvent::register_event(*tx_identifier, *cert_index));
        }
    }

    fn handle_pool_retirement(
        &mut self,
        block: &BlockInfo,
        ret: &PoolRetirement,
        tx_identifier: &TxIdentifier,
        cert_index: &u64,
    ) {
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

            // Note: not removing pending updates - the deregistation may happen many
            // epochs later than the update, and we apply updates before deregistrations
            // so they cannot recreate deregistered SPOs
        }

        // update historical spos
        if let Some(historical_spos) = self.historical_spos.as_mut() {
            if let Some(historical_spo) = historical_spos.get_mut(&ret.operator) {
                historical_spo
                    .add_pool_updates(PoolUpdateEvent::retire_event(*tx_identifier, *cert_index));
            } else {
                error!(
                    "Historical SPO for {} not registered when try to retire it",
                    hex::encode(&ret.operator)
                );
            }
        }
    }

    fn register_stake_address(&mut self, stake_address: &StakeAddress) {
        let Some(stake_addresses) = self.stake_addresses.as_ref() else {
            return;
        };
        let mut stake_addresses = stake_addresses.lock().unwrap();
        stake_addresses.register_stake_address(stake_address);
    }

    fn deregister_stake_address(&mut self, stake_address: &StakeAddress) {
        let Some(stake_addresses) = self.stake_addresses.as_ref() else {
            return;
        };
        let mut stake_addresses = stake_addresses.lock().unwrap();
        let old_spo = stake_addresses.get(stake_address).and_then(|s| s.delegated_spo.clone());

        if stake_addresses.deregister_stake_address(stake_address) {
            // update historical_spos
            if let Some(historical_spos) = self.historical_spos.as_mut() {
                if let Some(old_spo) = old_spo.as_ref() {
                    // remove delegators from old_spo
                    if let Some(historical_spo) = historical_spos.get_mut(old_spo) {
                        if let Some(removed) = historical_spo.remove_delegator(stake_address) {
                            if !removed {
                                error!(
                                    "Historical SPO state for {} does not contain delegator {}",
                                    hex::encode(old_spo),
                                    stake_address
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Record a stake delegation
    /// Update historical_spo_state's delegators
    fn record_stake_delegation(&mut self, stake_address: &StakeAddress, spo: &KeyHash) {
        let Some(stake_addresses) = self.stake_addresses.as_ref() else {
            return;
        };
        let mut stake_addresses = stake_addresses.lock().unwrap();
        let old_spo = stake_addresses.get(stake_address).and_then(|s| s.delegated_spo.clone());

        if stake_addresses.record_stake_delegation(stake_address, spo) {
            // update historical_spos
            if let Some(historical_spos) = self.historical_spos.as_mut() {
                // Remove old delegator
                if let Some(old_spo) = old_spo.as_ref() {
                    match historical_spos.get_mut(old_spo) {
                        Some(historical_spo) => {
                            if let Some(removed) = historical_spo.remove_delegator(stake_address) {
                                if !removed {
                                    error!(
                                        "Historical SPO state for {} does not contain delegator {}",
                                        hex::encode(old_spo),
                                        stake_address
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
                if let Some(added) = historical_spo.add_delegator(stake_address) {
                    if !added {
                        error!(
                            "Historical SPO state for {} already contains delegator {}",
                            hex::encode(spo),
                            stake_address
                        );
                    }
                }
            }
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
            match &tx_cert.cert {
                // for spo_state
                TxCertificate::PoolRegistration(reg) => {
                    self.handle_pool_registration(
                        block,
                        reg,
                        &tx_cert.tx_identifier,
                        &tx_cert.cert_index,
                    );
                }
                TxCertificate::PoolRetirement(ret) => {
                    self.handle_pool_retirement(
                        block,
                        ret,
                        &tx_cert.tx_identifier,
                        &tx_cert.cert_index,
                    );
                }

                // for stake addresses
                TxCertificate::StakeRegistration(stake_address) => {
                    self.register_stake_address(stake_address);
                }
                TxCertificate::StakeDeregistration(stake_address) => {
                    self.deregister_stake_address(stake_address);
                }
                TxCertificate::Registration(reg) => {
                    self.register_stake_address(&reg.stake_address);
                    // we don't care deposite
                }
                TxCertificate::Deregistration(dreg) => {
                    self.deregister_stake_address(&dreg.stake_address);
                    // we don't care refund
                }
                TxCertificate::StakeDelegation(delegation) => {
                    self.record_stake_delegation(&delegation.stake_address, &delegation.operator);
                }
                TxCertificate::StakeAndVoteDelegation(delegation) => {
                    self.record_stake_delegation(&delegation.stake_address, &delegation.operator);
                    // don't care about vote delegation
                }
                TxCertificate::StakeRegistrationAndDelegation(delegation) => {
                    self.register_stake_address(&delegation.stake_address);
                    self.record_stake_delegation(&delegation.stake_address, &delegation.operator);
                }
                TxCertificate::StakeRegistrationAndVoteDelegation(delegation) => {
                    self.register_stake_address(&delegation.stake_address);
                    // don't care about vote delegation
                }
                TxCertificate::StakeRegistrationAndStakeAndVoteDelegation(delegation) => {
                    self.register_stake_address(&delegation.stake_address);
                    self.record_stake_delegation(&delegation.stake_address, &delegation.operator);
                    // don't care about vote delegation
                }
                _ => (),
            }
        }
        Ok(maybe_message)
    }

    pub fn handle_governance(
        &mut self,
        voting_procedures: &[(TxHash, VotingProcedures)],
    ) -> Result<()> {
        // when we save historical spo's vote
        let Some(historical_spos) = self.historical_spos.as_mut() else {
            return Ok(());
        };

        for (tx_hash, voting_procedures) in voting_procedures {
            for (voter, single_votes) in &voting_procedures.votes {
                let spo = match voter {
                    Voter::StakePoolKey(spo) => spo,
                    _ => continue,
                };

                let historical_spo = historical_spos
                    .entry(spo.clone())
                    .or_insert_with(|| HistoricalSPOState::new(&self.store_config));

                if let Some(votes) = historical_spo.votes.as_mut() {
                    for vp in single_votes.voting_procedures.values() {
                        votes.push(VoteRecord {
                            tx_hash: *tx_hash,
                            vote_index: vp.vote_index,
                            vote: vp.vote.clone(),
                        });
                    }
                }
            }
        }
        Ok(())
    }

    /// Handle withdrawals
    pub fn handle_withdrawals(&mut self, withdrawals_msg: &WithdrawalsMessage) -> Result<()> {
        let Some(stake_addresses) = self.stake_addresses.as_ref() else {
            return Ok(());
        };
        let mut stake_addresses = stake_addresses.lock().unwrap();
        for withdrawal in withdrawals_msg.withdrawals.iter() {
            stake_addresses.process_withdrawal(withdrawal);
        }

        Ok(())
    }

    /// Handle stake deltas
    pub fn handle_stake_deltas(&mut self, deltas_msg: &StakeAddressDeltasMessage) -> Result<()> {
        let Some(stake_addresses) = self.stake_addresses.as_ref() else {
            return Ok(());
        };
        let mut stake_addresses = stake_addresses.lock().unwrap();
        for delta in deltas_msg.deltas.iter() {
            stake_addresses.process_stake_delta(delta);
        }

        Ok(())
    }

    /// Handle Stake Reward Deltas
    pub fn handle_stake_reward_deltas(
        &mut self,
        _block_info: &BlockInfo,
        reward_deltas_msg: &StakeRewardDeltasMessage,
    ) -> Result<()> {
        let Some(stake_addresses) = self.stake_addresses.as_ref() else {
            return Ok(());
        };

        // Handle deltas
        for delta in reward_deltas_msg.deltas.iter() {
            let mut stake_addresses = stake_addresses.lock().unwrap();
            if let Err(e) = stake_addresses.update_reward(&delta.stake_address, delta.delta) {
                error!("Updating reward account {}: {e}", delta.stake_address);
            }
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
        PoolRetirement, Ratio, StakeAddress, TxCertificate, TxCertificateWithPos, TxIdentifier,
    };
    use tokio::sync::Mutex;

    fn default_pool_registration(
        operator: Vec<u8>,
        vrf_key_hash: Option<Vec<u8>>,
    ) -> PoolRegistration {
        PoolRegistration {
            operator: operator.clone(),
            vrf_key_hash: vrf_key_hash.unwrap_or_else(|| vec![0]),
            pledge: 0,
            cost: 0,
            margin: Ratio {
                numerator: 0,
                denominator: 0,
            },
            reward_account: StakeAddress::default(),
            pool_owners: vec![StakeAddress::default()],
            relays: vec![],
            pool_metadata: None,
        }
    }

    #[test]
    fn get_returns_none_on_empty_state() {
        let state = State::default();
        assert!(state.get(&vec![0]).is_none());
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
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRegistration(default_pool_registration(vec![0], None)),
            tx_identifier: TxIdentifier::default(),
            cert_index: 1,
        });
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
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRetirement(PoolRetirement {
                operator: vec![0],
                epoch: 1,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
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
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRetirement(PoolRetirement {
                operator: vec![0],
                epoch: 2,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
        assert!(state.handle_tx_certs(&block, &msg).is_ok());

        block.number = 1;
        msg = new_certs_msg();
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRetirement(PoolRetirement {
                operator: vec![1],
                epoch: 2,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
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
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRetirement(PoolRetirement {
                operator: vec![0],
                epoch: 2,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        history.lock().await.commit(block.number, state);

        let mut state = history.lock().await.get_current_state();
        block.number = 1;
        msg = new_certs_msg();
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRetirement(PoolRetirement {
                operator: vec![1],
                epoch: 2,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
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
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRegistration(default_pool_registration(vec![0], None)),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
        assert!(state.handle_tx_certs(&block, &msg).is_ok());

        assert_eq!(1, state.spos.len());
        let spo = state.spos.get(&vec![0u8]);
        assert!(!spo.is_none());

        block.number = 1;
        let mut msg = new_certs_msg();
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRetirement(PoolRetirement {
                operator: vec![0],
                epoch: 1,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
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
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRegistration(default_pool_registration(vec![0], None)),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        assert_eq!(1, state.spos.len());
        let spo = state.spos.get(&vec![0u8]);
        assert!(!spo.is_none());
        history.lock().await.commit(block.number, state);

        let mut state = history.lock().await.get_current_state();
        block.number = 1;
        msg = new_certs_msg();
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRetirement(PoolRetirement {
                operator: vec![0],
                epoch: 1,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
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
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRetirement(PoolRetirement {
                operator: vec![0],
                epoch: 2,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
        assert!(state.handle_tx_certs(&block, &msg).is_ok());

        block.number = 1;
        msg = new_certs_msg();
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRetirement(PoolRetirement {
                operator: vec![1],
                epoch: 3,
            }),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        let mut retiring_pools = state.get_retiring_pools();
        retiring_pools.sort_by_key(|p| p.epoch);
        assert_eq!(2, retiring_pools.len());
        assert_eq!(vec![0], retiring_pools[0].operator);
        assert_eq!(2, retiring_pools[0].epoch);
        assert_eq!(vec![1], retiring_pools[1].operator);
        assert_eq!(3, retiring_pools[1].epoch);
    }

    #[test]
    fn get_total_blocks_minted_returns_zeros_when_state_is_new() {
        let state = State::default();
        assert_eq!(0, state.get_total_blocks_minted_by_pools(&vec![vec![0]])[0]);
        assert_eq!(0, state.get_total_blocks_minted_by_pool(&vec![0]));
    }

    #[test]
    fn get_total_blocks_minted_returns_after_handle_mint() {
        let mut state = State::new(&save_blocks_store_config());
        let mut block = new_block(0);
        let mut msg = new_certs_msg();
        let spo_id = keyhash_224(&vec![1 as u8]);
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRegistration(default_pool_registration(spo_id.clone(), None)),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
        assert!(state.handle_tx_certs(&block, &msg).is_ok());

        block = new_block(2);
        assert_eq!(true, state.handle_mint(&block, &vec![1]));
        assert_eq!(1, state.get_total_blocks_minted_by_pool(&spo_id));

        block = new_block(3);
        assert_eq!(true, state.handle_mint(&block, &vec![1]));
        assert_eq!(2, state.get_total_blocks_minted_by_pools(&vec![spo_id])[0]);
    }

    #[test]
    fn get_blocks_returns_none_when_blocks_not_enabled() {
        let state = State::default();
        assert!(state.get_blocks_by_pool(&vec![0]).is_none());
    }

    #[test]
    fn handle_mint_returns_false_if_pool_not_found() {
        let mut state = State::new(&save_blocks_store_config());
        let block = new_block(0);
        assert_eq!(false, state.handle_mint(&block, &vec![0]));
    }

    #[test]
    fn get_blocks_return_data_after_handle_mint() {
        let mut state = State::new(&save_blocks_store_config());
        let mut block = new_block(0);
        let mut msg = new_certs_msg();
        let spo_id = keyhash_224(&vec![1 as u8]);
        msg.certificates.push(TxCertificateWithPos {
            cert: TxCertificate::PoolRegistration(default_pool_registration(spo_id.clone(), None)),
            tx_identifier: TxIdentifier::default(),
            cert_index: 0,
        });
        assert!(state.handle_tx_certs(&block, &msg).is_ok());
        block = new_block(2);
        assert_eq!(true, state.handle_mint(&block, &vec![1])); // Note raw issuer_vkey
        let blocks = state.get_blocks_by_pool(&spo_id).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], block.number);

        let blocks = state.get_blocks_by_pool_and_epoch(&spo_id, 2).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0], block.number);

        assert!(state.get_blocks_by_pool_and_epoch(&spo_id, 3).is_none());
    }
}
