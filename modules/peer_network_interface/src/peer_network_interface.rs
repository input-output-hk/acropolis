mod chain_state;
mod configuration;
mod connection;
mod network;

use acropolis_common::{
    BlockInfo, BlockIntent, BlockStatus,
    commands::chain_sync::ChainSyncCommand,
    genesis_values::GenesisValues,
    messages::{CardanoMessage, Command, Message, RawBlockMessage, StateTransitionMessage},
    upstream_cache::{UpstreamCache, UpstreamCacheRecord},
};
use anyhow::{Result, bail};
use caryatid_sdk::{Context, Subscription, module};
use config::Config;
use pallas::network::miniprotocols::Point;
use tokio::sync::mpsc;
use tracing::{error, warn};

use std::{path::Path, sync::Arc, time::Duration};

use crate::{
    configuration::{InterfaceConfig, SyncPoint},
    connection::Header,
    network::{NetworkEvent, NetworkManager},
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
        let mut command_subscription = context.subscribe(&cfg.sync_command_topic).await?;

        context.clone().run(async move {
            let genesis_values = if let Some(mut sub) = genesis_complete {
                Self::wait_genesis_completion(&mut sub)
                    .await
                    .expect("could not fetch genesis values")
            } else {
                cfg.genesis_values.clone().expect("genesis values not found")
            };

            let mut upstream_cache = None;
            let mut last_epoch = None;
            let mut cache_sync_point = Point::Origin;
            if cfg.sync_point == SyncPoint::Cache {
                match Self::init_cache(&cfg.cache_dir, &cfg.block_topic, &context).await {
                    Ok((cache, sync_point)) => {
                        upstream_cache = Some(cache);
                        if let Point::Specific(slot, _) = sync_point {
                            let (epoch, _) = genesis_values.slot_to_epoch(slot);
                            last_epoch = Some(epoch);
                        }
                        cache_sync_point = sync_point;
                    }
                    Err(e) => {
                        warn!("could not initialize upstream cache: {e:#}");
                    }
                }
            }

            let mut sink = BlockSink {
                context,
                topic: cfg.block_topic.clone(),
                genesis_values: genesis_values.clone(),
                upstream_cache,
                last_epoch,
                rolled_back: false,
            };

            let manager = match cfg.sync_point {
                SyncPoint::Origin => {
                    tracing::info!("Starting sync from origin");
                    let mut manager = Self::init_manager(
                        cfg.node_addresses,
                        genesis_values.magic_number,
                        sink,
                        command_subscription,
                    );
                    manager.sync_to_point(Point::Origin);
                    manager
                }
                SyncPoint::Tip => {
                    tracing::info!("Starting sync from tip");
                    let mut manager = Self::init_manager(
                        cfg.node_addresses,
                        genesis_values.magic_number,
                        sink,
                        command_subscription,
                    );
                    if let Err(error) = manager.sync_to_tip().await {
                        warn!("could not sync to tip: {error:#}");
                        return;
                    }
                    manager
                }
                SyncPoint::Cache => {
                    tracing::info!("Starting sync from cache at {:?}", cache_sync_point);
                    let mut manager = Self::init_manager(
                        cfg.node_addresses,
                        genesis_values.magic_number,
                        sink,
                        command_subscription,
                    );
                    manager.sync_to_point(cache_sync_point);
                    manager
                }
                SyncPoint::Dynamic => {
                    match Self::wait_initial_command(&mut command_subscription).await {
                        Ok(point) => {
                            if let Point::Specific(slot, _) = &point {
                                let (epoch, _) = sink.genesis_values.slot_to_epoch(*slot);
                                sink.last_epoch = Some(epoch);
                                tracing::info!(
                                    "Starting sync from slot {} in epoch {}",
                                    slot,
                                    epoch,
                                );
                            }
                            let mut manager = Self::init_manager(
                                cfg.node_addresses,
                                genesis_values.magic_number,
                                sink,
                                command_subscription,
                            );
                            manager.sync_to_point(point);
                            manager
                        }
                        Err(error) => {
                            warn!("sync command never received: {error:#}");
                            return;
                        }
                    }
                }
            };

            if let Err(err) = manager.run().await {
                error!("chain sync failed: {err:#}");
            }
        });

        Ok(())
    }

    fn init_manager(
        node_addresses: Vec<String>,
        magic_number: u32,
        sink: BlockSink,
        command_subscription: Box<dyn Subscription<Message>>,
    ) -> NetworkManager {
        let (events_sender, events) = mpsc::channel(1024);
        tokio::spawn(Self::forward_commands_to_events(
            command_subscription,
            events_sender.clone(),
        ));
        let mut manager = NetworkManager::new(magic_number, events, events_sender, sink);
        for address in node_addresses {
            manager.handle_new_connection(address, Duration::ZERO);
        }
        manager
    }

    async fn forward_commands_to_events(
        mut subscription: Box<dyn Subscription<Message>>,
        events_sender: mpsc::Sender<NetworkEvent>,
    ) -> Result<()> {
        while let Ok((_, msg)) = subscription.read().await {
            if let Message::Command(Command::ChainSync(ChainSyncCommand::FindIntersect(p))) =
                msg.as_ref()
            {
                let point = match p {
                    acropolis_common::Point::Origin => Point::Origin,
                    acropolis_common::Point::Specific { hash, slot } => {
                        Point::Specific(*slot, hash.to_vec())
                    }
                };

                if events_sender.send(NetworkEvent::SyncPointUpdate { point }).await.is_err() {
                    bail!("event channel closed");
                }
            }
        }

        bail!("subscription closed");
    }

    async fn init_cache(
        cache_dir: &Path,
        block_topic: &str,
        context: &Context<Message>,
    ) -> Result<(UpstreamCache, Point)> {
        let mut cache = UpstreamCache::new(cache_dir)?;
        let mut cache_sync_point = None;
        cache.start_reading()?;
        while let Some(record) = cache.read_record()? {
            cache_sync_point = Some((record.id.slot, record.id.hash));
            let message = Arc::new(Message::Cardano((
                record.id,
                CardanoMessage::BlockAvailable(Arc::unwrap_or_clone(record.message)),
            )));
            context.message_bus.publish(block_topic, message).await?;
        }
        let sync_point = match cache_sync_point {
            None => Point::Origin,
            Some((slot, hash)) => Point::Specific(slot, hash.to_vec()),
        };
        Ok((cache, sync_point))
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

    async fn wait_initial_command(
        subscription: &mut Box<dyn Subscription<Message>>,
    ) -> Result<Point> {
        let (_, message) = subscription.read().await?;
        match message.as_ref() {
            Message::Command(Command::ChainSync(ChainSyncCommand::FindIntersect(point))) => {
                match point {
                    acropolis_common::Point::Origin => Ok(Point::Origin),
                    acropolis_common::Point::Specific { hash, slot } => {
                        Ok(Point::Specific(*slot, hash.to_vec()))
                    }
                }
            }
            msg => bail!("Unexpected message in sync command topic: {msg:?}"),
        }
    }
}

struct BlockSink {
    context: Arc<Context<Message>>,
    topic: String,
    genesis_values: GenesisValues,
    upstream_cache: Option<UpstreamCache>,
    last_epoch: Option<u64>,
    rolled_back: bool,
}
impl BlockSink {
    pub async fn announce_roll_forward(
        &mut self,
        header: &Header,
        body: &[u8],
        tip: Option<&Point>,
    ) -> Result<()> {
        let info = self.make_block_info(header, tip);
        let raw_block = RawBlockMessage {
            header: header.bytes.clone(),
            body: body.to_vec(),
        };
        if let Some(cache) = self.upstream_cache.as_mut() {
            let record = UpstreamCacheRecord {
                id: BlockInfo {
                    tip_slot: None, // when replaying, we don't care where the tip was
                    ..info.clone()
                },
                message: Arc::new(raw_block.clone()),
            };
            cache.write_record(&record)?;
        }
        let message = Arc::new(Message::Cardano((
            info,
            CardanoMessage::BlockAvailable(raw_block),
        )));
        self.context.publish(&self.topic, message).await?;
        self.rolled_back = false;
        Ok(())
    }

    pub async fn announce_roll_backward(
        &mut self,
        header: &Header,
        tip: Option<&Point>,
    ) -> Result<()> {
        self.rolled_back = true;
        let info = self.make_block_info(header, tip);
        let point = acropolis_common::Point::Specific {
            hash: info.hash,
            slot: info.slot,
        };
        let message = Arc::new(Message::Cardano((
            info,
            CardanoMessage::StateTransition(StateTransitionMessage::Rollback(point)),
        )));
        self.context.publish(&self.topic, message).await
    }

    fn make_block_info(&mut self, header: &Header, tip: Option<&Point>) -> BlockInfo {
        let slot = header.slot;
        let (epoch, epoch_slot) = self.genesis_values.slot_to_epoch(slot);
        let new_epoch = self.last_epoch != Some(epoch);
        self.last_epoch = Some(epoch);
        let timestamp = self.genesis_values.slot_to_timestamp(slot);
        BlockInfo {
            status: if self.rolled_back {
                BlockStatus::RolledBack
            } else {
                BlockStatus::Volatile
            },
            intent: BlockIntent::Apply,
            slot,
            number: header.number,
            hash: header.hash,
            epoch,
            epoch_slot,
            new_epoch,
            tip_slot: tip.map(|p| p.slot_or_default()),
            timestamp,
            era: header.era,
        }
    }
}
