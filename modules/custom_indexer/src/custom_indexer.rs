//! Acropolis custom indexer module for Caryatid
//!
//! This module lets downstream applications register `ChainIndex` implementations
//! that react to on-chain transactions. The indexer handles cursor persistence,
//! initial sync, and dispatching decoded transactions to user provided indices.

pub mod chain_index;
mod configuration;
pub mod cursor_store;

use std::sync::Arc;
use tokio::sync::Mutex;

use anyhow::Result;
use config::Config;
use tracing::{error, info, warn};

use caryatid_sdk::{async_trait, Context, Module};
use pallas::ledger::traverse::MultiEraTx;

use acropolis_common::{
    commands::chain_sync::ChainSyncCommand,
    messages::{CardanoMessage, Command, Message, StateTransitionMessage},
    Point,
};

use crate::{
    chain_index::ChainIndex, configuration::CustomIndexerConfig, cursor_store::CursorStore,
};

pub struct CustomIndexer<I: ChainIndex, CS: CursorStore> {
    index: Arc<Mutex<I>>,
    cursor_store: Arc<Mutex<CS>>,
    tip: Arc<Mutex<Point>>,
}

impl<I: ChainIndex, CS: CursorStore> CustomIndexer<I, CS> {
    pub fn new(index: I, cursor_store: CS, start: Point) -> Self {
        Self {
            index: Arc::new(Mutex::new(index)),
            cursor_store: Arc::new(Mutex::new(cursor_store)),
            tip: Arc::new(Mutex::new(start)),
        }
    }
}

#[async_trait]
impl<I, CS> Module<Message> for CustomIndexer<I, CS>
where
    I: ChainIndex,
    CS: CursorStore,
{
    fn get_name(&self) -> &'static str {
        "custom-indexer"
    }

    fn get_description(&self) -> &'static str {
        "Single external chain indexer module"
    }

    async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = CustomIndexerConfig::try_load(&config)?;
        let mut subscription = context.subscribe(&cfg.txs_subscribe_topic).await?;
        let run_context = context.clone();

        // Retrieve tip from cursor store with fallback to initial sync point
        let start_point = {
            let saved = self.cursor_store.lock().await.load().await?;
            let mut tip_guard = self.tip.lock().await;
            let start_point = saved.unwrap_or_else(|| tip_guard.clone());
            *tip_guard = start_point.clone();
            start_point
        };

        let index = Arc::clone(&self.index);
        let cursor_store = Arc::clone(&self.cursor_store);
        let tip = Arc::clone(&self.tip);

        context.run(async move {
            // Publish initial sync point
            let msg = Message::Command(Command::ChainSync(ChainSyncCommand::FindIntersect(
                start_point,
            )));
            run_context.publish(&cfg.sync_command_publisher_topic, Arc::new(msg)).await?;
            info!(
                "Publishing initial sync command on {}",
                cfg.sync_command_publisher_topic
            );

            // Forward received txs and rollback notifications to index handlers
            loop {
                match subscription.read().await {
                    Ok((_, message)) => {
                        match message.as_ref() {
                            Message::Cardano((block, CardanoMessage::ReceivedTxs(txs_msg))) => {
                                // Call handle_onchain_tx on the index for all decoded txs
                                let mut idx = index.lock().await;
                                for (tx_index, raw_tx) in txs_msg.txs.iter().enumerate() {
                                    match MultiEraTx::decode(raw_tx) {
                                        Ok(tx) => {
                                            if let Err(e) = idx.handle_onchain_tx(block, &tx).await
                                            {
                                                warn!(
                                                    "Failed to index tx {} in block {}: {e:#}",
                                                    tx_index, block.number
                                                );
                                            }
                                        }
                                        Err(_) => {
                                            warn!(
                                                "Failed to decode tx {} in block {}",
                                                tx_index, block.number
                                            );
                                        }
                                    }
                                }

                                // Update and save tip
                                let new_tip = Point::Specific {
                                    hash: block.hash,
                                    slot: block.slot,
                                };
                                *tip.lock().await = new_tip.clone();
                                cursor_store.lock().await.save(&new_tip).await?;
                            }

                            Message::Cardano((
                                _,
                                CardanoMessage::StateTransition(StateTransitionMessage::Rollback(
                                    point,
                                )),
                            )) => {
                                // Call handle rollback on index
                                {
                                    let mut idx = index.lock().await;
                                    if let Err(e) = idx.handle_rollback(point).await {
                                        error!("Failed to handle rollback at {:?}: {e:#}", point);
                                        return Err(e);
                                    }
                                }

                                // Rollback tip and save
                                {
                                    *tip.lock().await = point.clone();
                                }
                                cursor_store.lock().await.save(point).await?;
                            }
                            _ => (),
                        }
                    }
                    Err(e) => {
                        error!("Subscription closed: {e:#}");
                        break;
                    }
                }
            }

            Ok::<_, anyhow::Error>(())
        });

        Ok(())
    }
}
