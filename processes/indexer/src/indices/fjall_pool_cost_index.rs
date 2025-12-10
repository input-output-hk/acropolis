#![allow(unused)]
use acropolis_codec::utils::to_pool_id;
use acropolis_common::{BlockInfo, Lovelace, Point, PoolId};
use acropolis_module_custom_indexer::chain_index::ChainIndex;
use anyhow::Result;
use caryatid_sdk::async_trait;
use fjall::{Config, Keyspace, Partition, PartitionCreateOptions};
use pallas::ledger::primitives::{alonzo, conway};
use pallas::ledger::traverse::{MultiEraCert, MultiEraTx};
use std::collections::BTreeMap;
use std::path::Path;
use tokio::sync::watch;
use tracing::warn;

#[derive(Clone)]
pub struct FjallPoolCostState {
    pub pools: BTreeMap<PoolId, Lovelace>,
}

pub struct FjallPoolCostIndex {
    state: FjallPoolCostState,
    sender: watch::Sender<FjallPoolCostState>,
    partition: Partition,
}

impl FjallPoolCostIndex {
    pub fn new(path: impl AsRef<Path>, sender: watch::Sender<FjallPoolCostState>) -> Result<Self> {
        // Open DB
        let cfg = Config::new(path).max_write_buffer_size(512 * 1024 * 1024);
        let keyspace = Keyspace::open(cfg)?;
        let partition = keyspace.open_partition("pools", PartitionCreateOptions::default())?;

        // Read existing state into memory
        let mut pools = BTreeMap::new();
        for item in partition.iter() {
            let (key, val) = item?;
            let pool_id = PoolId::try_from(key.as_ref())?;
            let cost: Lovelace = bincode::deserialize(&val)?;
            pools.insert(pool_id, cost);
        }

        Ok(Self {
            state: FjallPoolCostState { pools },
            sender,
            partition,
        })
    }
}

#[async_trait]
impl ChainIndex for FjallPoolCostIndex {
    fn name(&self) -> String {
        "in-memory-pool-cost-index".into()
    }

    async fn handle_onchain_tx(&mut self, _info: &BlockInfo, tx: &MultiEraTx<'_>) -> Result<()> {
        let mut changed = false;
        for cert in tx.certs().iter() {
            match cert {
                MultiEraCert::AlonzoCompatible(cert) => match cert.as_ref().as_ref() {
                    alonzo::Certificate::PoolRegistration { operator, cost, .. } => {
                        let pool_id = to_pool_id(operator);
                        let key = pool_id.as_ref();
                        let value = bincode::serialize(cost)?;

                        self.state.pools.insert(pool_id, *cost);
                        self.partition.insert(key, value)?;
                        changed = true;
                    }
                    alonzo::Certificate::PoolRetirement(operator, ..) => {
                        let pool_id = to_pool_id(operator);
                        let key = pool_id.as_ref();

                        self.state.pools.remove(&pool_id);
                        self.partition.remove(key)?;
                        changed = true;
                    }

                    _ => {}
                },
                MultiEraCert::Conway(cert) => match cert.as_ref().as_ref() {
                    conway::Certificate::PoolRegistration { operator, cost, .. } => {
                        let pool_id = to_pool_id(operator);
                        let key = pool_id.as_ref();
                        let value = bincode::serialize(cost)?;

                        self.state.pools.insert(pool_id, *cost);
                        self.partition.insert(key, value)?;
                        changed = true;
                    }
                    conway::Certificate::PoolRetirement(operator, ..) => {
                        let pool_id = to_pool_id(operator);
                        let key = pool_id.as_ref();

                        self.state.pools.remove(&pool_id);
                        self.partition.remove(key)?;
                        changed = true;
                    }
                    _ => {}
                },
                _ => {}
            }
        }

        if changed && self.sender.send(self.state.clone()).is_err() {
            warn!("Pool cost state receiver dropped");
        }

        Ok(())
    }

    async fn reset(&mut self, start: &Point) -> Result<Point> {
        self.state.pools = BTreeMap::new();

        for item in self.partition.iter() {
            let (key, _) = item?;
            self.partition.remove(key.as_ref())?;
        }

        Ok(start.clone())
    }
}
