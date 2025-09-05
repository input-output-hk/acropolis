mod stores;

use acropolis_common::{
    crypto::keyhash,
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::blocks::{
        BlockInfo, BlockTransaction, BlockTransactions, BlockTransactionsCBOR, BlocksStateQuery,
        BlocksStateQueryResponse, NextBlocks, PreviousBlocks, DEFAULT_BLOCKS_QUERY_TOPIC,
    },
    TxHash,
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
                let Some(block) = store.get_latest_block()? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let info = Self::to_block_info(block, store, true)?;
                Ok(BlocksStateQueryResponse::LatestBlock(info))
            }
            BlocksStateQuery::GetLatestBlockTransactions => {
                let Some(block) = store.get_latest_block()? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let txs = Self::to_block_transactions(block)?;
                Ok(BlocksStateQueryResponse::LatestBlockTransactions(txs))
            }
            BlocksStateQuery::GetLatestBlockTransactionsCBOR => {
                let Some(block) = store.get_latest_block()? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let txs = Self::to_block_transactions_cbor(block)?;
                Ok(BlocksStateQueryResponse::LatestBlockTransactionsCBOR(txs))
            }
            BlocksStateQuery::GetBlockInfo { block_key } => {
                let Some(block) = store.get_block_by_hash(block_key)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let info = Self::to_block_info(block, store, false)?;
                Ok(BlocksStateQueryResponse::BlockInfo(info))
            }
            BlocksStateQuery::GetBlockBySlot { slot } => {
                let Some(block) = store.get_block_by_slot(*slot)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let info = Self::to_block_info(block, store, false)?;
                Ok(BlocksStateQueryResponse::BlockBySlot(info))
            }
            BlocksStateQuery::GetBlockByEpochSlot { epoch, slot } => {
                let Some(block) = store.get_block_by_epoch_slot(*epoch, *slot)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let info = Self::to_block_info(block, store, false)?;
                Ok(BlocksStateQueryResponse::BlockByEpochSlot(info))
            }
            BlocksStateQuery::GetNextBlocks {
                block_key,
                limit,
                skip,
            } => {
                if *limit == 0 {
                    return Ok(BlocksStateQueryResponse::NextBlocks(NextBlocks {
                        blocks: vec![],
                    }));
                }
                let Some(block) = store.get_block_by_hash(&block_key)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let number = Self::get_block_number(&block)?;
                let min_number = number + 1 + skip;
                let max_number = min_number + limit - 1;
                let blocks = store.get_blocks_by_number_range(min_number, max_number)?;
                let info = Self::to_block_info_bulk(blocks, store, false)?;
                Ok(BlocksStateQueryResponse::NextBlocks(NextBlocks {
                    blocks: info,
                }))
            }
            BlocksStateQuery::GetPreviousBlocks {
                block_key,
                limit,
                skip,
            } => {
                if *limit == 0 {
                    return Ok(BlocksStateQueryResponse::PreviousBlocks(PreviousBlocks {
                        blocks: vec![],
                    }));
                }
                let Some(block) = store.get_block_by_hash(&block_key)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let number = Self::get_block_number(&block)?;
                let Some(max_number) = number.checked_sub(1 + skip) else {
                    return Ok(BlocksStateQueryResponse::PreviousBlocks(PreviousBlocks {
                        blocks: vec![],
                    }));
                };
                let min_number = max_number.saturating_sub(limit - 1);
                let blocks = store.get_blocks_by_number_range(min_number, max_number)?;
                let info = Self::to_block_info_bulk(blocks, store, false)?;
                Ok(BlocksStateQueryResponse::PreviousBlocks(PreviousBlocks {
                    blocks: info,
                }))
            }
            BlocksStateQuery::GetBlockTransactions { block_key } => {
                let Some(block) = store.get_block_by_hash(block_key)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let txs = Self::to_block_transactions(block)?;
                Ok(BlocksStateQueryResponse::BlockTransactions(txs))
            }
            BlocksStateQuery::GetBlockTransactionsCBOR { block_key } => {
                let Some(block) = store.get_block_by_hash(block_key)? else {
                    return Ok(BlocksStateQueryResponse::NotFound);
                };
                let txs = Self::to_block_transactions_cbor(block)?;
                Ok(BlocksStateQueryResponse::BlockTransactionsCBOR(txs))
            }

            other => bail!("{other:?} not yet supported"),
        }
    }

    fn get_block_number(block: &Block) -> Result<u64> {
        Ok(pallas_traverse::MultiEraBlock::decode(&block.bytes)?.number())
    }

    fn to_block_info(block: Block, store: &Arc<dyn Store>, is_latest: bool) -> Result<BlockInfo> {
        let blocks = vec![block];
        let mut info = Self::to_block_info_bulk(blocks, store, is_latest)?;
        Ok(info.remove(0))
    }

    fn to_block_info_bulk(
        blocks: Vec<Block>,
        store: &Arc<dyn Store>,
        final_block_is_latest: bool,
    ) -> Result<Vec<BlockInfo>> {
        if blocks.is_empty() {
            return Ok(vec![]);
        }
        let mut decoded_blocks = vec![];
        for block in &blocks {
            decoded_blocks.push(pallas_traverse::MultiEraBlock::decode(&block.bytes)?);
        }

        let (latest_number, latest_hash) = if final_block_is_latest {
            let latest = decoded_blocks.last().unwrap();
            (latest.number(), latest.hash())
        } else {
            let raw_latest = store.get_latest_block()?.unwrap();
            let latest = pallas_traverse::MultiEraBlock::decode(&raw_latest.bytes)?;
            (latest.number(), latest.hash())
        };

        let mut next_hash = if final_block_is_latest {
            None
        } else {
            let next_number = decoded_blocks.last().unwrap().number() + 1;
            if next_number > latest_number {
                None
            } else if next_number == latest_number {
                Some(latest_hash)
            } else {
                let raw_next = store.get_block_by_number(next_number)?;
                if let Some(raw_next) = raw_next {
                    let next = pallas_traverse::MultiEraBlock::decode(&raw_next.bytes)?;
                    Some(next.hash())
                } else {
                    None
                }
            }
        };

        let mut block_info = vec![];
        for (block, decoded) in blocks.iter().zip(decoded_blocks).rev() {
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

            block_info.push(BlockInfo {
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
                previous_block: header.previous_hash().map(|h| h.to_vec()),
                next_block: next_hash.map(|h| h.to_vec()),
                confirmations: latest_number - header.number(),
            });

            next_hash = Some(header.hash());
        }

        block_info.reverse();
        Ok(block_info)
    }

    fn to_block_transactions(block: Block) -> Result<BlockTransactions> {
        let decoded = pallas_traverse::MultiEraBlock::decode(&block.bytes)?;
        let hashes = decoded.txs().iter().map(|tx| TxHash::from(*tx.hash())).collect();
        Ok(BlockTransactions { hashes })
    }

    fn to_block_transactions_cbor(block: Block) -> Result<BlockTransactionsCBOR> {
        let decoded = pallas_traverse::MultiEraBlock::decode(&block.bytes)?;
        let txs = decoded
            .txs()
            .iter()
            .map(|tx| {
                let hash = TxHash::from(*tx.hash());
                let cbor = tx.encode();
                BlockTransaction { hash, cbor }
            })
            .collect();
        Ok(BlockTransactionsCBOR { txs })
    }
}
