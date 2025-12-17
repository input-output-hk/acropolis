//! Acropolis fake block injector module for Caryatid
//! Posts test blocks into the Acropolis system after bootstrapping

use acropolis_codec::map_to_block_era;
use acropolis_common::{
    commands::chain_sync::ChainSyncCommand,
    genesis_values::GenesisValues,
    messages::{CardanoMessage, Command, Message, RawBlockMessage},
    BlockHash, BlockInfo, BlockIntent, BlockStatus,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::{module, Context};
use config::Config;
use glob::glob;
use pallas_traverse::MultiEraBlock;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};

const CONFIG_BOOTSTRAPPED_TOPIC: (&str, &str) = (
    "bootstrapped-subscribe-topic",
    "cardano.sequence.bootstrapped",
);
const CONFIG_COMPLETION_TOPIC: (&str, &str) = ("completion-topic", "cardano.sync.command");
const CONFIG_BLOCK_PUBLISH_TOPIC: (&str, &str) = ("block-publish-topic", "cardano.block.available");

/// Fake block injector module
#[module(
    message_type(Message),
    name = "fake-block-injector",
    description = "Fake block injector"
)]
pub struct FakeBlockInjector;

impl FakeBlockInjector {
    /// Publish a block
    async fn process_block(
        context: Arc<Context<Message>>,
        raw_block: Vec<u8>,
        block_publish_topic: String,
        genesis: &GenesisValues,
    ) -> Result<()> {
        // Decode it
        let block = MultiEraBlock::decode(&raw_block)?;
        let slot = block.slot();
        let number = block.number();

        let (epoch, epoch_slot) = genesis.slot_to_epoch(slot);
        let new_epoch = false; // TODO
        let timestamp = genesis.slot_to_timestamp(slot);
        let era = map_to_block_era(&block)?;

        let block_info = BlockInfo {
            status: BlockStatus::Volatile,
            // Consensus will set the Validate bit if wanted
            intent: BlockIntent::Apply,
            slot,
            number,
            hash: BlockHash::from(*block.hash()),
            epoch,
            epoch_slot,
            new_epoch,
            timestamp,
            tip_slot: None,
            era,
        };

        info!("  -> block {number}, slot {slot}");

        // Send the block message
        let message = RawBlockMessage {
            header: block.header().cbor().to_vec(),
            body: raw_block,
        };

        let message_enum =
            Message::Cardano((block_info.clone(), CardanoMessage::BlockAvailable(message)));

        context
            .message_bus
            .publish(&block_publish_topic, Arc::new(message_enum))
            .await
            .unwrap_or_else(|e| error!("Failed to publish block message: {e}"));

        Ok(())
    }

    /// Read and publish all the blocks
    async fn process_blocks(
        context: Arc<Context<Message>>,
        config: Arc<Config>,
        genesis: &GenesisValues,
    ) -> Result<()> {
        let block_publish_topic = config
            .get_string(CONFIG_BLOCK_PUBLISH_TOPIC.0)
            .unwrap_or(CONFIG_BLOCK_PUBLISH_TOPIC.1.to_string());
        info!("Publishing blocks on '{block_publish_topic}'");

        if let Ok(file_pattern) = config.get_string("block-files") {
            // Scan directory
            let mut files: Vec<PathBuf> = glob(&file_pattern)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e.msg))?
                .collect::<Result<_, _>>()
                .map_err(io::Error::other)?;

            // Sort into lexicographic order
            files.sort();

            // Read and process them
            for path in files {
                info!("  {}", path.display());
                let raw_block = fs::read(&path)?; // Vec<u8>
                Self::process_block(
                    context.clone(),
                    raw_block,
                    block_publish_topic.clone(),
                    genesis,
                )
                .await?;
            }

            Ok(())
        } else {
            // TODO - other options like constructing blocks from explicit Tx in config
            error!("No block-files pattern given");
            Err(anyhow!("No block-files"))
        }
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let bootstrapped_topic = config
            .get_string(CONFIG_BOOTSTRAPPED_TOPIC.0)
            .unwrap_or(CONFIG_BOOTSTRAPPED_TOPIC.1.to_string());
        info!("Creating subscriber for bootstrapped on '{bootstrapped_topic}'");
        let mut bootstrapped_subscription = context.subscribe(&bootstrapped_topic).await?;

        let completion_topic = config
            .get_string(CONFIG_COMPLETION_TOPIC.0)
            .unwrap_or(CONFIG_COMPLETION_TOPIC.1.to_string());
        info!("Creating subscriber for completion on '{completion_topic}'");
        let mut completion_subscription = context.subscribe(&completion_topic).await?;

        context.clone().run(async move {
            // Wait for bootstrapped first - immediately sent by genesis bootstrapper
            let Ok((_, bootstrapped_message)) = bootstrapped_subscription.read().await else {
                return;
            };
            info!("Received bootstrapped message");
            let genesis = match bootstrapped_message.as_ref() {
                Message::Cardano((_, CardanoMessage::GenesisComplete(complete))) => {
                    complete.values.clone()
                }
                x => panic!("unexpected bootstrapped message: {x:?}"),
            };

            // Then wait for completion of Mithril / Snapshot bootstrap to get the last
            // block read
            let Ok((_, completion_message)) = completion_subscription.read().await else {
                return;
            };
            info!("Received completion message");
            let point = match completion_message.as_ref() {
                Message::Command(Command::ChainSync(ChainSyncCommand::FindIntersect(point))) => {
                    point
                }
                x => panic!("unexpected completion message: {x:?}"),
            };
            info!(slot = point.slot(), "Snapshot completed");

            // Send out the blocks
            if let Err(e) = Self::process_blocks(context, config, &genesis).await {
                error!("Failed to process blocks: {e}");
            }
        });

        Ok(())
    }
}
