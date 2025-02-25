//! Acropolis Mithril snapshot fetcher module for Caryatid
//! Fetches a snapshot from Mithril and replays all the blocks in it

use caryatid_sdk::{Context, Module, module};
use acropolis_messages::{BlockHeaderMessage, BlockBodyMessage, Message};
use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::{Result, anyhow};
use config::Config;
use tracing::{debug, info, error};
use mithril_client::{
    ClientBuilder, 
    MessageBuilder, 
    feedback::{
        FeedbackReceiver,
        MithrilEvent
    }
};
use std::fs;
use std::path::Path;
use pallas::{ledger::traverse::MultiEraBlock, storage::hardano};

const DEFAULT_HEADER_TOPIC: &str = "cardano.block.header";
const DEFAULT_BODY_TOPIC: &str = "cardano.block.body";
const DEFAULT_AGGREGATOR_URL: &str = 
    "https://aggregator.release-mainnet.api.mithril.network/aggregator";
const DEFAULT_GENESIS_KEY: &str = r#"
5b3139312c36362c3134302c3138352c3133382c31312c3233372c3230372c3235302c3134342c32
372c322c3138382c33302c31322c38312c3135352c3230342c31302c3137392c37352c32332c3133
382c3139362c3231372c352c31342c32302c35372c37392c33392c3137365d"#;
const DEFAULT_DIRECTORY: &str = "downloads";

/// Mithril feedback receiver
struct FeedbackLogger {
    last_percentage: Arc<Mutex<u64>>,
}

impl FeedbackLogger {
    fn new() -> Self {
        Self { last_percentage: Arc::new(Mutex::new(0)) }
    }
}

#[async_trait::async_trait]
impl FeedbackReceiver for FeedbackLogger {
    async fn handle_event(&self, event: MithrilEvent) {
        match event {
            MithrilEvent::SnapshotDownloadStarted { size, .. } => {
                info!("Started snapshot download - {size} bytes");
            }
            MithrilEvent::SnapshotDownloadProgress { downloaded_bytes: bytes, size, .. } => {
                let percentage = bytes * 100 / size;
                let mut last_percentage = self.last_percentage.lock().await;
                if percentage > *last_percentage {
                    info!("Downloaded {percentage}% of the snapshot");
                    *last_percentage = percentage;
                }
            }
            MithrilEvent::SnapshotDownloadCompleted { .. } => {
                info!("Download complete");
            }
            MithrilEvent::CertificateChainValidationStarted { .. } => {
                info!("Started certificate chain validation");
            }
            MithrilEvent::CertificateValidated { certificate_hash, .. } => {
                info!("Validated certificate {certificate_hash}");
            }
            MithrilEvent::CertificateChainValidated { .. } => {
                info!("Certificate chain validated OK");
            }
            MithrilEvent::CertificateFetchedFromCache { certificate_hash, .. } => {
                info!("Fetched certificate {certificate_hash} from cache");
             }
        }
    }
}

/// Mithril snapshot fetcher module
#[module(
    message_type(Message),
    name = "mithril-snapshot-fetcher",
    description = "Mithril snapshot fetcher"
)]
pub struct MithrilSnapshotFetcher;

impl MithrilSnapshotFetcher
{
    /// Fetch and unpack a snapshot
    async fn download_snapshot(config: Arc<Config>) -> Result<()> {
       let aggregator_url = config.get_string("aggregator-url")
            .unwrap_or(DEFAULT_AGGREGATOR_URL.to_string());
        let genesis_key = config.get_string("genesis-key")
            .unwrap_or(DEFAULT_GENESIS_KEY.to_string());
        let directory = config.get_string("directory")
            .unwrap_or(DEFAULT_DIRECTORY.to_string());

        let feedback_logger = Arc::new(FeedbackLogger::new());
        let client = ClientBuilder::aggregator(&aggregator_url, &genesis_key)
            .add_feedback_receiver(feedback_logger)
            .build()?;

        // Find the latest snapshot
        let snapshots = client.snapshot().list().await?;
        let latest_snapshot = snapshots.first()
            .ok_or(anyhow!("No snapshots available"))?;
        let snapshot = client.snapshot().get(&latest_snapshot.digest)
            .await?
            .ok_or(anyhow!("No snapshot for digest {}", latest_snapshot.digest))?;
        info!("Using Mithril snapshot {snapshot:?}");

        // Verify the certificate chain
        let certificate = client
            .certificate()
            .verify_chain(&snapshot.certificate_hash)
            .await?;

        // Download the snapshot
        fs::create_dir_all(&directory)?;
        let dir = Path::new(&directory);
        client.snapshot().download_unpack(&snapshot, &dir)
            .await?;

        // Register download
        if let Err(e) = client.snapshot().add_statistics(&snapshot).await {
            error!("Could not increment snapshot download statistics: {:?}", e);
            // But that doesn't affect us...
        }
    
        // Verify the snapshot
        let message = MessageBuilder::new()
            .compute_snapshot_message(&certificate, dir)
            .await?;

        if !certificate.match_message(&message) {
            return Err(anyhow!("Snapshot verification failed"));
        }

        Ok(())
    }

    /// Process the snapshot
    async fn process_snapshot(context: Arc<Context<Message>>,
                              config: Arc<Config>) -> Result<()> {
        let header_topic = config.get_string("header-topic")
            .unwrap_or(DEFAULT_HEADER_TOPIC.to_string());
        let body_topic = config.get_string("body-topic")
            .unwrap_or(DEFAULT_BODY_TOPIC.to_string());
        let directory = config.get_string("directory")
            .unwrap_or(DEFAULT_DIRECTORY.to_string());

        // Path to immutable DB
        let path = Path::new(&directory).join("immutable");

        // Scan using hardano and output blocks
        if let Some(tip) = hardano::immutable::get_tip(&path)? {
            info!("Snapshot contains blocks up to slot {}", tip.slot_or_default());
        }

        let blocks = hardano::immutable::read_blocks(&path)?;
        for raw_block in blocks {
            match raw_block {
                Ok(raw_block) => {

                    // Decode it
                    let block = MultiEraBlock::decode(&raw_block)?;
                    let slot = block.slot();
                    let number = block.number();

                    if number % 100000 == 0 {
                        info!("Read block number {}, slot {}", number, slot);
                    }

                    // Send the block header message
                    let header = block.header();
                    let header_message = BlockHeaderMessage {
                        slot: slot,
                        number: number,
                        raw: header.cbor().to_vec()
                    };

                    debug!("Mithril snapshot sending {:?}", header_message);

                    let header_message_enum: Message = header_message.into();
                    context.message_bus.publish(&header_topic, Arc::new(header_message_enum))
                        .await
                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));

                    // Send the block body message
                    let body_message = BlockBodyMessage {
                        slot: slot,
                        raw: raw_block
                    };

                    debug!("Mithril snapshot fetcher sending {:?}", body_message);

                    let body_message_enum: Message = body_message.into();
                    context.message_bus.publish(&body_topic, Arc::new(body_message_enum))
                        .await
                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                }
                Err(e) => error!("Error reading block: {e}")
            }
        }

        Ok(())
    }

    /// Main init function
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        tokio::spawn(async move {

            if config.get_bool("download").unwrap_or(true) {
               match Self::download_snapshot(config.clone()).await {
                    Err(e) => error!("Failed to fetch Mithril snapshot: {e}"),
                    _ => {}
                }
            }

            match Self::process_snapshot(context, config).await {
                Err(e) => error!("Failed to process Mithril snapshot: {e}"),
                _ => {}
            }
        });

        Ok(())
    }
}
