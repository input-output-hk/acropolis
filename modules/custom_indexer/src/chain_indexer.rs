use acropolis_common::{
    commands::chain_sync::{ChainSyncCommand, Point},
    messages::{CardanoMessage, Command, Message},
};
use anyhow::Result;
use caryatid_sdk::{async_trait, Context, Module};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;

use pallas::ledger::traverse::MultiEraTx;

use crate::{
    configuration::CustomIndexerConfig, cursor_store::CursorStore, managed_index::ManagedIndex,
};

pub struct CustomIndexer<I: ManagedIndex, CS: CursorStore> {
    index: Arc<Mutex<I>>,
    cursor_store: Arc<Mutex<CS>>,
    tip: Arc<Mutex<Point>>,
}

impl<I: ManagedIndex, CS: CursorStore> CustomIndexer<I, CS> {
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
    I: ManagedIndex,
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
            let saved = {
                let cs = self.cursor_store.lock().await;
                cs.load().await?
            };

            let mut tip_guard = self.tip.lock().await;
            if let Some(saved_point) = saved {
                *tip_guard = saved_point.clone();
                saved_point
            } else {
                tip_guard.clone()
            }
        };

        let index = Arc::clone(&self.index);
        let cursor_store = Arc::clone(&self.cursor_store);
        let tip = Arc::clone(&self.tip);

        context.run(async move {
            // Publish initial sync point
            let msg = Message::Command(Command::ChainSync(ChainSyncCommand::FindIntersect {
                point: start_point.clone(),
            }));
            run_context.publish(&cfg.sync_command_publisher_topic, Arc::new(msg)).await?;

            // Forward received txs to index handlers
            while let Ok((_, message)) = subscription.read().await {
                if let Message::Cardano((block, CardanoMessage::ReceivedTxs(txs_msg))) =
                    message.as_ref()
                {
                    // Call handle_onchain_tx on the index for all decoded txs
                    {
                        let mut idx = index.lock().await;
                        for raw_tx in &txs_msg.txs {
                            let tx = MultiEraTx::decode(raw_tx)?;
                            idx.handle_onchain_tx(block, &tx).await?;
                        }
                    }

                    // Update and save tip
                    let new_tip = Point::Specific(block.slot, block.hash);
                    {
                        *tip.lock().await = new_tip.clone();
                        cursor_store.lock().await.save(&new_tip).await?;
                    }
                }
            }

            Ok::<_, anyhow::Error>(())
        });

        Ok(())
    }
}
