//! Acropolis Mithril snapshot fetcher module for Caryatid
//! Fetches a snapshot from Mithril and replays all the blocks in it

use acropolis_common::{
    calculations::slot_to_epoch,
    messages::{BlockBodyMessage, BlockHeaderMessage, CardanoMessage, Message},
    BlockInfo, BlockStatus, Era,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::{module, Context, Module};
use config::Config;
use mithril_client::{
    feedback::{FeedbackReceiver, MithrilEvent},
    ClientBuilder, MessageBuilder,
};
use pallas::{
    ledger::traverse::{Era as PallasEra, MultiEraBlock},
    storage::hardano,
};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use tokio::{join, sync::Mutex};
use tracing::{debug, error, info, info_span, Instrument};

const DEFAULT_STARTUP_TOPIC: &str = "cardano.sequence.bootstrapped";
const DEFAULT_HEADER_TOPIC: &str = "cardano.block.header";
const DEFAULT_BODY_TOPIC: &str = "cardano.block.body";
const DEFAULT_COMPLETION_TOPIC: &str = "cardano.snapshot.complete";

const DEFAULT_AGGREGATOR_URL: &str =
    "https://aggregator.release-mainnet.api.mithril.network/aggregator";
const DEFAULT_GENESIS_KEY: &str = r#"
5b3139312c36362c3134302c3138352c3133382c31312c3233372c3230372c3235302c3134342c32
372c322c3138382c33302c31322c38312c3135352c3230342c31302c3137392c37352c32332c3133
382c3139362c3231372c352c31342c32302c35372c37392c33392c3137365d"#;
const DEFAULT_PAUSE_EPOCH: (&str, i64) = ("pause-epoch", -1);
const DEFAULT_DIRECTORY: &str = "downloads";

/// Mithril feedback receiver
struct FeedbackLogger {
    last_percentage: Arc<Mutex<u64>>,
}

impl FeedbackLogger {
    fn new() -> Self {
        Self {
            last_percentage: Arc::new(Mutex::new(0)),
        }
    }
}

#[async_trait::async_trait]
impl FeedbackReceiver for FeedbackLogger {
    async fn handle_event(&self, event: MithrilEvent) {
        #[allow(unreachable_patterns)] // To allow _ in cases where we do cover everything
        match event {
            MithrilEvent::SnapshotDownloadStarted { size, .. } => {
                info!("Started snapshot download - {size} bytes");
            }
            MithrilEvent::SnapshotDownloadProgress {
                downloaded_bytes: bytes,
                size,
                ..
            } => {
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
            MithrilEvent::CertificateValidated {
                certificate_hash, ..
            } => {
                info!("Validated certificate {certificate_hash}");
            }
            MithrilEvent::CertificateChainValidated { .. } => {
                info!("Certificate chain validated OK");
            }
            MithrilEvent::CertificateFetchedFromCache {
                certificate_hash, ..
            } => {
                info!("Fetched certificate {certificate_hash} from cache");
            }

            _ => {} // Catchall for future updates
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

impl MithrilSnapshotFetcher {
    /// Fetch and unpack a snapshot
    async fn download_snapshot(config: Arc<Config>) -> Result<()> {
        let aggregator_url =
            config.get_string("aggregator-url").unwrap_or(DEFAULT_AGGREGATOR_URL.to_string());
        let genesis_key =
            config.get_string("genesis-key").unwrap_or(DEFAULT_GENESIS_KEY.to_string());
        let directory = config.get_string("directory").unwrap_or(DEFAULT_DIRECTORY.to_string());

        let feedback_logger = Arc::new(FeedbackLogger::new());
        let client = ClientBuilder::aggregator(&aggregator_url, &genesis_key)
            .add_feedback_receiver(feedback_logger)
            .build()?;

        // Find the latest snapshot
        let snapshots = client.cardano_database().list().await?;
        let latest_snapshot = snapshots.first().ok_or(anyhow!("No snapshots available"))?;
        let snapshot = client
            .cardano_database()
            .get(&latest_snapshot.digest)
            .await?
            .ok_or(anyhow!("No snapshot for digest {}", latest_snapshot.digest))?;
        info!("Using Mithril snapshot {snapshot:?}");

        // Verify the certificate chain
        let certificate = client.certificate().verify_chain(&snapshot.certificate_hash).await?;

        // Download the snapshot
        fs::create_dir_all(&directory)?;
        let dir = Path::new(&directory);
        client.cardano_database().download_unpack(&snapshot, &dir).await?;

        // Register download
        if let Err(e) = client.cardano_database().add_statistics(&snapshot).await {
            error!("Could not increment snapshot download statistics: {:?}", e);
            // But that doesn't affect us...
        }

        // Verify the snapshot
        let message = MessageBuilder::new().compute_snapshot_message(&certificate, dir).await?;

        if !certificate.match_message(&message) {
            return Err(anyhow!("Snapshot verification failed"));
        }

        Ok(())
    }

    /// Process the snapshot
    async fn process_snapshot(context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let header_topic =
            config.get_string("header-topic").unwrap_or(DEFAULT_HEADER_TOPIC.to_string());
        let body_topic = config.get_string("body-topic").unwrap_or(DEFAULT_BODY_TOPIC.to_string());
        let completion_topic =
            config.get_string("completion-topic").unwrap_or(DEFAULT_COMPLETION_TOPIC.to_string());
        let directory = config.get_string("directory").unwrap_or(DEFAULT_DIRECTORY.to_string());
        let mut pause_epoch = load_pause_epoch(&config);

        // Path to immutable DB
        let path = Path::new(&directory).join("immutable");

        // Scan using hardano and output blocks
        if let Some(tip) = hardano::immutable::get_tip(&path)? {
            info!(
                "Snapshot contains blocks up to slot {}",
                tip.slot_or_default()
            );
        }

        let mut last_block_info: Option<BlockInfo> = None;

        let blocks = hardano::immutable::read_blocks(&path)?;
        let mut last_block_number: u64 = 0;
        let mut last_epoch: Option<u64> = None;
        for raw_block in blocks {
            match raw_block {
                Ok(raw_block) => {
                    let span = info_span!("mithril_snapshot_fetcher.raw_block");
                    async {
                        // Decode it
                        // TODO - can we avoid this and still get the slot & number?
                        let block = MultiEraBlock::decode(&raw_block)?;
                        let slot = block.slot();
                        let number = block.number();

                        if tracing::enabled!(tracing::Level::DEBUG) {
                            debug!(number, slot);
                        }

                        // Skip EBBs
                        match block {
                            MultiEraBlock::EpochBoundary(_) => return Ok::<(), anyhow::Error>(()),
                            _ => {}
                        }

                        // Error and ignore any out of sequence
                        if number <= last_block_number && last_block_number != 0 {
                            error!(
                                number,
                                last_block_number, "Rewind of block number in Mithril! Skipped..."
                            );
                            return Ok::<(), anyhow::Error>(());
                        }
                        last_block_number = number;

                        let epoch = slot_to_epoch(slot);
                        let new_epoch = match last_epoch {
                            Some(last_epoch) => epoch != last_epoch,
                            None => true,
                        };
                        last_epoch = Some(epoch);

                        if new_epoch {
                            info!(epoch, number, slot, "New epoch");

                            if let Some(pe) = pause_epoch {
                                if epoch == pe {
                                    if prompt_epoch_pause(epoch).await {
                                        info!("Continuing without further pauses...");
                                        pause_epoch = None;
                                    } else {
                                        pause_epoch = Some(epoch + 1);
                                    }
                                }
                            }
                        }

                        let era = match block.era() {
                            PallasEra::Byron => Era::Byron,
                            PallasEra::Shelley => Era::Shelley,
                            PallasEra::Allegra => Era::Allegra,
                            PallasEra::Mary => Era::Mary,
                            PallasEra::Alonzo => Era::Alonzo,
                            PallasEra::Babbage => Era::Babbage,
                            _ => Era::Conway,
                        };

                        let block_info = BlockInfo {
                            status: BlockStatus::Immutable,
                            slot,
                            number,
                            hash: block.hash().to_vec(),
                            epoch,
                            new_epoch,
                            era,
                        };

                        // Send the block header message
                        let header = block.header();
                        let header_message = BlockHeaderMessage {
                            raw: header.cbor().to_vec(),
                        };

                        let header_message_enum = Message::Cardano((
                            block_info.clone(),
                            CardanoMessage::BlockHeader(header_message),
                        ));
                        let header_future =
                            context.message_bus.publish(&header_topic, Arc::new(header_message_enum));

                        // Send the block body message
                        let body_message = BlockBodyMessage { raw: raw_block };

                        let body_message_enum = Message::Cardano((
                            block_info.clone(),
                            CardanoMessage::BlockBody(body_message),
                        ));
                        let body_future =
                            context.message_bus.publish(&body_topic, Arc::new(body_message_enum));

                        let (header_result, body_result) = join!(header_future, body_future);
                        header_result.unwrap_or_else(|e| error!("Failed to publish header: {e}"));
                        body_result.unwrap_or_else(|e| error!("Failed to publish body: {e}"));

                        last_block_info = Some(block_info);
                        Ok::<(), anyhow::Error>(())
                    }.instrument(span).await?;
                }
                Err(e) => error!("Error reading block: {e}"),
            }
        }

        // Send completion message
        if let Some(last_block_info) = last_block_info {
            info!("Finished shapshot at block {}, epoch {}",
                  last_block_info.number, last_block_info.epoch);
            let message_enum =
                Message::Cardano((last_block_info, CardanoMessage::SnapshotComplete));
            context
                .message_bus
                .publish(&completion_topic, Arc::new(message_enum))
                .await
                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
        }
        Ok(())
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let startup_topic =
            config.get_string("startup-topic").unwrap_or(DEFAULT_STARTUP_TOPIC.to_string());
        info!("Creating startup subscriber on '{startup_topic}'");

        let mut subscription = context.subscribe(&startup_topic).await?;
        context.clone().run(async move {
            let Ok(_) = subscription.read().await else {
                return;
            };
            info!("Received startup message");

            if config.get_bool("download").unwrap_or(true) {
                let mut delay = 1;
                loop {
                    match Self::download_snapshot(config.clone()).await {
                        Err(e) => error!("Failed to fetch Mithril snapshot: {e}"),
                        _ => {
                            break;
                        }
                    }
                    info!("Will retry in {delay}s");
                    sleep(Duration::from_secs(delay));
                    info!("Retrying snapshot download");
                    delay = (delay * 2).min(60);
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

/// Helper to parse pause_epoch from config
fn load_pause_epoch(config: &Config) -> Option<u64> {
    let pause_epoch = config.get_int(DEFAULT_PAUSE_EPOCH.0).unwrap_or(DEFAULT_PAUSE_EPOCH.1);
    (pause_epoch >= 0).then(|| {
        info!("Pausing enabled at epoch {pause_epoch}");
        pause_epoch as u64
    })
}

/// Async helper to prompt user for pause behavior
async fn prompt_epoch_pause(epoch: u64) -> bool {
    info!("Paused at epoch {epoch}. Press [Enter] to step to next epoch, or [c + Enter] to continue without pauses.");
    tokio::task::spawn_blocking(|| {
        use std::io::{self, BufRead};
        let stdin = io::stdin();
        let mut handle = stdin.lock();
        let mut line = String::new();
        handle.read_line(&mut line).unwrap();
        line.trim().eq_ignore_ascii_case("c")
    })
    .await
    .unwrap()
}
