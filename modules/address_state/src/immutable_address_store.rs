use std::{collections::HashMap, path::Path};

use crate::state::{AddressEntry, AddressStorageConfig, UtxoDelta};
use acropolis_common::{Address, AddressTotals, TxIdentifier, UTxOIdentifier};
use anyhow::Result;
use fjall::{Database, Keyspace, KeyspaceCreateOptions};
use minicbor::{decode, to_vec};
use tokio::{sync::Mutex, task};
use tracing::{debug, error, info};

// Metadata keys which store the last epoch saved in each partition
const ADDRESS_UTXOS_EPOCH_COUNTER: &[u8] = b"utxos_epoch_last";
const ADDRESS_TXS_EPOCH_COUNTER: &[u8] = b"txs_epoch_last";
const ADDRESS_TOTALS_EPOCH_COUNTER: &[u8] = b"totals_epoch_last";

#[derive(Default)]
struct MergedDeltas {
    created_utxos: Vec<UTxOIdentifier>,
    spent_utxos: Vec<UTxOIdentifier>,
    txs: Vec<TxIdentifier>,
    totals: AddressTotals,
}

pub struct ImmutableAddressStore {
    utxos: Keyspace,
    txs: Keyspace,
    totals: Keyspace,
    database: Database,
    pub pending: Mutex<Vec<HashMap<Address, AddressEntry>>>,
}

impl ImmutableAddressStore {
    pub fn new(path: impl AsRef<Path>, clear_on_start: bool) -> Result<Self> {
        let path = path.as_ref();
        if path.exists() && clear_on_start {
            std::fs::remove_dir_all(path)?;
        }

        let database = Database::builder(path).open()?;

        let utxos = database.keyspace("address_utxos", KeyspaceCreateOptions::default)?;
        let txs = database.keyspace("address_txs", KeyspaceCreateOptions::default)?;
        let totals = database.keyspace("address_totals", KeyspaceCreateOptions::default)?;

        Ok(Self {
            utxos,
            txs,
            totals,
            database,
            pending: Mutex::new(Vec::new()),
        })
    }

    /// Persists volatile UTxOs, transactions, and totals into their respective Fjall partitions
    /// for an entire epoch. Skips any partitions that have already stored the given epoch.
    /// All writes are batched and committed atomically, preventing on-disk corruption in case of failure.
    pub async fn persist_epoch(&self, epoch: u64, config: &AddressStorageConfig) -> Result<()> {
        // Skip if all options disabled
        if !(config.store_info || config.store_transactions || config.store_totals) {
            debug!("no persistence needed for epoch {epoch} (all stores disabled)");
            return Ok(());
        }

        // Determine which partitions need persistence
        let (persist_utxos, persist_txs, persist_totals) = if config.clear_on_start {
            (
                config.store_info,
                config.store_transactions,
                config.store_totals,
            )
        } else {
            let utxos = config.store_info
                && !self
                    .epoch_exists(self.utxos.clone(), ADDRESS_UTXOS_EPOCH_COUNTER, epoch)
                    .await?;
            let txs = config.store_transactions
                && !self.epoch_exists(self.txs.clone(), ADDRESS_TXS_EPOCH_COUNTER, epoch).await?;
            let totals = config.store_totals
                && !self
                    .epoch_exists(self.totals.clone(), ADDRESS_TOTALS_EPOCH_COUNTER, epoch)
                    .await?;
            (utxos, txs, totals)
        };

        // Skip if all partitions have already been persisted for the epoch
        if !(persist_utxos || persist_txs || persist_totals) {
            debug!("no persistence needed for epoch {epoch}");
            return Ok(());
        }

        let drained_blocks = {
            let mut pending = self.pending.lock().await;
            std::mem::take(&mut *pending)
        };

        let mut batch = self.database.batch();
        let mut change_count = 0;

        for (address, deltas) in Self::merge_block_deltas(drained_blocks) {
            change_count += 1;
            let addr_key = address.to_bytes_key()?;

            if persist_utxos && (!deltas.created_utxos.is_empty() || !deltas.spent_utxos.is_empty())
            {
                let mut live: Vec<UTxOIdentifier> = self
                    .utxos
                    .get(&addr_key)?
                    .map(|bytes| decode(&bytes))
                    .transpose()?
                    .unwrap_or_default();

                live.extend(&deltas.created_utxos);

                for u in &deltas.spent_utxos {
                    live.retain(|x| x != u);
                }

                batch.insert(&self.utxos, &addr_key, to_vec(&live)?);
            }

            if persist_txs && !deltas.txs.is_empty() {
                let mut live: Vec<TxIdentifier> = self
                    .txs
                    .get(&addr_key)?
                    .map(|bytes| decode(&bytes))
                    .transpose()?
                    .unwrap_or_default();

                live.extend(deltas.txs.iter().cloned());
                batch.insert(&self.txs, &addr_key, to_vec(&live)?);
            }

            if persist_totals && deltas.totals.tx_count != 0 {
                let mut live: AddressTotals = self
                    .totals
                    .get(&addr_key)?
                    .map(|bytes| decode(&bytes))
                    .transpose()?
                    .unwrap_or_default();

                live += deltas.totals;
                batch.insert(&self.totals, &addr_key, to_vec(&live)?);
            }
        }

        // Metadata markers
        for (enabled, part, key) in [
            (persist_utxos, &self.utxos, ADDRESS_UTXOS_EPOCH_COUNTER),
            (persist_txs, &self.txs, ADDRESS_TXS_EPOCH_COUNTER),
            (persist_totals, &self.totals, ADDRESS_TOTALS_EPOCH_COUNTER),
        ] {
            if enabled {
                batch.insert(part, key, epoch.to_le_bytes());
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

    pub async fn update_immutable(&self, drained: Vec<HashMap<Address, AddressEntry>>) {
        let mut pending = self.pending.lock().await;
        pending.extend(drained);
    }

    pub async fn get_utxos(&self, address: &Address) -> Result<Option<Vec<UTxOIdentifier>>> {
        let key = address.to_bytes_key()?;

        let db_raw = self.utxos.get(&key)?;
        let db_had_key = db_raw.is_some();

        let mut live: Vec<UTxOIdentifier> =
            db_raw.map(|bytes| decode(&bytes)).transpose()?.unwrap_or_default();

        let pending = self.pending.lock().await;
        let mut pending_touched = false;
        for block_map in pending.iter() {
            if let Some(entry) = block_map.get(address) {
                if let Some(deltas) = &entry.utxos {
                    pending_touched = true;
                    for delta in deltas {
                        match delta {
                            UtxoDelta::Created(u) => live.push(*u),
                            UtxoDelta::Spent(u) => live.retain(|x| x != u),
                        }
                    }
                }
            }
        }

        // Only return None if the address never existed
        if live.is_empty() {
            if db_had_key || pending_touched {
                Ok(Some(vec![]))
            } else {
                Ok(None)
            }
        } else {
            Ok(Some(live))
        }
    }

    pub async fn get_txs(&self, address: &Address) -> Result<Option<Vec<TxIdentifier>>> {
        let key = address.to_bytes_key()?;
        let mut live: Vec<TxIdentifier> =
            self.txs.get(&key)?.map(|bytes| decode(&bytes)).transpose()?.unwrap_or_default();

        let pending = self.pending.lock().await;
        for block_map in pending.iter() {
            if let Some(entry) = block_map.get(address) {
                if let Some(txs) = &entry.transactions {
                    live.extend(txs.iter().cloned());
                }
            }
        }

        if live.is_empty() {
            Ok(None)
        } else {
            Ok(Some(live))
        }
    }

    pub async fn get_totals(&self, address: &Address) -> Result<Option<AddressTotals>> {
        let key = address.to_bytes_key()?;

        let mut live: AddressTotals =
            self.totals.get(&key)?.map(|bytes| decode(&bytes)).transpose()?.unwrap_or_default();

        let pending = self.pending.lock().await;
        for block_map in pending.iter() {
            if let Some(entry) = block_map.get(address) {
                if let Some(deltas) = &entry.totals {
                    for delta in deltas {
                        live.apply_delta(delta);
                    }
                }
            }
        }

        if live.tx_count == 0 {
            Ok(None)
        } else {
            Ok(Some(live))
        }
    }

    pub async fn get_last_epoch_stored(&self) -> Result<Option<u64>> {
        let read_marker = |keyspace: Keyspace, key: &'static [u8]| async move {
            task::spawn_blocking(move || {
                Ok::<_, anyhow::Error>(match keyspace.get(key)? {
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

        let u = read_marker(self.utxos.clone(), ADDRESS_UTXOS_EPOCH_COUNTER).await?;
        let t = read_marker(self.txs.clone(), ADDRESS_TXS_EPOCH_COUNTER).await?;
        let tot = read_marker(self.totals.clone(), ADDRESS_TOTALS_EPOCH_COUNTER).await?;

        let min_epoch = [u, t, tot].into_iter().flatten().min();

        if let Some(epoch) = min_epoch {
            info!("last epoch already stored across partitions: {epoch}");
        } else {
            info!("no epoch markers found across partitions");
        }

        Ok(min_epoch)
    }

    async fn epoch_exists(
        &self,
        keyspace: Keyspace,
        key: &'static [u8],
        epoch: u64,
    ) -> Result<bool> {
        let exists = task::spawn_blocking(move || -> Result<bool> {
            let bytes = match keyspace.get(key)? {
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

    fn merge_block_deltas(
        drained_blocks: Vec<HashMap<Address, AddressEntry>>,
    ) -> HashMap<Address, MergedDeltas> {
        let mut merged = HashMap::new();

        for block_map in drained_blocks {
            for (addr, entry) in block_map {
                let target = merged.entry(addr.clone()).or_insert_with(MergedDeltas::default);

                // Remove UTxOs that are spent in the same epoch
                if let Some(deltas) = &entry.utxos {
                    for delta in deltas {
                        match delta {
                            UtxoDelta::Created(u) => target.created_utxos.push(*u),
                            UtxoDelta::Spent(u) => {
                                if target.created_utxos.contains(u) {
                                    target.created_utxos.retain(|x| x != u);
                                } else {
                                    target.spent_utxos.push(*u);
                                }
                            }
                        }
                    }
                }

                // Merge Tx vectors
                if let Some(txs) = &entry.transactions {
                    target.txs.extend(txs.iter().cloned());
                }

                // Sum totals
                if let Some(totals) = &entry.totals {
                    for delta in totals {
                        target.totals.apply_delta(delta);
                    }
                }
            }
        }

        merged
    }
}
