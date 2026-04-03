use std::{collections::BTreeMap, sync::Arc};

use acropolis_common::{
    messages::{ProtocolParamsMessage, RawBlockMessage},
    state_history::debug_fingerprint,
    BlockInfo, GenesisDelegates, HeavyDelegate, PoolId,
};
use anyhow::Result;
use imbl::HashMap;

use crate::stores::Store;

#[derive(Default, Debug, Clone)]
pub struct State {
    pub byron_heavy_delegates: HashMap<PoolId, HeavyDelegate>,
    pub shelley_genesis_delegates: GenesisDelegates,
}

#[derive(serde::Serialize)]
struct StableState {
    byron_heavy_delegates: BTreeMap<PoolId, HeavyDelegate>,
    shelley_genesis_delegates: GenesisDelegates,
}

impl serde::Serialize for State {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        StableState {
            byron_heavy_delegates: self
                .byron_heavy_delegates
                .iter()
                .map(|(pool_id, delegate)| (*pool_id, delegate.clone()))
                .collect(),
            shelley_genesis_delegates: self.shelley_genesis_delegates.clone(),
        }
        .serialize(serializer)
    }
}

impl State {
    pub fn new() -> Self {
        Self {
            byron_heavy_delegates: HashMap::new(),
            shelley_genesis_delegates: GenesisDelegates::default(),
        }
    }

    pub fn rollback_debug_summary(&self) -> String {
        let byron_heavy_delegates: BTreeMap<PoolId, HeavyDelegate> = self
            .byron_heavy_delegates
            .iter()
            .map(|(pool_id, delegate)| (*pool_id, delegate.clone()))
            .collect();

        format!(
            "byron_heavy_delegates_len={} byron_heavy_delegates={} shelley_genesis_delegates={}",
            byron_heavy_delegates.len(),
            debug_fingerprint(&byron_heavy_delegates),
            debug_fingerprint(&self.shelley_genesis_delegates),
        )
    }
}

impl State {
    pub fn handle_new_block(
        store: &Arc<dyn Store>,
        block_info: &BlockInfo,
        block: &RawBlockMessage,
    ) -> Result<()> {
        if store.should_persist(block_info.number) {
            store.insert_block(block_info, &block.body)?;
        }

        Ok(())
    }

    pub fn handle_first_block(
        store: &Arc<dyn Store>,
        block_info: &BlockInfo,
        block: &RawBlockMessage,
    ) -> Result<()> {
        if !store.should_persist(block_info.number) {
            if let Some(existing) = store.get_block_by_number(block_info.number)? {
                if existing.bytes != block.body {
                    return Err(anyhow::anyhow!(
                        "Stored block {} does not match. Set clear-on-start to true",
                        block_info.number
                    ));
                }
            } else {
                return Err(anyhow::anyhow!(
                    "Unable to retrieve block {}. Set clear-on-start to true",
                    block_info.number
                ));
            }
        }

        Self::handle_new_block(store, block_info, block)?;

        Ok(())
    }

    pub fn handle_new_params(&mut self, params: &ProtocolParamsMessage) {
        if let Some(byron) = &params.params.byron {
            self.byron_heavy_delegates = byron.heavy_delegation.clone().into();
        }
        if let Some(shelley) = &params.params.shelley {
            self.shelley_genesis_delegates = shelley.gen_delegs.clone();
        }
    }
}
