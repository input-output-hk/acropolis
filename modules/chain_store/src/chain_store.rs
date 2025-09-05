mod stores;

use acropolis_common::{
    crypto::keyhash,
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
            BlocksStateQuery::GetLatestBlock => match store.get_latest_block()? {
                Some(block) => {
                    let info = Self::to_block_info(block, store, true)?;
                    Ok(BlocksStateQueryResponse::LatestBlock(info))
                }
                None => Ok(BlocksStateQueryResponse::NotFound),
            },
            BlocksStateQuery::GetBlockInfo { block_key } => {
                match store.get_block_by_hash(block_key)? {
                    Some(block) => {
                        let info = Self::to_block_info(block, store, false)?;
                        Ok(BlocksStateQueryResponse::BlockInfo(info))
                    }
                    None => Ok(BlocksStateQueryResponse::NotFound),
                }
            }
            BlocksStateQuery::GetBlockBySlot { slot } => match store.get_block_by_slot(*slot)? {
                Some(block) => {
                    let info = Self::to_block_info(block, store, false)?;
                    Ok(BlocksStateQueryResponse::BlockBySlot(info))
                }
                None => Ok(BlocksStateQueryResponse::NotFound),
            },
            BlocksStateQuery::GetBlockByEpochSlot { epoch, slot } => {
                match store.get_block_by_epoch_slot(*epoch, *slot)? {
                    Some(block) => {
                        let info = Self::to_block_info(block, store, false)?;
                        Ok(BlocksStateQueryResponse::BlockByEpochSlot(info))
                    }
                    None => Ok(BlocksStateQueryResponse::NotFound),
                }
            }
            other => bail!("{other:?} not yet supported"),
        }
    }

    fn to_block_info(block: Block, store: &Arc<dyn Store>, is_latest: bool) -> Result<BlockInfo> {
        let decoded = pallas_traverse::MultiEraBlock::decode(&block.bytes)?;
        let header = decoded.header();
        let mut output = None;
        let mut fees = None;
        for tx in decoded.txs() {
            if let Some(new_fee) = tx.fee() {
                fees = Some(fees.unwrap_or_default() + new_fee);
            }
            for o in tx.outputs() {
                output = Some(output.unwrap_or_default() + o.value().coin())
            }
        }
        let (op_cert_hot_vkey, op_cert_counter) = match &header {
            pallas_traverse::MultiEraHeader::BabbageCompatible(h) => {
                let cert = &h.header_body.operational_cert;
                (
                    Some(&cert.operational_cert_hot_vkey),
                    Some(cert.operational_cert_sequence_number),
                )
            }
            pallas_traverse::MultiEraHeader::ShelleyCompatible(h) => (
                Some(&h.header_body.operational_cert_hot_vkey),
                Some(h.header_body.operational_cert_sequence_number),
            ),
            _ => (None, None),
        };
        let op_cert = op_cert_hot_vkey.map(|vkey| keyhash(vkey));

        let (next_block, confirmations) = if is_latest {
            (None, 0)
        } else {
            let number = header.number();
            let raw_latest_block = store.get_latest_block()?.unwrap();
            let latest_block = pallas_traverse::MultiEraBlock::decode(&raw_latest_block.bytes)?;
            let latest_block_number = latest_block.number();
            let confirmations = latest_block_number - number;

            let next_block_number = number + 1;
            let next_block_hash = if next_block_number == latest_block_number {
                Some(latest_block.hash().to_vec())
            } else {
                let raw_next_block = store.get_block_by_number(next_block_number)?;
                if let Some(raw_block) = raw_next_block {
                    let block = pallas_traverse::MultiEraBlock::decode(&raw_block.bytes)?;
                    Some(block.hash().to_vec())
                } else {
                    None
                }
            };
            (next_block_hash, confirmations)
        };

        Ok(BlockInfo {
            timestamp: block.extra.timestamp,
            number: header.number(),
            hash: header.hash().to_vec(),
            slot: header.slot(),
            epoch: block.extra.epoch,
            epoch_slot: block.extra.epoch_slot,
            issuer_vkey: header.issuer_vkey().map(|key| key.to_vec()),
            size: block.bytes.len() as u64,
            tx_count: decoded.tx_count() as u64,
            output,
            fees,
            block_vrf: header.vrf_vkey().map(|key| key.to_vec()),
            op_cert,
            op_cert_counter,
            previous_block: header.previous_hash().map(|x| x.to_vec()),
            next_block,
            confirmations,
        })
    }
}
