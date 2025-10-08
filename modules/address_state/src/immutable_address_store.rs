use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use crate::state::{AddressEntry, AddressStorageConfig, UtxoDelta};
use acropolis_common::{Address, AddressTotals, TxIdentifier, UTxOIdentifier};
use anyhow::Result;
use fjall::{Keyspace, Partition, PartitionCreateOptions};
use minicbor::{decode, to_vec};
use tokio::task;
use tracing::{debug, error, info};

// Metadata keys which store the last epoch saved in each partition
const ADDRESS_UTXOS_EPOCH_COUNTER: &[u8] = b"utxos_epoch_last";
const ADDRESS_TXS_EPOCH_COUNTER: &[u8] = b"txs_epoch_last";
const ADDRESS_TOTALS_EPOCH_COUNTER: &[u8] = b"totals_epoch_last";

pub struct ImmutableAddressStore {
    utxos: Partition,
    txs: Partition,
    totals: Partition,
    keyspace: Keyspace,
}

impl ImmutableAddressStore {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let cfg = fjall::Config::new(path).max_write_buffer_size(512 * 1024 * 1024);
        let keyspace = Keyspace::open(cfg)?;

        let utxos = keyspace.open_partition("address_utxos", PartitionCreateOptions::default())?;
        let txs = keyspace.open_partition("address_txs", PartitionCreateOptions::default())?;
        let totals =
            keyspace.open_partition("address_totals", PartitionCreateOptions::default())?;

        Ok(Self {
            utxos,
            txs,
            totals,
            keyspace,
        })
    }

    /// Persists volatile UTxOs, transactions, and totals into their respective Fjall partitions for an entire epoch.
    /// Skips any partitions that have already stored the given epoch.
    /// All writes are batched and committed atomically, preventing on-disk corruption in case of failure.
    pub async fn persist_epoch(
        &self,
        epoch: u64,
        drained_blocks: Vec<HashMap<Address, AddressEntry>>,
        config: &AddressStorageConfig,
    ) -> Result<()> {
        let persist_utxos = config.store_info
            && !self.epoch_exists(self.utxos.clone(), ADDRESS_UTXOS_EPOCH_COUNTER, epoch).await?;
        let persist_txs = config.store_transactions
            && !self.epoch_exists(self.txs.clone(), ADDRESS_TXS_EPOCH_COUNTER, epoch).await?;
        let persist_totals = config.store_totals
            && !self.epoch_exists(self.totals.clone(), ADDRESS_TOTALS_EPOCH_COUNTER, epoch).await?;

        if !(persist_utxos || persist_txs || persist_totals) {
            debug!("no persistence needed for epoch {epoch} (already persisted or disabled)",);
            return Ok(());
        }

        let keyspace = self.keyspace.clone();
        let utxos = self.utxos.clone();
        let txs = self.txs.clone();
        let totals = self.totals.clone();

        task::spawn_blocking(move || -> Result<()> {
            let mut batch = keyspace.batch();
            let mut change_count = 0;

            for block_map in drained_blocks.into_iter() {
                if block_map.is_empty() {
                    continue;
                }

                for (addr, entry) in block_map {
                    change_count += 1;
                    let addr_key = addr.to_bytes_key()?;

                    if persist_utxos {
                        let mut live: HashSet<UTxOIdentifier> = utxos
                            .get(&addr_key)?
                            .map(|bytes| decode(&bytes))
                            .transpose()?
                            .unwrap_or_default();

                        if let Some(deltas) = &entry.utxos {
                            for delta in deltas {
                                match delta {
                                    UtxoDelta::Created(u) => {
                                        live.insert(*u);
                                    }
                                    UtxoDelta::Spent(u) => {
                                        live.remove(u);
                                    }
                                }
                            }
                        }

                        batch.insert(&utxos, &addr_key, to_vec(&live)?);
                    }

                    if persist_txs {
                        let mut live: Vec<TxIdentifier> = txs
                            .get(&addr_key)?
                            .map(|bytes| decode(&bytes))
                            .transpose()?
                            .unwrap_or_default();

                        if let Some(txs_deltas) = &entry.transactions {
                            live.extend(txs_deltas.iter().cloned());
                        }

                        batch.insert(&txs, &addr_key, to_vec(&live)?);
                    }

                    if persist_totals {
                        let mut live: AddressTotals = totals
                            .get(&addr_key)?
                            .map(|bytes| decode(&bytes))
                            .transpose()?
                            .unwrap_or_default();

                        if let Some(deltas) = &entry.totals {
                            for delta in deltas {
                                live.apply_delta(delta);
                            }
                        }

                        batch.insert(&totals, &addr_key, to_vec(&live)?);
                    }
                }
            }

            // Metadata markers
            if persist_utxos {
                batch.insert(&utxos, ADDRESS_UTXOS_EPOCH_COUNTER, &epoch.to_le_bytes());
            }
            if persist_txs {
                batch.insert(&txs, ADDRESS_TXS_EPOCH_COUNTER, &epoch.to_le_bytes());
            }
            if persist_totals {
                batch.insert(&totals, ADDRESS_TOTALS_EPOCH_COUNTER, &epoch.to_le_bytes());
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
        })
        .await??;

        Ok(())
    }

    pub async fn get_utxos(&self, address: &Address) -> Result<Option<Vec<UTxOIdentifier>>> {
        let key = address.to_bytes_key()?;
        let partition = self.utxos.clone();
        task::spawn_blocking(move || match partition.get(key)? {
            Some(bytes) => {
                let decoded: Vec<UTxOIdentifier> = decode(&bytes)?;
                Ok(Some(decoded))
            }
            None => Ok(None),
        })
        .await?
    }

    pub async fn get_txs(&self, address: &Address) -> Result<Option<Vec<TxIdentifier>>> {
        let key = address.to_bytes_key()?;
        let partition = self.txs.clone();
        task::spawn_blocking(move || match partition.get(key)? {
            Some(bytes) => {
                let decoded: Vec<TxIdentifier> = decode(&bytes)?;
                Ok(Some(decoded))
            }
            None => Ok(None),
        })
        .await?
    }

    pub async fn get_totals(&self, address: &Address) -> Result<Option<AddressTotals>> {
        let key = address.to_bytes_key()?;
        let partition = self.totals.clone();
        task::spawn_blocking(move || match partition.get(key)? {
            Some(bytes) => {
                let decoded: AddressTotals = decode(&bytes)?;
                Ok(Some(decoded))
            }
            None => Ok(None),
        })
        .await?
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
