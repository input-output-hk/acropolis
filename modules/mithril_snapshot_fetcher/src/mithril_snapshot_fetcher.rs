//! Acropolis Mithril snapshot fetcher module for Caryatid
//! Fetches a snapshot from Mithril and replays all the blocks in it

use acropolis_common::{
    genesis_values::GenesisValues,
    messages::{BlockBodyMessage, BlockHeaderMessage, CardanoMessage, Message},
    BlockInfo, BlockStatus, Era, NetworkId,
};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::{module, Context, Module};
use chrono::{Duration, Utc};
use config::Config;
use mithril_client::{
    feedback::{FeedbackReceiver, MithrilEvent},
    ClientBuilder, MessageBuilder, Snapshot,
};
use pallas::{
    ledger::traverse::{Era as PallasEra, MultiEraBlock},
    storage::hardano,
};
use std::fs::{self, File};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration as SystemDuration;
use tokio::{join, sync::Mutex};
use tracing::{debug, error, info, info_span, Instrument};

mod pause;
use pause::PauseType;

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
const DEFAULT_PAUSE: (&str, PauseType) = ("pause", PauseType::NoPause);
const DEFAULT_DOWNLOAD_MAX_AGE: &str = "download-max-age";
const DEFAULT_DIRECTORY: &str = "downloads";
const SNAPSHOT_METADATA_FILE: &str = "snapshot_metadata.json";
const DEFAULT_NETWORK_ID: &str = "mainnet";

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
    fn load_snapshot_metadata(path: &Path) -> Result<Snapshot> {
        let snapshot_metadata_file = File::open(path)?;
        let snapshot_metadata = serde_json::from_reader::<_, Snapshot>(snapshot_metadata_file)?;
        Ok(snapshot_metadata)
    }

    fn save_snapshot_metadata(snapshot: &Snapshot, path: &Path) -> Result<()> {
        let stringified_snapshot = serde_json::to_string_pretty(snapshot)?;
        let mut snapshot_metadata_file = File::create(path)?;
        snapshot_metadata_file.write_all(stringified_snapshot.as_bytes())?;
        snapshot_metadata_file.flush()?;

        Ok(())
    }

    fn should_skip_download(
        old_snapshot_metadata: &Snapshot,
        latest_snapshot_metadata: &Snapshot,
        config: &Config,
    ) -> bool {
        let download_max_age = config.get::<u64>(DEFAULT_DOWNLOAD_MAX_AGE);

        match download_max_age {
            Ok(download_max_age) => {
                if download_max_age == 0 {
                    info!("Always download snapshot. Download max age is 0");
                    return false;
                }

                let now = Utc::now();
                if (now - old_snapshot_metadata.created_at)
                    > Duration::hours(download_max_age as i64)
                {
                    info!("Snapshot is expired by download max age: {download_max_age} hours");
                    if latest_snapshot_metadata.digest != old_snapshot_metadata.digest
                        && latest_snapshot_metadata.created_at > old_snapshot_metadata.created_at
                    {
                        info!("Latest snapshot is available and newer than the old snapshot");
                        false
                    } else {
                        info!("SKIP DOWNLOAD: Newer snapshot is not available");
                        true
                    }
                } else {
                    info!(
                        "SKIP DOWNLOAD: Snapshot is not expired by download max age: {download_max_age} hours"
                    );
                    true
                }
            }
            Err(error) => {
                info!("SKIP DOWNLOAD: Download max age is not set or invalid: {error:?}");
                true
            }
        }
    }

    /// Fetch and unpack a snapshot
    async fn download_snapshot(config: Arc<Config>) -> Result<()> {
        let aggregator_url =
            config.get_string("aggregator-url").unwrap_or(DEFAULT_AGGREGATOR_URL.to_string());
        let genesis_key =
            config.get_string("genesis-key").unwrap_or(DEFAULT_GENESIS_KEY.to_string());
        let directory = config.get_string("directory").unwrap_or(DEFAULT_DIRECTORY.to_string());
        let snapshot_metadata_path = Path::new(&directory).join(SNAPSHOT_METADATA_FILE);

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

        // Check if the snapshot is expired by download max age
        let old_snapshot = Self::load_snapshot_metadata(&snapshot_metadata_path);
        if let Ok(old_snapshot) = old_snapshot {
            if Self::should_skip_download(&old_snapshot, &snapshot, &config) {
                info!("Using old Mithril snapshot {old_snapshot:?}");
                return Ok(());
            }
        }

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

        // Save snapshot metadata as JSON
        if let Err(e) = Self::save_snapshot_metadata(&snapshot, &snapshot_metadata_path) {
            error!("Failed to save snapshot metadata: {e}");
        }

        // Verify the snapshot
        let message = MessageBuilder::new().compute_snapshot_message(&certificate, dir).await?;

        if !certificate.match_message(&message) {
            return Err(anyhow!("Snapshot verification failed"));
        }

        Ok(())
    }

    /// Process the snapshot
    async fn process_snapshot(
        context: Arc<Context<Message>>,
        config: Arc<Config>,
        genesis: GenesisValues,
    ) -> Result<()> {
        let header_topic =
            config.get_string("header-topic").unwrap_or(DEFAULT_HEADER_TOPIC.to_string());
        let body_topic = config.get_string("body-topic").unwrap_or(DEFAULT_BODY_TOPIC.to_string());
        let completion_topic =
            config.get_string("completion-topic").unwrap_or(DEFAULT_COMPLETION_TOPIC.to_string());
        let directory = config.get_string("directory").unwrap_or(DEFAULT_DIRECTORY.to_string());
        let mut pause_constraint =
            PauseType::from_config(&config, DEFAULT_PAUSE).unwrap_or(PauseType::NoPause);

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

                        let (epoch, epoch_slot) = genesis.slot_to_epoch(slot);
                        let new_epoch = match last_epoch {
                            Some(last_epoch) => epoch != last_epoch,
                            None => true,
                        };
                        last_epoch = Some(epoch);

                        if new_epoch {
                            info!(epoch, number, slot, "New epoch");
                        }

                        let timestamp = genesis.slot_to_timestamp(slot);

                        let network_id = NetworkId::from(
                            config
                                .get_string("network-id")
                                .unwrap_or(DEFAULT_NETWORK_ID.to_string()),
                        );

                        let era = match block.era() {
                            PallasEra::Byron => Era::Byron,
                            PallasEra::Shelley => Era::Shelley,
                            PallasEra::Allegra => Era::Allegra,
                            PallasEra::Mary => Era::Mary,
                            PallasEra::Alonzo => Era::Alonzo,
                            PallasEra::Babbage => Era::Babbage,
                            PallasEra::Conway => Era::Conway,
                            x => bail!(
                                "Block slot {slot}, number {number} has impossible era: {x:?}"
                            ),
                        };

                        let block_info = BlockInfo {
                            status: BlockStatus::Immutable,
                            slot,
                            number,
                            hash: *block.hash(),
                            epoch,
                            epoch_slot,
                            new_epoch,
                            network_id,
                            timestamp,
                            era,
                        };

                        // Check pause constraint
                        if pause_constraint.should_pause(&block_info) {
                            let description = pause_constraint.get_description();
                            let next_pause_constraint = pause_constraint.get_next();
                            let next_description = next_pause_constraint.get_description();
                            if prompt_pause(description, next_description).await {
                                info!("Continuing without further pauses...");
                                pause_constraint = PauseType::NoPause;
                            } else {
                                pause_constraint = next_pause_constraint;
                            }
                        }

                        // Send the block header message
                        let header = block.header();
                        let header_message = BlockHeaderMessage {
                            raw: header.cbor().to_vec(),
                        };

                        let header_message_enum = Message::Cardano((
                            block_info.clone(),
                            CardanoMessage::BlockHeader(header_message),
                        ));
                        let header_future = context
                            .message_bus
                            .publish(&header_topic, Arc::new(header_message_enum));

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
                    }
                    .instrument(span)
                    .await?;
                }
                Err(e) => error!("Error reading block: {e}"),
            }
        }

        // Send completion message
        if let Some(last_block_info) = last_block_info {
            info!(
                "Finished shapshot at block {}, epoch {}",
                last_block_info.number, last_block_info.epoch
            );
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
            let Ok((_, startup_message)) = subscription.read().await else {
                return;
            };
            info!("Received startup message");
            let genesis = match startup_message.as_ref() {
                Message::Cardano((_, CardanoMessage::GenesisComplete(complete))) => {
                    complete.values.clone()
                }
                x => panic!("unexpected startup message: {x:?}"),
            };

            let mut delay = 1;
            loop {
                match Self::download_snapshot(config.clone()).await {
                    Err(e) => error!("Failed to fetch Mithril snapshot: {e}"),
                    _ => {
                        break;
                    }
                }
                info!("Will retry in {delay}s");
                sleep(SystemDuration::from_secs(delay));
                info!("Retrying snapshot download");
                delay = (delay * 2).min(60);
            }

            match Self::process_snapshot(context, config, genesis).await {
                Err(e) => error!("Failed to process Mithril snapshot: {e}"),
                _ => {}
            }
        });

        Ok(())
    }
}

/// Async helper to prompt user for pause behavior
async fn prompt_pause(description: String, next_description: String) -> bool {
    info!(
        "Paused at {description}. Press [Enter] to step to {next_description}, or [c + Enter] to continue without pauses."
    );
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

#[cfg(test)]
mod tests {
    use super::*;
    use mithril_common::test::double::Dummy;

    #[test]
    fn can_save_and_load_snapshot_metadata() {
        let snapshot = Snapshot::dummy();
        let path = Path::new("/tmp/snapshot_metadata.json");
        let result = MithrilSnapshotFetcher::save_snapshot_metadata(&snapshot, path);
        assert!(result.is_ok());
        let result = MithrilSnapshotFetcher::load_snapshot_metadata(path);
        assert!(result.is_ok());
        let loaded_snapshot = result.unwrap();
        assert_eq!(snapshot.digest, loaded_snapshot.digest);
        assert_eq!(snapshot.created_at, loaded_snapshot.created_at);
        assert_eq!(snapshot.size, loaded_snapshot.size);
    }

    #[test]
    fn test_never_skip_download() {
        let old_snapshot_metadata = Snapshot::dummy();
        let config =
            Config::builder().set_override("download-max-age", 0).unwrap().build().unwrap();
        let latest_snapshot_metadata = Snapshot::dummy();
        assert!(!MithrilSnapshotFetcher::should_skip_download(
            &old_snapshot_metadata,
            &latest_snapshot_metadata,
            &config
        ));
    }

    #[test]
    fn test_should_skip_download_if_not_expired() {
        let old_snapshot_metadata = Snapshot {
            created_at: Utc::now() - Duration::hours(2),
            ..Snapshot::dummy()
        };
        let config =
            Config::builder().set_override("download-max-age", 8).unwrap().build().unwrap();
        let latest_snapshot_metadata = Snapshot {
            created_at: Utc::now(),
            ..Snapshot::dummy()
        };
        assert!(MithrilSnapshotFetcher::should_skip_download(
            &old_snapshot_metadata,
            &latest_snapshot_metadata,
            &config
        ));
    }

    #[test]
    fn test_should_skip_download_if_no_new_snapshot_available() {
        let old_snapshot_metadata = Snapshot {
            created_at: Utc::now() - Duration::hours(10),
            digest: "old_snapshot_digest".to_string(),
            ..Snapshot::dummy()
        };
        let config =
            Config::builder().set_override("download-max-age", 8).unwrap().build().unwrap();
        let latest_snapshot_metadata = Snapshot {
            created_at: Utc::now() - Duration::hours(10),
            digest: "old_snapshot_digest".to_string(),
            ..Snapshot::dummy()
        };
        assert!(MithrilSnapshotFetcher::should_skip_download(
            &old_snapshot_metadata,
            &latest_snapshot_metadata,
            &config
        ));
    }

    #[test]
    fn test_should_not_skip_download_if_new_snapshot_available() {
        let old_snapshot_metadata = Snapshot {
            created_at: Utc::now() - Duration::hours(10),
            digest: "old_snapshot_digest".to_string(),
            ..Snapshot::dummy()
        };
        let config =
            Config::builder().set_override("download-max-age", 8).unwrap().build().unwrap();
        let latest_snapshot_metadata = Snapshot {
            created_at: Utc::now() - Duration::hours(2),
            digest: "new_snapshot_digest".to_string(),
            ..Snapshot::dummy()
        };
        assert!(!MithrilSnapshotFetcher::should_skip_download(
            &old_snapshot_metadata,
            &latest_snapshot_metadata,
            &config
        ));
    }
}
