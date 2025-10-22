use std::{collections::HashMap, path::Path};

use acropolis_common::{ShelleyAddress, StakeCredential};
use anyhow::Result;
use fjall::{Keyspace, Partition, PartitionCreateOptions};
use minicbor::{decode, to_vec};
use tokio::sync::Mutex;
use tracing::{debug, error, info};

use crate::state::{
    AccountEntry, AccountWithdrawal, ActiveStakeHistory, DelegationUpdate,
    HistoricalAccountsConfig, RegistrationUpdate, RewardHistory,
};

pub struct ImmutableHistoricalAccountStore {
    rewards_history: Partition,
    active_stake_history: Partition,
    delegation_history: Partition,
    registration_history: Partition,
    withdrawal_history: Partition,
    mir_history: Partition,
    addresses: Partition,
    keyspace: Keyspace,
    pub pending: Mutex<Vec<HashMap<StakeCredential, AccountEntry>>>,
}

impl ImmutableHistoricalAccountStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let cfg = fjall::Config::new(path).max_write_buffer_size(512 * 1024 * 1024).temporary(true);
        let keyspace = Keyspace::open(cfg)?;

        let rewards_history =
            keyspace.open_partition("rewards_history", PartitionCreateOptions::default())?;
        let active_stake_history =
            keyspace.open_partition("active_stake_history", PartitionCreateOptions::default())?;
        let delegation_history =
            keyspace.open_partition("delegation_history", PartitionCreateOptions::default())?;
        let registration_history =
            keyspace.open_partition("registration_history", PartitionCreateOptions::default())?;
        let withdrawal_history =
            keyspace.open_partition("withdrawal_history", PartitionCreateOptions::default())?;
        let mir_history =
            keyspace.open_partition("mir_history", PartitionCreateOptions::default())?;
        let addresses = keyspace.open_partition("addresses", PartitionCreateOptions::default())?;

        Ok(Self {
            rewards_history,
            active_stake_history,
            delegation_history,
            registration_history,
            withdrawal_history,
            mir_history,
            addresses,
            keyspace,
            pending: Mutex::new(Vec::new()),
        })
    }

    /// Persists volatile UTxOs, transactions, and totals into their respective Fjall partitions for an entire epoch.
    /// Skips any partitions that have already stored the given epoch.
    /// All writes are batched and committed atomically, preventing on-disk corruption in case of failure.
    pub async fn persist_epoch(&self, epoch: u32, config: &HistoricalAccountsConfig) -> Result<()> {
        if !(config.store_rewards_history
            || config.store_active_stake_history
            || config.store_delegation_history
            || config.store_registration_history
            || config.store_withdrawal_history
            || config.store_mir_history
            || config.store_addresses)
        {
            debug!("no persistence needed for epoch {epoch} (disabled)",);
            return Ok(());
        }

        let drained_blocks = {
            let mut pending = self.pending.lock().await;
            std::mem::take(&mut *pending)
        };

        let mut batch = self.keyspace.batch();
        let mut change_count = 0;

        for block_map in drained_blocks.into_iter() {
            if block_map.is_empty() {
                continue;
            }

            for (account, entry) in block_map {
                let epoch_key = Self::make_epoch_key(&account, epoch);
                change_count += 1;

                // Persist rewards
                if config.store_rewards_history {
                    batch.insert(
                        &self.rewards_history,
                        &epoch_key,
                        to_vec(&entry.reward_history)?,
                    );
                }

                // Persist active stake
                if config.store_active_stake_history {
                    batch.insert(
                        &self.active_stake_history,
                        &epoch_key,
                        to_vec(&entry.active_stake_history)?,
                    );
                }

                // Persist account delegation updates
                if config.store_delegation_history {
                    if let Some(updates) = &entry.delegation_history {
                        batch.insert(&self.delegation_history, &epoch_key, to_vec(&updates)?);
                    }
                }

                // Persist account registration updates
                if config.store_registration_history {
                    if let Some(updates) = &entry.registration_history {
                        batch.insert(&self.registration_history, &epoch_key, to_vec(&updates)?);
                    }
                }

                // Persist withdrawal updates
                if config.store_withdrawal_history {
                    if let Some(updates) = &entry.withdrawal_history {
                        batch.insert(&self.withdrawal_history, &epoch_key, to_vec(&updates)?);
                    }
                }

                // Persist MIR updates
                if config.store_mir_history {
                    if let Some(updates) = &entry.mir_history {
                        batch.insert(&self.mir_history, &epoch_key, to_vec(&updates)?);
                    }
                }

                // Persist address updates
                // TODO: Deduplicate addresses across epochs
                if config.store_addresses {
                    if let Some(updates) = &entry.addresses {
                        batch.insert(&self.addresses, &epoch_key, to_vec(&updates)?);
                    }
                }
            }
        }

        match batch.commit() {
            Ok(_) => {
                info!("committed {change_count} address changes for epoch {epoch}");
                Ok(())
            }
            Err(e) => {
                error!("batch commit failed for epoch {epoch}: {e}");
                Err(e.into())
            }
        }
    }

    pub async fn update_immutable(&self, drained: Vec<HashMap<StakeCredential, AccountEntry>>) {
        let mut pending = self.pending.lock().await;
        pending.extend(drained);
    }

    pub async fn _get_rewards_history(
        &self,
        account: &StakeCredential,
    ) -> Result<Option<Vec<RewardHistory>>> {
        let account_key = account.get_hash();

        let mut immutable_rewards = Vec::<RewardHistory>::new();

        for result in self.rewards_history.prefix(&account_key) {
            let (_key, bytes) = result?;
            let epoch_rewards: Vec<RewardHistory> = decode(&bytes)?;
            immutable_rewards.extend(epoch_rewards);
        }

        for block_map in self.pending.lock().await.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(deltas) = &entry.reward_history {
                    immutable_rewards.extend(deltas.clone());
                }
            }
        }

        if immutable_rewards.is_empty() {
            Ok(None)
        } else {
            Ok(Some(immutable_rewards))
        }
    }

    pub async fn _get_active_stake_history(
        &self,
        account: &StakeCredential,
    ) -> Result<Option<Vec<ActiveStakeHistory>>> {
        let account_key = account.get_hash();

        let mut immutable_active_stake = Vec::<ActiveStakeHistory>::new();

        for result in self.active_stake_history.prefix(&account_key) {
            let (_key, bytes) = result?;
            let epoch_stakes: Vec<ActiveStakeHistory> = decode(&bytes)?;
            immutable_active_stake.extend(epoch_stakes);
        }

        for block_map in self.pending.lock().await.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(deltas) = &entry.active_stake_history {
                    immutable_active_stake.extend(deltas.clone());
                }
            }
        }

        if immutable_active_stake.is_empty() {
            Ok(None)
        } else {
            Ok(Some(immutable_active_stake))
        }
    }

    pub async fn _get_delegation_history(
        &self,
        account: &StakeCredential,
    ) -> Result<Option<Vec<DelegationUpdate>>> {
        let account_key = account.get_hash();

        let mut immutable_delegations = Vec::<DelegationUpdate>::new();

        for result in self.delegation_history.prefix(&account_key) {
            let (_key, bytes) = result?;
            let epoch_delegations: Vec<DelegationUpdate> = decode(&bytes)?;
            immutable_delegations.extend(epoch_delegations);
        }

        for block_map in self.pending.lock().await.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(updates) = &entry.delegation_history {
                    immutable_delegations.extend(updates.iter().cloned());
                }
            }
        }

        if immutable_delegations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(immutable_delegations))
        }
    }

    pub async fn _get_registration_history(
        &self,
        account: &StakeCredential,
    ) -> Result<Option<Vec<RegistrationUpdate>>> {
        let account_key = account.get_hash();

        let mut immutable_registrations = Vec::<RegistrationUpdate>::new();

        for result in self.registration_history.prefix(&account_key) {
            let (_key, bytes) = result?;
            let epoch_registrations: Vec<RegistrationUpdate> = decode(&bytes)?;
            immutable_registrations.extend(epoch_registrations);
        }

        for block_map in self.pending.lock().await.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(updates) = &entry.registration_history {
                    immutable_registrations.extend(updates.iter().cloned());
                }
            }
        }

        if immutable_registrations.is_empty() {
            Ok(None)
        } else {
            Ok(Some(immutable_registrations))
        }
    }

    pub async fn _get_withdrawal_history(
        &self,
        account: &StakeCredential,
    ) -> Result<Option<Vec<AccountWithdrawal>>> {
        let account_key = account.get_hash();

        let mut immutable_withdrawals = Vec::<AccountWithdrawal>::new();

        for result in self.withdrawal_history.prefix(&account_key) {
            let (_key, bytes) = result?;
            let epoch_withdrawals: Vec<AccountWithdrawal> = decode(&bytes)?;
            immutable_withdrawals.extend(epoch_withdrawals);
        }

        for block_map in self.pending.lock().await.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(updates) = &entry.withdrawal_history {
                    immutable_withdrawals.extend(updates.iter().cloned());
                }
            }
        }

        if immutable_withdrawals.is_empty() {
            Ok(None)
        } else {
            Ok(Some(immutable_withdrawals))
        }
    }

    pub async fn _get_mir_history(
        &self,
        account: &StakeCredential,
    ) -> Result<Option<Vec<AccountWithdrawal>>> {
        let account_key = account.get_hash();

        let mut immutable_mirs = Vec::<AccountWithdrawal>::new();

        for result in self.mir_history.prefix(&account_key) {
            let (_key, bytes) = result?;
            let epoch_mirs: Vec<AccountWithdrawal> = decode(&bytes)?;
            immutable_mirs.extend(epoch_mirs);
        }

        for block_map in self.pending.lock().await.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(updates) = &entry.mir_history {
                    immutable_mirs.extend(updates.iter().cloned());
                }
            }
        }

        if immutable_mirs.is_empty() {
            Ok(None)
        } else {
            Ok(Some(immutable_mirs))
        }
    }

    pub async fn _get_addresses(
        &self,
        account: &StakeCredential,
    ) -> Result<Option<Vec<ShelleyAddress>>> {
        let account_key = account.get_hash();

        let mut immutable_addresses = Vec::<ShelleyAddress>::new();

        for result in self.addresses.prefix(&account_key) {
            let (_key, bytes) = result?;
            let epoch_addresses: Vec<ShelleyAddress> = decode(&bytes)?;
            immutable_addresses.extend(epoch_addresses);
        }

        for block_map in self.pending.lock().await.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(updates) = &entry.addresses {
                    immutable_addresses.extend(updates.iter().cloned());
                }
            }
        }

        if immutable_addresses.is_empty() {
            Ok(None)
        } else {
            Ok(Some(immutable_addresses))
        }
    }

    // Used for once per epoch data (rewards & active stake)
    fn make_epoch_key(account: &StakeCredential, epoch: u32) -> [u8; 32] {
        let mut key = [0u8; 32];
        key[..28].copy_from_slice(&account.get_hash());
        key[28..32].copy_from_slice(&epoch.to_be_bytes());
        key
    }
}
