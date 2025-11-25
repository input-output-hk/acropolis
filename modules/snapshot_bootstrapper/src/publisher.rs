use acropolis_common::{
    messages::{
        CardanoMessage, EpochActivityMessage, Message, SnapshotMessage, SnapshotStateMessage,
    },
    snapshot::streaming_snapshot::{
        DRepCallback, DRepInfo, EpochBootstrapData, EpochCallback, GovernanceProposal,
        PoolCallback, PoolInfo, ProposalCallback, SnapshotCallbacks, SnapshotMetadata,
        StakeCallback, UtxoCallback, UtxoEntry,
    },
    stake_addresses::AccountState,
    BlockInfo, PoolId,
};
use anyhow::Result;
use caryatid_sdk::Context;
use std::sync::Arc;
use tracing::{info, warn};

/// Handles publishing snapshot data to the message bus
pub struct SnapshotPublisher {
    context: Arc<Context<Message>>,
    completion_topic: String,
    snapshot_topic: String,
    metadata: Option<SnapshotMetadata>,
    utxo_count: u64,
    pools: Vec<PoolInfo>,
    accounts: Vec<AccountState>,
    dreps: Vec<DRepInfo>,
    proposals: Vec<GovernanceProposal>,
}

impl SnapshotPublisher {
    pub fn new(
        context: Arc<Context<Message>>,
        completion_topic: String,
        snapshot_topic: String,
    ) -> Self {
        Self {
            context,
            completion_topic,
            snapshot_topic,
            metadata: None,
            utxo_count: 0,
            pools: Vec::new(),
            accounts: Vec::new(),
            dreps: Vec::new(),
            proposals: Vec::new(),
        }
    }

    pub async fn publish_start(&self) -> Result<()> {
        let message = Arc::new(Message::Snapshot(SnapshotMessage::Startup));
        self.context.publish(&self.snapshot_topic, message).await
    }

    pub async fn publish_completion(&self, block_info: BlockInfo) -> Result<()> {
        let message = Arc::new(Message::Cardano((
            block_info,
            CardanoMessage::SnapshotComplete,
        )));
        self.context.publish(&self.completion_topic, message).await
    }

    pub fn metadata(&self) -> Option<&SnapshotMetadata> {
        self.metadata.as_ref()
    }

    /// Convert hex pool ID string to PoolId
    fn parse_pool_id(pool_id_hex: &str) -> Option<PoolId> {
        match hex::decode(pool_id_hex) {
            Ok(bytes) if bytes.len() == 28 => {
                let mut arr = [0u8; 28];
                arr.copy_from_slice(&bytes);
                Some(PoolId::from(arr))
            }
            Ok(bytes) => {
                warn!(
                    "Invalid pool ID length: expected 28 bytes, got {} for {}",
                    bytes.len(),
                    pool_id_hex
                );
                None
            }
            Err(e) => {
                warn!("Failed to decode pool ID {}: {}", pool_id_hex, e);
                None
            }
        }
    }

    /// Build EpochActivityMessage from EpochBootstrapData
    fn build_epoch_activity_message(data: &EpochBootstrapData) -> EpochActivityMessage {
        let spo_blocks: Vec<(PoolId, usize)> = data
            .blocks_current_epoch
            .iter()
            .filter_map(|(pool_id_hex, count)| {
                Self::parse_pool_id(pool_id_hex).map(|pool_id| (pool_id, *count as usize))
            })
            .collect();

        EpochActivityMessage {
            epoch: data.epoch,
            epoch_start_time: 0,   // TODO: Calculate / enhance EpochBoostrapData
            epoch_end_time: 0,     // TODO: Calculate / enhance EpochBoostrapData
            first_block_time: 0,   // TODO: Calculate / enhance EpochBoostrapData
            first_block_height: 0, // TODO: Calculate / enhance EpochBoostrapData
            last_block_time: 0,    // TODO: Calculate / enhance EpochBoostrapData
            last_block_height: 0,  // TODO: Calculate / enhance EpochBoostrapData
            total_blocks: data.total_blocks_current as usize,
            total_txs: 0,     // TODO: Calculate / enhance EpochBoostrapData
            total_outputs: 0, // TODO: Calculate / enhance EpochBoostrapData
            total_fees: 0,    // TODO: Calculate / enhance EpochBoostrapData
            spo_blocks,
            nonce: None, // TODO: Calculate / enhance EpochBoostrapData
        }
    }
}

impl UtxoCallback for SnapshotPublisher {
    fn on_utxo(&mut self, _utxo: UtxoEntry) -> Result<()> {
        self.utxo_count += 1;

        // Log progress every million UTXOs
        if self.utxo_count % 1_000_000 == 0 {
            info!("Processed {} UTXOs", self.utxo_count);
        }
        Ok(())
    }
}

impl PoolCallback for SnapshotPublisher {
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()> {
        info!("Received {} pools", pools.len());
        self.pools.extend(pools);
        Ok(())
    }
}

impl StakeCallback for SnapshotPublisher {
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()> {
        info!("Received {} accounts", accounts.len());
        self.accounts.extend(accounts);
        Ok(())
    }
}

impl DRepCallback for SnapshotPublisher {
    fn on_dreps(&mut self, dreps: Vec<DRepInfo>) -> Result<()> {
        info!("Received {} DReps", dreps.len());
        self.dreps.extend(dreps);
        Ok(())
    }
}

impl ProposalCallback for SnapshotPublisher {
    fn on_proposals(&mut self, proposals: Vec<GovernanceProposal>) -> Result<()> {
        info!("Received {} proposals", proposals.len());
        self.proposals.extend(proposals);
        Ok(())
    }
}

impl EpochCallback for SnapshotPublisher {
    fn on_epoch(&mut self, data: EpochBootstrapData) -> Result<()> {
        info!(
            "Received epoch bootstrap data for epoch {}: {} current epoch blocks, {} previous epoch blocks",
            data.epoch,
            data.total_blocks_current,
            data.total_blocks_previous
        );

        let epoch_activity = Self::build_epoch_activity_message(&data);

        info!(
            "Publishing epoch bootstrap for epoch {} with {} SPO entries",
            data.epoch,
            epoch_activity.spo_blocks.len()
        );

        let message = Arc::new(Message::Snapshot(SnapshotMessage::Bootstrap(
            SnapshotStateMessage::EpochState(epoch_activity),
        )));

        // Clone what we need for the async task
        let context = self.context.clone();
        let snapshot_topic = self.snapshot_topic.clone();

        // Spawn async publish task since this callback is synchronous
        tokio::spawn(async move {
            if let Err(e) = context.publish(&snapshot_topic, message).await {
                tracing::error!("Failed to publish epoch bootstrap: {}", e);
            }
        });

        Ok(())
    }
}

impl SnapshotCallbacks for SnapshotPublisher {
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()> {
        info!("Snapshot metadata for epoch {}", metadata.epoch);
        info!("  UTXOs: {:?}", metadata.utxo_count);
        info!(
            "  Pot balances: treasury={}, reserves={}, deposits={}",
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
        Ok(())
    }
}
