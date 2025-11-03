use std::sync::Arc;

use acropolis_common::{
    genesis_values::GenesisValues,
    messages::{CardanoMessage, GenesisCompleteMessage, Message},
    snapshot::{
        streaming_snapshot::{
            DRepCallback, DRepInfo, GovernanceProposal, PoolCallback, PoolInfo, ProposalCallback,
            SnapshotCallbacks, SnapshotMetadata, StakeCallback, UtxoCallback, UtxoEntry,
        },
        StreamingSnapshotParser,
    },
    stake_addresses::AccountState,
    BlockHash, BlockInfo, BlockStatus, Era,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use tokio::time::Instant;
use tracing::{error, info, info_span, Instrument};

const DEFAULT_SNAPSHOT_TOPIC: &str = "cardano.snapshot";
const DEFAULT_STARTUP_TOPIC: &str = "cardano.sequence.start";
const DEFAULT_COMPLETION_TOPIC: &str = "cardano.sequence.bootstrapped";

/// Callback handler that accumulates snapshot data and builds state
struct SnapshotHandler {
    context: Arc<Context<Message>>,
    snapshot_topic: String,

    // Accumulated data from callbacks
    metadata: Option<SnapshotMetadata>,
    utxo_count: u64,
    pools: Vec<PoolInfo>,
    accounts: Vec<AccountState>,
    dreps: Vec<DRepInfo>,
    proposals: Vec<GovernanceProposal>,
}

#[module(
    message_type(Message),
    name = "snapshot-bootstrapper",
    description = "Snapshot Bootstrapper to broadcast state"
)]
pub struct SnapshotBootstrapper;

impl SnapshotHandler {
    fn new(context: Arc<Context<Message>>, snapshot_topic: String) -> Self {
        Self {
            context,
            snapshot_topic,
            metadata: None,
            utxo_count: 0,
            pools: Vec::new(),
            accounts: Vec::new(),
            dreps: Vec::new(),
            proposals: Vec::new(),
        }
    }

    /// Build BlockInfo from accumulated metadata
    fn build_block_info(&self) -> Result<BlockInfo> {
        let metadata =
            self.metadata.as_ref().ok_or_else(|| anyhow::anyhow!("No metadata available"))?;

        // Create a synthetic BlockInfo representing the snapshot state
        // This represents the last block included in the snapshot
        Ok(BlockInfo {
            status: BlockStatus::Immutable, // Snapshot blocks are immutable
            slot: 0,                        // TODO: Extract from snapshot metadata if available
            number: 0,                      // TODO: Extract from snapshot metadata if available
            hash: BlockHash::default(),     // TODO: Extract from snapshot metadata if available
            epoch: metadata.epoch,
            epoch_slot: 0,    // TODO: Extract from snapshot metadata if available
            new_epoch: false, // Not necessarily a new epoch
            timestamp: 0,     // TODO: Extract from snapshot metadata if available
            era: Era::Conway, // TODO: Determine from snapshot or config
        })
    }

    /// Build GenesisValues from snapshot data
    fn build_genesis_values(&self) -> Result<GenesisValues> {
        // TODO: These values should ideally come from the snapshot or configuration
        // For now, using defaults for Conway era
        Ok(GenesisValues {
            byron_timestamp: 1506203091, // Byron mainnet genesis timestamp
            shelley_epoch: 208,          // Shelley started at epoch 208 on mainnet
            shelley_epoch_len: 432000,   // 5 days in seconds
            shelley_genesis_hash: [
                // Shelley mainnet genesis hash (placeholder - should be from config)
                0x1a, 0x3d, 0x98, 0x7a, 0x95, 0xad, 0xd2, 0x3e, 0x4f, 0x4d, 0x2d, 0x78, 0x74, 0x9f,
                0x96, 0x65, 0xd4, 0x1e, 0x48, 0x3e, 0xf2, 0xa2, 0x22, 0x9c, 0x4b, 0x0b, 0xf3, 0x9f,
                0xad, 0x7d, 0x5e, 0x27,
            ],
        })
    }

    async fn publish_start(&self) -> Result<()> {
        self.context
            .message_bus
            .publish(
                &self.snapshot_topic,
                Arc::new(Message::Snapshot(
                    acropolis_common::messages::SnapshotMessage::Startup,
                )),
            )
            .await
            .map_err(|e| anyhow::anyhow!("Failed to publish start message: {e}"))
    }

    async fn publish_completion(
        &self,
        block_info: BlockInfo,
        genesis_values: GenesisValues,
    ) -> Result<()> {
        let message = Message::Cardano((
            block_info,
            CardanoMessage::GenesisComplete(GenesisCompleteMessage {
                values: genesis_values,
            }),
        ));

        self.context
            .message_bus
            .publish(&self.snapshot_topic, Arc::new(message))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to publish completion: {e}"))
    }
}

impl UtxoCallback for SnapshotHandler {
    fn on_utxo(&mut self, _utxo: UtxoEntry) -> Result<()> {
        self.utxo_count += 1;

        // Log progress every million UTXOs
        if self.utxo_count.is_multiple_of(1_000_000) {
            info!("Processed {} UTXOs", self.utxo_count);
        }
        // TODO: Accumulate UTXO data if needed or send in chunks to UTXOState processor
        Ok(())
    }
}

impl PoolCallback for SnapshotHandler {
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()> {
        info!("Received {} pools", pools.len());
        self.pools.extend(pools);
        // TODO: Publish pool data.
        Ok(())
    }
}

impl StakeCallback for SnapshotHandler {
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()> {
        info!("Received {} accounts", accounts.len());
        self.accounts.extend(accounts);
        // TODO: Publish account data.
        Ok(())
    }
}

impl DRepCallback for SnapshotHandler {
    fn on_dreps(&mut self, dreps: Vec<DRepInfo>) -> Result<()> {
        info!("Received {} DReps", dreps.len());
        self.dreps.extend(dreps);
        // TODO: Publish DRep data.

        Ok(())
    }
}

impl ProposalCallback for SnapshotHandler {
    fn on_proposals(&mut self, proposals: Vec<GovernanceProposal>) -> Result<()> {
        info!("Received {} proposals", proposals.len());
        self.proposals.extend(proposals);
        // TODO: Publish proposal data.
        Ok(())
    }
}

impl SnapshotCallbacks for SnapshotHandler {
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()> {
        info!("Received snapshot metadata for epoch {}", metadata.epoch);
        info!("  - UTXOs: {:?}", metadata.utxo_count);
        info!(
            "  - Pot balances: treasury={}, reserves={}, deposits={}",
            metadata.pot_balances.treasury,
            metadata.pot_balances.reserves,
            metadata.pot_balances.deposits
        );
        info!(
            "  - Previous epoch blocks: {}",
            metadata.blocks_previous_epoch.len()
        );
        info!(
            "  - Current epoch blocks: {}",
            metadata.blocks_current_epoch.len()
        );

        self.metadata = Some(metadata);
        Ok(())
    }

    fn on_complete(&mut self) -> Result<()> {
        info!("Snapshot parsing completed");
        info!("Final statistics:");
        info!("  - UTXOs processed: {}", self.utxo_count);
        info!("  - Pools: {}", self.pools.len());
        info!("  - Accounts: {}", self.accounts.len());
        info!("  - DReps: {}", self.dreps.len());
        info!("  - Proposals: {}", self.proposals.len());

        // We could send a Resolver reference from here for large data, i.e. the UTXO set,
        // which could be a file reference. For a file reference, we'd extend the parser to
        // give us a callback value with the offset into the file; and we'd make the streaming
        // UTXO parser public and reusable, adding it to the resolver implementation.
        Ok(())
    }
}

impl SnapshotBootstrapper {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let file_path = config
            .get_string("snapshot-path")
            .inspect_err(|e| error!("failed to find snapshot-path config: {e}"))?;

        let startup_topic =
            config.get_string("startup-topic").unwrap_or(DEFAULT_STARTUP_TOPIC.to_string());

        let snapshot_topic =
            config.get_string("snapshot-topic").unwrap_or(DEFAULT_SNAPSHOT_TOPIC.to_string());
        info!("Publishing snapshots on '{snapshot_topic}'");

        let completion_topic =
            config.get_string("completion-topic").unwrap_or(DEFAULT_COMPLETION_TOPIC.to_string());
        info!("Completing with '{completion_topic}'");

        let mut subscription = context.subscribe(&startup_topic).await?;

        context.clone().run(async move {
            let Ok(_) = subscription.read().await else {
                return;
            };
            info!("Received startup message");

            let span = info_span!("snapshot_bootstrapper.handle");
            async {
                if let Err(e) =
                    Self::process_snapshot(&file_path, context.clone(), &completion_topic).await
                {
                    error!("Failed to process snapshot: {}", e);
                }
            }
            .instrument(span)
            .await;
        });

        Ok(())
    }

    async fn process_snapshot(
        file_path: &str,
        context: Arc<Context<Message>>,
        completion_topic: &str,
    ) -> Result<()> {
        let parser = StreamingSnapshotParser::new(file_path);
        let mut callbacks = SnapshotHandler::new(context.clone(), completion_topic.to_string());

        info!(
            "Starting snapshot parsing and publishing from: {}",
            file_path
        );
        let start = Instant::now();

        callbacks.publish_start().await?;

        // Parse the snapshot with our callback handler
        parser.parse(&mut callbacks)?;

        let duration = start.elapsed();
        info!(
            "✓ Parse and publish completed successfully in {:.2?}",
            duration
        );

        // Build the final state from accumulated data
        let block_info = callbacks.build_block_info()?;
        let genesis_values = callbacks.build_genesis_values()?;

        // Publish completion message to trigger next phase (e.g., Mithril)
        callbacks.publish_completion(block_info, genesis_values).await?;

        info!("Snapshot bootstrap completed successfully");
        Ok(())
    }
}
