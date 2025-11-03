mod configuration;
mod connection;
mod network;

use acropolis_common::{
    BlockInfo, BlockStatus,
    genesis_values::GenesisValues,
    messages::{CardanoMessage, Message, RawBlockMessage},
};
use anyhow::{Result, bail};
use caryatid_sdk::{Context, Module, Subscription, module};
use config::Config;
use pallas::network::miniprotocols::Point;
use tokio::sync::mpsc;

use std::{sync::Arc, time::Duration};

use crate::{
    configuration::{InterfaceConfig, SyncPoint},
    connection::Header,
    network::NetworkManager,
};

#[module(
    message_type(Message),
    name = "peer-network-interface",
    description = "Mini-protocol chain fetcher from several upstream nodes"
)]
pub struct PeerNetworkInterface;

impl PeerNetworkInterface {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = InterfaceConfig::try_load(&config)?;
        let genesis_complete = if cfg.genesis_values.is_none() {
            Some(context.subscribe(&cfg.genesis_completion_topic).await?)
        } else {
            None
        };
        let snapshot_complete = match cfg.sync_point {
            SyncPoint::Snapshot => Some(context.subscribe(&cfg.snapshot_completion_topic).await?),
            _ => None,
        };
        let (events_sender, events) = mpsc::channel(1024);

        context.clone().run(async move {
            let genesis_values = if let Some(mut sub) = genesis_complete {
                Self::wait_genesis_completion(&mut sub)
                    .await
                    .expect("could not fetch genesis values")
            } else {
                cfg.genesis_values.expect("genesis values not found")
            };
            let sink = BlockSink {
                context,
                topic: cfg.block_topic,
                genesis_values,
            };

            let mut manager = NetworkManager::new(cfg.magic_number, events, events_sender, sink);
            for address in cfg.node_addresses {
                manager.handle_new_connection(address, Duration::ZERO).await;
            }

            match cfg.sync_point {
                SyncPoint::Origin => manager.sync_to_point(Point::Origin).await,
                SyncPoint::Tip => manager.sync_to_tip().await?,
                SyncPoint::Cache => unimplemented!(),
                SyncPoint::Snapshot => {
                    let mut subscription =
                        snapshot_complete.expect("Snapshot topic subscription missing");
                    let point = Self::wait_snapshot_completion(&mut subscription).await?;
                    manager.sync_to_point(point).await;
                }
            }

            manager.run().await
        });

        Ok(())
    }

    async fn wait_genesis_completion(
        subscription: &mut Box<dyn Subscription<Message>>,
    ) -> Result<GenesisValues> {
        let (_, message) = subscription.read().await?;
        match message.as_ref() {
            Message::Cardano((_, CardanoMessage::GenesisComplete(complete))) => {
                Ok(complete.values.clone())
            }
            msg => bail!("Unexpected message in genesis completion topic: {msg:?}"),
        }
    }

    async fn wait_snapshot_completion(
        subscription: &mut Box<dyn Subscription<Message>>,
    ) -> Result<Point> {
        let (_, message) = subscription.read().await?;
        match message.as_ref() {
            Message::Cardano((block, CardanoMessage::SnapshotComplete)) => {
                Ok(Point::Specific(block.slot, block.hash.to_vec()))
            }
            msg => bail!("Unexpected message in snapshot completion topic: {msg:?}"),
        }
    }
}

struct BlockSink {
    context: Arc<Context<Message>>,
    topic: String,
    genesis_values: GenesisValues,
}
impl BlockSink {
    pub async fn announce(&self, header: &Header, body: &[u8], rolled_back: bool) -> Result<()> {
        let info = self.make_block_info(header, rolled_back);
        let available = CardanoMessage::BlockAvailable(RawBlockMessage {
            header: header.bytes.clone(),
            body: body.to_vec(),
        });
        let message = Arc::new(Message::Cardano((info, available)));
        self.context.publish(&self.topic, message).await
    }

    fn make_block_info(&self, header: &Header, rolled_back: bool) -> BlockInfo {
        let slot = header.slot;
        let (epoch, epoch_slot) = self.genesis_values.slot_to_epoch(slot);
        let new_epoch = slot == self.genesis_values.epoch_to_first_slot(epoch);
        let timestamp = self.genesis_values.slot_to_timestamp(slot);
        BlockInfo {
            status: if rolled_back {
                BlockStatus::RolledBack
            } else {
                BlockStatus::Volatile
            },
            slot,
            number: header.number,
            hash: header.hash,
            epoch,
            epoch_slot,
            new_epoch,
            timestamp,
            era: header.era,
        }
    }
}
