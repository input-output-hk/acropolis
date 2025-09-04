mod stores;

use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::blocks::{
        BlockInfo, BlocksStateQuery, BlocksStateQueryResponse, DEFAULT_BLOCKS_QUERY_TOPIC,
    },
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Module};
use config::Config;
use std::sync::Arc;
use tracing::error;

use crate::stores::{fjall::FjallStore, Block, Store};

const DEFAULT_BLOCKS_TOPIC: &str = "cardano.block.body";
const DEFAULT_STORE: &str = "fjall";

#[module(
    message_type(Message),
    name = "chain-store",
    description = "Block and TX state"
)]
pub struct ChainStore;

impl ChainStore {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let new_blocks_topic =
            config.get_string("blocks-topic").unwrap_or(DEFAULT_BLOCKS_TOPIC.to_string());
        let block_queries_topic = config
            .get_string(DEFAULT_BLOCKS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_BLOCKS_QUERY_TOPIC.1.to_string());

        let store_type = config.get_string("store").unwrap_or(DEFAULT_STORE.to_string());
        let store: Arc<dyn Store> = match store_type.as_str() {
            "fjall" => Arc::new(FjallStore::new(config.clone())?),
            _ => bail!("Unknown store type {store_type}"),
        };

        let query_store = store.clone();
        context.handle(&block_queries_topic, move |req| {
            let query_store = query_store.clone();
            async move {
                let Message::StateQuery(StateQuery::Blocks(query)) = req.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Blocks(
                        BlocksStateQueryResponse::Error("Invalid message for blocks-state".into()),
                    )));
                };
                let res = Self::handle_blocks_query(&query_store, query)
                    .unwrap_or_else(|err| BlocksStateQueryResponse::Error(err.to_string()));
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Blocks(res)))
            }
        });

        let mut new_blocks_subscription = context.subscribe(&new_blocks_topic).await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = new_blocks_subscription.read().await else {
                    return;
                };

                if let Err(err) = Self::handle_new_block(&store, &message) {
                    error!("Could not insert block: {err}");
                }
            }
        });

        Ok(())
    }

    fn handle_new_block(store: &Arc<dyn Store>, message: &Message) -> Result<()> {
        let Message::Cardano((info, CardanoMessage::BlockBody(body))) = message else {
            bail!("Unexpected message type: {message:?}");
        };

        store.insert_block(info, &body.raw)
    }

    fn handle_blocks_query(
        store: &Arc<dyn Store>,
        query: &BlocksStateQuery,
    ) -> Result<BlocksStateQueryResponse> {
        match query {
            BlocksStateQuery::GetLatestBlock => {
                let block = store.get_latest_block()?;
                let info = Self::to_block_info(block);
                Ok(BlocksStateQueryResponse::LatestBlock(info))
            }
            BlocksStateQuery::GetBlockInfo { block_key } => {
                let block = store.get_block_by_hash(block_key)?;
                let info = Self::to_block_info(block);
                Ok(BlocksStateQueryResponse::BlockInfo(info))
            }
            BlocksStateQuery::GetBlockBySlot { slot } => {
                let block = store.get_block_by_slot(*slot)?;
                let info = Self::to_block_info(block);
                Ok(BlocksStateQueryResponse::BlockBySlot(info))
            }
            other => bail!("{other:?} not yet supported"),
        }
    }

    fn to_block_info(_block: Block) -> BlockInfo {
        BlockInfo {}
    }
}
