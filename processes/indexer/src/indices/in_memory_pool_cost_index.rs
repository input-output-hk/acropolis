#![allow(unused)]
use acropolis_codec::map_parameters::to_pool_id;
use acropolis_common::{BlockInfo, Lovelace, Point, PoolId};
use acropolis_module_custom_indexer::chain_index::ChainIndex;
use anyhow::Result;
use caryatid_sdk::async_trait;
use pallas::ledger::primitives::{alonzo, conway};
use pallas::ledger::traverse::{MultiEraCert, MultiEraTx};
use std::collections::BTreeMap;
use tokio::sync::watch;
use tracing::warn;

#[derive(Clone)]
pub struct InMemoryPoolCostState {
    pub pools: BTreeMap<PoolId, Lovelace>,
}

pub struct InMemoryPoolCostIndex {
    state: InMemoryPoolCostState,
    sender: watch::Sender<InMemoryPoolCostState>,
}

impl InMemoryPoolCostIndex {
    pub fn new(sender: watch::Sender<InMemoryPoolCostState>) -> Self {
        Self {
            state: InMemoryPoolCostState {
                pools: BTreeMap::new(),
            },
            sender,
        }
    }
}

#[async_trait]
impl ChainIndex for InMemoryPoolCostIndex {
    fn name(&self) -> String {
        "fjall-pool-cost-index".into()
    }

    async fn handle_onchain_tx(&mut self, _info: &BlockInfo, tx: &MultiEraTx<'_>) -> Result<()> {
        let mut changed = false;
        for cert in tx.certs().iter() {
            match cert {
                MultiEraCert::AlonzoCompatible(cert) => match cert.as_ref().as_ref() {
                    alonzo::Certificate::PoolRegistration { operator, cost, .. } => {
                        self.state.pools.insert(to_pool_id(operator), *cost);
                        changed = true;
                    }
                    alonzo::Certificate::PoolRetirement(operator, ..) => {
                        self.state.pools.remove(&to_pool_id(operator));
                        changed = true;
                    }

                    _ => {}
                },
                MultiEraCert::Conway(cert) => match cert.as_ref().as_ref() {
                    conway::Certificate::PoolRegistration { operator, cost, .. } => {
                        self.state.pools.insert(to_pool_id(operator), *cost);
                        changed = true;
                    }
                    conway::Certificate::PoolRetirement(operator, ..) => {
                        self.state.pools.remove(&to_pool_id(operator));
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
        Ok(start.clone())
    }
}
