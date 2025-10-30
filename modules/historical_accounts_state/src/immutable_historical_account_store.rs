use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    path::Path,
};

use acropolis_common::{
    queries::accounts::{AccountReward, AccountWithdrawal, DelegationUpdate, RegistrationUpdate},
    ShelleyAddress, StakeAddress,
};
use anyhow::Result;
use fjall::{Keyspace, Partition, PartitionCreateOptions};
use minicbor::{decode, to_vec};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use tokio::sync::Mutex;
use tracing::{debug, error};

use crate::state::{AccountEntry, ActiveStakeHistory, HistoricalAccountsConfig};

pub struct ImmutableHistoricalAccountStore {
    rewards_history: Partition,
    active_stake_history: Partition,
    delegation_history: Partition,
    registration_history: Partition,
    mir_history: Partition,
    withdrawal_history: Partition,
    addresses: Partition,
    keyspace: Keyspace,
    pub pending: Mutex<Vec<HashMap<StakeAddress, AccountEntry>>>,
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

    /// Persists volatile registrations, delegations, MIRs, withdrawals, rewards,
    /// and addresses into their respective Fjall partitions for an entire epoch.
    /// Skips any partitions that have already stored the given epoch.
    /// All writes are batched and committed atomically, preventing on-disk corruption in case of failure.
    pub async fn persist_epoch(
        &self,
        epoch: u32,
        config: &HistoricalAccountsConfig,
    ) -> Result<u64> {
        let drained_blocks = {
            let mut pending = self.pending.lock().await;
            std::mem::take(&mut *pending)
        };

        if !config.any_enabled() {
            debug!("no persistence needed for epoch {epoch} (disabled)",);
            return Ok(0);
        }

        let mut batch = self.keyspace.batch();
        let mut change_count = 0;

        for (account, entry) in Self::merge_block_deltas(drained_blocks) {
            let epoch_key = Self::make_epoch_key(&account, epoch);
            change_count += 1;

            // Persist rewards
            if config.store_rewards_history {
                let rewards = entry.reward_history.clone().unwrap_or_default();
                batch.insert(&self.rewards_history, epoch_key, to_vec(&rewards)?);
            }

            // Persist active stake
            if config.store_active_stake_history {
                batch.insert(
                    &self.active_stake_history,
                    epoch_key,
                    to_vec(&entry.active_stake_history)?,
                );
            }

            // Persist account delegation updates
            if config.store_delegation_history {
                if let Some(updates) = &entry.delegation_history {
                    batch.insert(&self.delegation_history, epoch_key, to_vec(updates)?);
                }
            }

            // Persist account registration updates
            if config.store_registration_history {
                if let Some(updates) = &entry.registration_history {
                    batch.insert(&self.registration_history, epoch_key, to_vec(updates)?);
                }
            }

            // Persist withdrawal updates
            if config.store_withdrawal_history {
                if let Some(updates) = &entry.withdrawal_history {
                    batch.insert(&self.withdrawal_history, epoch_key, to_vec(updates)?);
                }
            }

            // Persist MIR updates
            if config.store_mir_history {
                if let Some(updates) = &entry.mir_history {
                    batch.insert(&self.mir_history, epoch_key, to_vec(updates)?);
                }
            }

            // Persist address updates
            if config.store_addresses {
                if let Some(updates) = &entry.addresses {
                    for address in updates {
                        let address_key = Self::make_address_key(&account, address.clone());
                        batch.insert(&self.addresses, address_key, &[]);
                    }
                }
            }
        }

        match batch.commit() {
            Ok(_) => Ok(change_count),
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

    pub async fn get_reward_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<AccountReward>>> {
        let mut immutable_rewards =
            self.collect_partition::<AccountReward>(&self.rewards_history, account.get_hash())?;

        self.merge_pending(
            account,
            |e| e.reward_history.as_ref(),
            &mut immutable_rewards,
        )
        .await;

        Ok((!immutable_rewards.is_empty()).then_some(immutable_rewards))
    }

    pub async fn _get_active_stake_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<ActiveStakeHistory>>> {
        let mut immutable_active_stake = self.collect_partition::<ActiveStakeHistory>(
            &self.active_stake_history,
            account.get_hash(),
        )?;

        self.merge_pending(
            account,
            |e| e.active_stake_history.as_ref(),
            &mut immutable_active_stake,
        )
        .await;

        Ok((!immutable_active_stake.is_empty()).then_some(immutable_active_stake))
    }

    pub async fn get_registration_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<RegistrationUpdate>>> {
        let mut immutable_registrations = self.collect_partition::<RegistrationUpdate>(
            &self.registration_history,
            account.get_hash(),
        )?;

        self.merge_pending(
            account,
            |e| e.registration_history.as_ref(),
            &mut immutable_registrations,
        )
        .await;

        Ok((!immutable_registrations.is_empty()).then_some(immutable_registrations))
    }

    pub async fn get_delegation_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<DelegationUpdate>>> {
        let mut immutable_delegations = self
            .collect_partition::<DelegationUpdate>(&self.delegation_history, account.get_hash())?;

        self.merge_pending(
            account,
            |e| e.delegation_history.as_ref(),
            &mut immutable_delegations,
        )
        .await;

        Ok((!immutable_delegations.is_empty()).then_some(immutable_delegations))
    }

    pub async fn get_mir_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<AccountWithdrawal>>> {
        let mut immutable_mirs =
            self.collect_partition::<AccountWithdrawal>(&self.mir_history, account.get_hash())?;

        self.merge_pending(account, |e| e.mir_history.as_ref(), &mut immutable_mirs).await;

        Ok((!immutable_mirs.is_empty()).then_some(immutable_mirs))
    }

    pub async fn get_withdrawal_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<AccountWithdrawal>>> {
        let mut immutable_withdrawals = self
            .collect_partition::<AccountWithdrawal>(&self.withdrawal_history, account.get_hash())?;

        self.merge_pending(
            account,
            |e| e.withdrawal_history.as_ref(),
            &mut immutable_withdrawals,
        )
        .await;

        Ok((!immutable_withdrawals.is_empty()).then_some(immutable_withdrawals))
    }

    pub async fn get_addresses(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<HashSet<ShelleyAddress>>> {
        let prefix = account.get_hash();
        let mut addresses: HashSet<ShelleyAddress> = HashSet::new();

        for result in self.addresses.prefix(&prefix) {
            let (key, _) = result?;
            let shelley = ShelleyAddress::from_bytes_key(&key[prefix.len()..])?;
            addresses.insert(shelley);
        }

        for block_map in self.pending.lock().await.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(addrs) = &entry.addresses {
                    addresses.extend(addrs.iter().cloned());
                }
            }
        }

        Ok((!addresses.is_empty()).then_some(addresses))
    }

    fn merge_block_deltas(
        block_deltas: Vec<HashMap<StakeAddress, AccountEntry>>,
    ) -> HashMap<StakeAddress, AccountEntry> {
        block_deltas.into_par_iter().reduce(HashMap::new, |mut acc, block_map| {
            for (account, entry) in block_map {
                let agg_entry = acc.entry(account).or_default();

                Self::extend_opt_vec(&mut agg_entry.reward_history, entry.reward_history);
                Self::extend_opt_vec(
                    &mut agg_entry.active_stake_history,
                    entry.active_stake_history,
                );
                Self::extend_opt_vec(&mut agg_entry.delegation_history, entry.delegation_history);
                Self::extend_opt_vec(
                    &mut agg_entry.registration_history,
                    entry.registration_history,
                );
                Self::extend_opt_vec(&mut agg_entry.withdrawal_history, entry.withdrawal_history);
                Self::extend_opt_vec(&mut agg_entry.mir_history, entry.mir_history);
                Self::extend_opt_set(&mut agg_entry.addresses, entry.addresses);
            }
            acc
        })
    }

    fn collect_partition<T>(&self, partition: &Partition, prefix: &[u8]) -> Result<Vec<T>>
    where
        T: for<'a> minicbor::Decode<'a, ()>,
    {
        let mut out = Vec::new();
        for result in partition.prefix(prefix) {
            let (_key, bytes) = result?;
            let vals: Vec<T> = decode(&bytes)?;
            out.extend(vals);
        }
        Ok(out)
    }

    async fn merge_pending<T, F>(&self, account: &StakeAddress, f: F, out: &mut Vec<T>)
    where
        F: Fn(&AccountEntry) -> Option<&Vec<T>>,
        T: Clone,
    {
        for block_map in self.pending.lock().await.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(data) = f(entry) {
                    out.extend(data.iter().cloned());
                }
            }
        }
    }

    fn make_epoch_key(account: &StakeAddress, epoch: u32) -> [u8; 32] {
        let mut key = [0u8; 32];
        key[..28].copy_from_slice(&account.get_credential().get_hash());
        key[28..32].copy_from_slice(&epoch.to_be_bytes());
        key
    }

    fn make_address_key(account: &StakeAddress, address: ShelleyAddress) -> Vec<u8> {
        let mut key = account.get_credential().get_hash();
        key.extend(address.to_bytes_key());
        key
    }

    fn extend_opt_vec<T>(target: &mut Option<Vec<T>>, src: Option<Vec<T>>) {
        if let Some(mut v) = src {
            if !v.is_empty() {
                target.get_or_insert_with(Vec::new).append(&mut v);
            }
        }
    }

    fn extend_opt_set<T>(target: &mut Option<HashSet<T>>, src: Option<HashSet<T>>)
    where
        T: Eq + Hash,
    {
        if let Some(mut s) = src {
            if !s.is_empty() {
                target.get_or_insert_with(HashSet::new).extend(s.drain());
            }
        }
    }
}
