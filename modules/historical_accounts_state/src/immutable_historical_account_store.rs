use std::{collections::HashMap, path::Path};

use acropolis_common::{ShelleyAddress, StakeAddress};
use anyhow::Result;
use fjall::{Keyspace, Partition, PartitionCreateOptions};
use minicbor::{decode, to_vec};
use tokio::{sync::Mutex, task};
use tracing::{debug, error, info};

use crate::state::{
    AccountEntry, AccountWithdrawal, ActiveStakeHistory, DelegationUpdate,
    HistoricalAccountsConfig, RegistrationUpdate, RewardHistory,
};

// Metadata keys which store the last epoch saved in each partition
const ACCOUNT_REWARDS_HISTORY_COUNTER: &[u8] = b"rewards_history_epoch_last";
const ACCOUNT_ACTIVE_STAKE_HISTORY_COUNTER: &[u8] = b"active_stake_history_epoch_last";
const ACCOUNT_DELEGATION_HISTORY_COUNTER: &[u8] = b"delegation_history_epoch_last";
const ACCOUNT_REGISTRATION_HISTORY_COUNTER: &[u8] = b"registration_history_epoch_last";
const ACCOUNT_WITHDRAWAL_HISTORY_COUNTER: &[u8] = b"withdrawal_history_epoch_last";
const ACCOUNT_MIR_HISTORY_COUNTER: &[u8] = b"mir_history_epoch_last";
const ACCOUNT_ADDRESSES_COUNTER: &[u8] = b"addresses_epoch_last";

pub struct ImmutableHistoricalAccountStore {
    rewards_history: Partition,
    active_stake_history: Partition,
    delegation_history: Partition,
    registration_history: Partition,
    withdrawal_history: Partition,
    mir_history: Partition,
    addresses: Partition,
    keyspace: Keyspace,
    pub pending: Mutex<Vec<HashMap<StakeAddress, AccountEntry>>>,
}

impl ImmutableHistoricalAccountStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let cfg = fjall::Config::new(path).max_write_buffer_size(512 * 1024 * 1024);
        let keyspace = Keyspace::open(cfg)?;

        let rewards_history = keyspace
            .open_partition("account_rewards_history", PartitionCreateOptions::default())?;
        let active_stake_history = keyspace.open_partition(
            "account_active_stake_history",
            PartitionCreateOptions::default(),
        )?;
        let delegation_history = keyspace.open_partition(
            "account_delegation_history",
            PartitionCreateOptions::default(),
        )?;
        let registration_history = keyspace.open_partition(
            "account_registration_history",
            PartitionCreateOptions::default(),
        )?;
        let withdrawal_history = keyspace.open_partition(
            "account_withdrawal_history",
            PartitionCreateOptions::default(),
        )?;
        let mir_history =
            keyspace.open_partition("account_mir_history", PartitionCreateOptions::default())?;
        let addresses =
            keyspace.open_partition("account_addresses", PartitionCreateOptions::default())?;

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
    pub async fn persist_epoch(&self, epoch: u64, config: &HistoricalAccountsConfig) -> Result<()> {
        let persist_rewards_history = config.store_rewards_history
            && !self
                .epoch_exists(
                    self.rewards_history.clone(),
                    ACCOUNT_REWARDS_HISTORY_COUNTER,
                    epoch,
                )
                .await?;
        let persist_active_stake_history = config.store_active_stake_history
            && !self
                .epoch_exists(
                    self.active_stake_history.clone(),
                    ACCOUNT_ACTIVE_STAKE_HISTORY_COUNTER,
                    epoch,
                )
                .await?;
        let persist_delegation_history = config.store_delegation_history
            && !self
                .epoch_exists(
                    self.delegation_history.clone(),
                    ACCOUNT_DELEGATION_HISTORY_COUNTER,
                    epoch,
                )
                .await?;
        let persist_registration_history = config.store_registration_history
            && !self
                .epoch_exists(
                    self.registration_history.clone(),
                    ACCOUNT_REGISTRATION_HISTORY_COUNTER,
                    epoch,
                )
                .await?;
        let persist_withdrawal_history = config.store_withdrawal_history
            && !self
                .epoch_exists(
                    self.withdrawal_history.clone(),
                    ACCOUNT_WITHDRAWAL_HISTORY_COUNTER,
                    epoch,
                )
                .await?;
        let persist_mir_history = config.store_mir_history
            && !self
                .epoch_exists(self.mir_history.clone(), ACCOUNT_MIR_HISTORY_COUNTER, epoch)
                .await?;
        let persist_addresses = config.store_addresses
            && !self.epoch_exists(self.addresses.clone(), ACCOUNT_ADDRESSES_COUNTER, epoch).await?;

        if !(persist_rewards_history
            || persist_active_stake_history
            || persist_delegation_history
            || persist_registration_history
            || persist_withdrawal_history
            || persist_mir_history
            || persist_addresses)
        {
            debug!("no persistence needed for epoch {epoch} (already persisted or disabled)",);
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
                change_count += 1;
                let account_key = account.to_bytes_key()?;

                // Persist rewards
                if persist_rewards_history {
                    let mut live: Vec<RewardHistory> = self
                        .rewards_history
                        .get(&account_key)?
                        .map(|bytes| decode(&bytes))
                        .transpose()?
                        .unwrap_or_default();

                    if let Some(rewards) = &entry.reward_history {
                        for reward in rewards {
                            live.push(reward.clone())
                        }
                    }

                    batch.insert(&self.rewards_history, &account_key, to_vec(&live)?);
                }

                // Persist active stake
                if persist_active_stake_history {
                    let mut live: Vec<ActiveStakeHistory> = self
                        .active_stake_history
                        .get(&account_key)?
                        .map(|bytes| decode(&bytes))
                        .transpose()?
                        .unwrap_or_default();

                    if let Some(deltas) = &entry.active_stake_history {
                        for delta in deltas {
                            live.push(delta.clone());
                        }
                    }

                    batch.insert(&self.active_stake_history, &account_key, to_vec(&live)?);
                }

                // Persist account delegation updates
                if persist_delegation_history {
                    let mut live: Vec<DelegationUpdate> = self
                        .delegation_history
                        .get(&account_key)?
                        .map(|bytes| decode(&bytes))
                        .transpose()?
                        .unwrap_or_default();

                    if let Some(updates) = &entry.delegation_history {
                        live.extend(updates.iter().cloned());
                    }

                    batch.insert(&self.delegation_history, &account_key, to_vec(&live)?);
                }

                // Persist account registration updates
                if persist_registration_history {
                    let mut live: Vec<RegistrationUpdate> = self
                        .registration_history
                        .get(&account_key)?
                        .map(|bytes| decode(&bytes))
                        .transpose()?
                        .unwrap_or_default();

                    if let Some(updates) = &entry.registration_history {
                        live.extend(updates.iter().cloned());
                    }

                    batch.insert(&self.registration_history, &account_key, to_vec(&live)?);
                }

                // Persist withdrawal updates
                if persist_withdrawal_history {
                    let mut live: Vec<AccountWithdrawal> = self
                        .withdrawal_history
                        .get(&account_key)?
                        .map(|bytes| decode(&bytes))
                        .transpose()?
                        .unwrap_or_default();

                    if let Some(updates) = &entry.withdrawal_history {
                        live.extend(updates.iter().cloned());
                    }

                    batch.insert(&self.withdrawal_history, &account_key, to_vec(&live)?);
                }

                // Persist MIR updates
                if persist_mir_history {
                    let mut live: Vec<AccountWithdrawal> = self
                        .mir_history
                        .get(&account_key)?
                        .map(|bytes| decode(&bytes))
                        .transpose()?
                        .unwrap_or_default();

                    if let Some(updates) = &entry.mir_history {
                        live.extend(updates.iter().cloned());
                    }

                    batch.insert(&self.mir_history, &account_key, to_vec(&live)?);
                }

                // Persist address updates
                if persist_addresses {
                    let mut live: Vec<ShelleyAddress> = self
                        .addresses
                        .get(&account_key)?
                        .map(|bytes| decode(&bytes))
                        .transpose()?
                        .unwrap_or_default();

                    if let Some(updates) = &entry.addresses {
                        live.extend(updates.iter().cloned());
                    }

                    batch.insert(&self.addresses, &account_key, to_vec(&live)?);
                }
            }
        }

        // Metadata markers
        if persist_active_stake_history {
            batch.insert(
                &self.active_stake_history,
                ACCOUNT_ACTIVE_STAKE_HISTORY_COUNTER,
                &epoch.to_le_bytes(),
            );
        }
        if persist_delegation_history {
            batch.insert(
                &self.delegation_history,
                ACCOUNT_DELEGATION_HISTORY_COUNTER,
                &epoch.to_le_bytes(),
            );
        }
        if persist_registration_history {
            batch.insert(
                &self.registration_history,
                ACCOUNT_REGISTRATION_HISTORY_COUNTER,
                &epoch.to_le_bytes(),
            );
        }
        if persist_withdrawal_history {
            batch.insert(
                &self.withdrawal_history,
                ACCOUNT_WITHDRAWAL_HISTORY_COUNTER,
                &epoch.to_le_bytes(),
            );
        }
        if persist_mir_history {
            batch.insert(
                &self.mir_history,
                ACCOUNT_MIR_HISTORY_COUNTER,
                &epoch.to_le_bytes(),
            );
        }
        if persist_addresses {
            batch.insert(
                &self.addresses,
                ACCOUNT_ADDRESSES_COUNTER,
                &epoch.to_le_bytes(),
            );
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

    pub async fn update_immutable(&self, drained: Vec<HashMap<StakeAddress, AccountEntry>>) {
        let mut pending = self.pending.lock().await;
        pending.extend(drained);
    }

    pub async fn _get_rewards_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<RewardHistory>>> {
        let key = account.to_bytes_key()?;

        let mut live: Vec<RewardHistory> = self
            .rewards_history
            .get(&key)?
            .map(|bytes| decode(&bytes))
            .transpose()?
            .unwrap_or_default();

        let pending = self.pending.lock().await;
        for block_map in pending.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(deltas) = &entry.reward_history {
                    for delta in deltas {
                        live.push(delta.clone());
                    }
                }
            }
        }

        if live.is_empty() {
            Ok(None)
        } else {
            let vec: Vec<_> = live.into_iter().collect();
            Ok(Some(vec))
        }
    }

    pub async fn _get_active_stake_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<ActiveStakeHistory>>> {
        let key = account.to_bytes_key()?;

        let mut live: Vec<ActiveStakeHistory> = self
            .active_stake_history
            .get(&key)?
            .map(|bytes| decode(&bytes))
            .transpose()?
            .unwrap_or_default();

        let pending = self.pending.lock().await;
        for block_map in pending.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(deltas) = &entry.active_stake_history {
                    for delta in deltas {
                        live.push(delta.clone());
                    }
                }
            }
        }

        if live.is_empty() {
            Ok(None)
        } else {
            let vec: Vec<_> = live.into_iter().collect();
            Ok(Some(vec))
        }
    }

    pub async fn _get_delegation_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<DelegationUpdate>>> {
        let key = account.to_bytes_key()?;
        let mut live: Vec<DelegationUpdate> = self
            .delegation_history
            .get(&key)?
            .map(|bytes| decode(&bytes))
            .transpose()?
            .unwrap_or_default();

        let pending = self.pending.lock().await;
        for block_map in pending.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(updates) = &entry.delegation_history {
                    live.extend(updates.iter().cloned());
                }
            }
        }

        if live.is_empty() {
            Ok(None)
        } else {
            Ok(Some(live))
        }
    }

    pub async fn _get_registration_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<RegistrationUpdate>>> {
        let key = account.to_bytes_key()?;
        let mut live: Vec<RegistrationUpdate> = self
            .registration_history
            .get(&key)?
            .map(|bytes| decode(&bytes))
            .transpose()?
            .unwrap_or_default();

        let pending = self.pending.lock().await;
        for block_map in pending.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(updates) = &entry.registration_history {
                    live.extend(updates.iter().cloned());
                }
            }
        }

        if live.is_empty() {
            Ok(None)
        } else {
            Ok(Some(live))
        }
    }

    pub async fn _get_withdrawal_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<AccountWithdrawal>>> {
        let key = account.to_bytes_key()?;
        let mut live: Vec<AccountWithdrawal> = self
            .withdrawal_history
            .get(&key)?
            .map(|bytes| decode(&bytes))
            .transpose()?
            .unwrap_or_default();

        let pending = self.pending.lock().await;
        for block_map in pending.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(updates) = &entry.withdrawal_history {
                    live.extend(updates.iter().cloned());
                }
            }
        }

        if live.is_empty() {
            Ok(None)
        } else {
            Ok(Some(live))
        }
    }

    pub async fn _get_mir_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<AccountWithdrawal>>> {
        let key = account.to_bytes_key()?;
        let mut live: Vec<AccountWithdrawal> = self
            .mir_history
            .get(&key)?
            .map(|bytes| decode(&bytes))
            .transpose()?
            .unwrap_or_default();

        let pending = self.pending.lock().await;
        for block_map in pending.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(updates) = &entry.mir_history {
                    live.extend(updates.iter().cloned());
                }
            }
        }

        if live.is_empty() {
            Ok(None)
        } else {
            Ok(Some(live))
        }
    }

    pub async fn _get_addresses(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<ShelleyAddress>>> {
        let key = account.to_bytes_key()?;
        let mut live: Vec<ShelleyAddress> =
            self.addresses.get(&key)?.map(|bytes| decode(&bytes)).transpose()?.unwrap_or_default();

        let pending = self.pending.lock().await;
        for block_map in pending.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(updates) = &entry.addresses {
                    live.extend(updates.iter().cloned());
                }
            }
        }

        if live.is_empty() {
            Ok(None)
        } else {
            Ok(Some(live))
        }
    }

    pub async fn get_last_epoch_stored(&self) -> Result<Option<u64>> {
        let read_marker = |partition: Partition, key: &'static [u8]| async move {
            task::spawn_blocking(move || {
                Ok::<_, anyhow::Error>(match partition.get(key)? {
                    Some(bytes) if bytes.len() == 8 => {
                        let mut arr = [0u8; 8];
                        arr.copy_from_slice(&bytes);
                        let val = u64::from_le_bytes(arr);
                        if val == u64::MAX {
                            None
                        } else {
                            Some(val)
                        }
                    }
                    _ => None,
                })
            })
            .await?
        };

        let ash = read_marker(
            self.active_stake_history.clone(),
            ACCOUNT_ACTIVE_STAKE_HISTORY_COUNTER,
        )
        .await?;
        let dh = read_marker(
            self.delegation_history.clone(),
            ACCOUNT_DELEGATION_HISTORY_COUNTER,
        )
        .await?;
        let rh = read_marker(
            self.registration_history.clone(),
            ACCOUNT_REGISTRATION_HISTORY_COUNTER,
        )
        .await?;
        let wh = read_marker(
            self.withdrawal_history.clone(),
            ACCOUNT_WITHDRAWAL_HISTORY_COUNTER,
        )
        .await?;
        let mh = read_marker(self.mir_history.clone(), ACCOUNT_MIR_HISTORY_COUNTER).await?;
        let a = read_marker(self.addresses.clone(), ACCOUNT_ADDRESSES_COUNTER).await?;
        let min_epoch = [ash, dh, rh, wh, mh, a].into_iter().flatten().min();

        if let Some(epoch) = min_epoch {
            info!("last epoch already stored across partitions: {epoch}");
        } else {
            info!("no epoch markers found across partitions");
        }

        Ok(min_epoch)
    }

    async fn epoch_exists(
        &self,
        partition: Partition,
        key: &'static [u8],
        epoch: u64,
    ) -> Result<bool> {
        let exists = task::spawn_blocking(move || -> Result<bool> {
            let bytes = match partition.get(key)? {
                Some(b) if b.len() == 8 => b,
                _ => return Ok(false),
            };

            let mut arr = [0u8; 8];
            arr.copy_from_slice(&bytes);
            let last_epoch = u64::from_le_bytes(arr);

            Ok(epoch <= last_epoch)
        })
        .await??;

        if exists {
            let key_name = std::str::from_utf8(key)
                .map(|s| s.to_string())
                .unwrap_or_else(|_| format!("{:?}", key));
            info!("epoch {epoch} already stored for {key_name}");
        }

        Ok(exists)
    }
}
