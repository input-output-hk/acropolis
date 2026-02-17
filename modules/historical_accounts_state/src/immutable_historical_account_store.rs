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
use fjall::{Database, Keyspace, KeyspaceCreateOptions};
use minicbor::{decode, to_vec};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use tokio::sync::Mutex;
use tracing::{debug, error};

use crate::state::{AccountEntry, ActiveStakeHistory, HistoricalAccountsConfig};

pub struct ImmutableHistoricalAccountStore {
    rewards_history: Keyspace,
    active_stake_history: Keyspace,
    delegation_history: Keyspace,
    registration_history: Keyspace,
    mir_history: Keyspace,
    withdrawal_history: Keyspace,
    addresses: Keyspace,
    tx_count: Keyspace,
    database: Database,
    pub pending: Mutex<Vec<HashMap<StakeAddress, AccountEntry>>>,
}

impl ImmutableHistoricalAccountStore {
    pub fn new(path: impl AsRef<Path>, clear_on_start: bool) -> Result<Self> {
        let path = path.as_ref();
        if clear_on_start && path.exists() {
            std::fs::remove_dir_all(path)?;
        }

        let database = Database::builder(path).temporary(true).open()?;

        let rewards_history =
            database.keyspace("rewards_history", KeyspaceCreateOptions::default)?;
        let active_stake_history =
            database.keyspace("active_stake_history", KeyspaceCreateOptions::default)?;
        let delegation_history =
            database.keyspace("delegation_history", KeyspaceCreateOptions::default)?;
        let registration_history =
            database.keyspace("registration_history", KeyspaceCreateOptions::default)?;
        let withdrawal_history =
            database.keyspace("withdrawal_history", KeyspaceCreateOptions::default)?;
        let mir_history = database.keyspace("mir_history", KeyspaceCreateOptions::default)?;
        let addresses = database.keyspace("addresses", KeyspaceCreateOptions::default)?;
        let tx_count = database.keyspace("tx_count", KeyspaceCreateOptions::default)?;

        Ok(Self {
            rewards_history,
            active_stake_history,
            delegation_history,
            registration_history,
            withdrawal_history,
            mir_history,
            addresses,
            tx_count,
            database,
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

        let mut batch = self.database.batch();
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
                    for (index, address) in updates.iter().enumerate() {
                        let idx = index as u32;
                        let address_key =
                            Self::make_address_key(&account, epoch, idx, address.clone());
                        batch.insert(&self.addresses, address_key, []);
                    }
                }
            }

            // Persist new tx count
            if config.store_tx_count {
                if let Some(count) = &entry.tx_count {
                    batch.insert(&self.tx_count, epoch_key, count.to_le_bytes());
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
        let mut immutable_rewards = self.collect_keyspace::<AccountReward>(
            &self.rewards_history,
            account.get_hash().as_ref(),
        )?;

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
        let mut immutable_active_stake = self.collect_keyspace::<ActiveStakeHistory>(
            &self.active_stake_history,
            account.get_hash().as_ref(),
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
        let mut immutable_registrations = self.collect_keyspace::<RegistrationUpdate>(
            &self.registration_history,
            account.get_hash().as_ref(),
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
        let mut immutable_delegations = self.collect_keyspace::<DelegationUpdate>(
            &self.delegation_history,
            account.get_hash().as_ref(),
        )?;

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
        let mut immutable_mirs = self.collect_keyspace::<AccountWithdrawal>(
            &self.mir_history,
            account.get_hash().as_ref(),
        )?;

        self.merge_pending(account, |e| e.mir_history.as_ref(), &mut immutable_mirs).await;

        Ok((!immutable_mirs.is_empty()).then_some(immutable_mirs))
    }

    pub async fn get_withdrawal_history(
        &self,
        account: &StakeAddress,
    ) -> Result<Option<Vec<AccountWithdrawal>>> {
        let mut immutable_withdrawals = self.collect_keyspace::<AccountWithdrawal>(
            &self.withdrawal_history,
            account.get_hash().as_ref(),
        )?;

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
    ) -> Result<Option<Vec<ShelleyAddress>>> {
        let prefix = account.to_binary();
        let mut addresses = Vec::new();
        let mut seen = HashSet::new();

        for result in self.addresses.prefix(&prefix) {
            let key = result.key()?;
            let shelley = ShelleyAddress::from_bytes_key(&key[prefix.len() + 8..])?;
            if seen.insert(shelley.clone()) {
                addresses.push(shelley);
            }
        }

        for block_map in self.pending.lock().await.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(addrs) = &entry.addresses {
                    for addr in addrs {
                        if seen.insert(addr.clone()) {
                            addresses.push(addr.clone());
                        }
                    }
                }
            }
        }

        Ok((!addresses.is_empty()).then_some(addresses))
    }

    pub async fn get_tx_count(&self, account: &StakeAddress) -> Result<Option<u32>> {
        let mut total_count = 0;

        for result in self.tx_count.prefix(account.get_hash().as_ref()) {
            let bytes = result.value()?;
            let epoch_count = u32::from_le_bytes(bytes[..4].try_into()?);
            total_count += epoch_count;
        }

        for block_map in self.pending.lock().await.iter() {
            if let Some(entry) = block_map.get(account) {
                if let Some(block_count) = &entry.tx_count {
                    total_count += block_count;
                }
            }
        }

        Ok((total_count != 0).then_some(total_count))
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
                Self::extend_opt_vec_ordered(&mut agg_entry.addresses, entry.addresses);
                if let Some(count) = entry.tx_count {
                    agg_entry.tx_count = Some(agg_entry.tx_count.unwrap_or(0) + count);
                }
            }
            acc
        })
    }

    fn collect_keyspace<T>(&self, keyspace: &Keyspace, prefix: &[u8]) -> Result<Vec<T>>
    where
        T: for<'a> minicbor::Decode<'a, ()>,
    {
        let mut out = Vec::new();
        for result in keyspace.prefix(prefix) {
            let bytes = result.value()?;
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
        key[..28].copy_from_slice(account.get_credential().get_hash().as_ref());
        key[28..32].copy_from_slice(&epoch.to_be_bytes());
        key
    }

    fn make_address_key(
        account: &StakeAddress,
        epoch: u32,
        index: u32,
        address: ShelleyAddress,
    ) -> Vec<u8> {
        let mut key = Vec::new();
        key.extend_from_slice(&account.to_binary());
        key.extend_from_slice(&epoch.to_be_bytes());
        key.extend_from_slice(&index.to_be_bytes());
        key.extend_from_slice(&address.to_bytes_key());
        key
    }

    fn extend_opt_vec<T>(target: &mut Option<Vec<T>>, src: Option<Vec<T>>) {
        if let Some(mut v) = src {
            if !v.is_empty() {
                target.get_or_insert_with(Vec::new).append(&mut v);
            }
        }
    }

    fn extend_opt_vec_ordered<T>(target: &mut Option<Vec<T>>, src: Option<Vec<T>>)
    where
        T: Eq + Hash + Clone,
    {
        if let Some(src_vec) = src {
            if src_vec.is_empty() {
                return;
            }

            let target_vec = target.get_or_insert_with(Vec::new);
            let mut seen: HashSet<T> = target_vec.iter().cloned().collect();

            for item in src_vec {
                if seen.insert(item.clone()) {
                    target_vec.push(item);
                }
            }
        }
    }
}
