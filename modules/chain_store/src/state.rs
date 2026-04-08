use std::sync::Arc;

use acropolis_common::{
    messages::{ProtocolParamsMessage, RawBlockMessage},
    serialization::serialize_imbl_hashmap_deterministic,
    BlockInfo, GenesisDelegates, HeavyDelegate, PoolId,
};
use anyhow::Result;
use imbl::HashMap;

use crate::stores::Store;

#[derive(Default, Debug, Clone, serde::Serialize)]
pub struct State {
    #[serde(serialize_with = "serialize_imbl_hashmap_deterministic")]
    pub byron_heavy_delegates: HashMap<PoolId, HeavyDelegate>,
    pub shelley_genesis_delegates: GenesisDelegates,
}

impl State {
    pub fn new() -> Self {
        Self {
            byron_heavy_delegates: HashMap::new(),
            shelley_genesis_delegates: GenesisDelegates::default(),
        }
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
