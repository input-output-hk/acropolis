use acropolis_common::protocol_params::{Nonces, PraosParams};
use acropolis_common::snapshot::{RawSnapshotsContainer, SnapshotsCallback};
use acropolis_common::{
    genesis_values::GenesisValues,
    messages::{
        CardanoMessage, EpochBootstrapMessage, Message, SnapshotMessage, SnapshotStateMessage,
    },
    params::EPOCH_LENGTH,
    snapshot::streaming_snapshot::{
        DRepCallback, DRepInfo, EpochBootstrapData, EpochCallback, GovernanceProposal,
        PoolCallback, PoolInfo, ProposalCallback, SnapshotCallbacks, SnapshotMetadata,
        StakeCallback, UtxoCallback, UtxoEntry,
    },
    stake_addresses::AccountState,
    BlockInfo,
};
use anyhow::Result;
use caryatid_sdk::Context;
use std::sync::Arc;
use tracing::info;

/// External epoch context containing nonces and timing information.
///
/// This data comes from bootstrap configuration files (nonces.json, headers/{slot}.{block_header_hash}.cbor, etc)
/// and is not available to the CBOR parser. It's injected into the publisher
/// so that `EpochBootstrapMessage` can include complete information, among other things.
#[derive(Debug, Clone)]
pub struct EpochContext {
    /// Nonces for the target epoch
    pub nonces: Nonces,
    /// Epoch start time (UNIX timestamp)
    pub epoch_start_time: u64,
    /// Epoch end time (UNIX timestamp)
    pub epoch_end_time: u64,
    /// Last block timestamp from header
    pub last_block_time: u64,
    /// Last block height from header
    pub last_block_height: u64,
}

impl EpochContext {
    /// Build context from nonces, header data, and genesis values.
    ///
    /// * `nonces` - Nonces loaded from nonces.json
    /// * `header_slot` - Slot number from the target block header
    /// * `header_block_height` - Block height from the target block header
    /// * `epoch` - Target epoch number
    /// * `genesis` - Genesis values for timestamp calculations
    pub fn new(
        nonces: Nonces,
        header_slot: u64,
        header_block_height: u64,
        epoch: u64,
        genesis: &GenesisValues,
    ) -> Self {
        let epoch_start_slot = genesis.epoch_to_first_slot(epoch);
        let epoch_start_time = genesis.slot_to_timestamp(epoch_start_slot);
        let epoch_end_time = epoch_start_time + EPOCH_LENGTH;
        let last_block_time = genesis.slot_to_timestamp(header_slot);

        Self {
            nonces,
            epoch_start_time,
            epoch_end_time,
            last_block_time,
            last_block_height: header_block_height,
        }
    }
}

/// Handles publishing snapshot data to the message bus.
///
/// Implements the sink traits that the streaming parser calls during parsing.
/// External context (nonces, timing) can be added via `with_bootstrap_context()`.
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
    epoch_context: EpochContext,
}

impl SnapshotPublisher {
    pub fn new(
        context: Arc<Context<Message>>,
        completion_topic: String,
        snapshot_topic: String,
        epoch_context: EpochContext,
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
            epoch_context,
        }
    }

    pub async fn publish_start(&self) -> Result<()> {
        let message = Arc::new(Message::Snapshot(SnapshotMessage::Startup));
        self.context.publish(&self.snapshot_topic, message).await.unwrap_or_else(|e| {
            tracing::error!("Failed to publish bootstrap startup message: {}", e);
        });
        Ok(())
    }

    pub async fn publish_completion(&self, block_info: BlockInfo) -> Result<()> {
        let message = Arc::new(Message::Cardano((
            block_info,
            CardanoMessage::SnapshotComplete,
        )));
        self.context.publish(&self.completion_topic, message).await.unwrap_or_else(|e| {
            tracing::error!("Failed to publish bootstrap completion message: {}", e);
        });
        Ok(())
    }

    fn build_epoch_bootstrap_message(&self, data: &EpochBootstrapData) -> EpochBootstrapMessage {
        let ctx = &self.epoch_context;
        let first_block_height = ctx.last_block_height.saturating_sub(data.total_blocks_current);

        EpochBootstrapMessage {
            epoch: data.epoch,
            epoch_start_time: ctx.epoch_start_time,
            epoch_end_time: ctx.epoch_end_time,
            first_block_time: ctx.epoch_start_time,
            first_block_height,
            last_block_time: ctx.last_block_time,
            last_block_height: ctx.last_block_height,
            total_blocks: data.total_blocks_current as usize,
            total_txs: 0,
            total_outputs: 0,
            total_fees: 0,
            spo_blocks: data.spo_blocks_current.clone(),
            nonces: ctx.nonces.clone(),
            praos_params: Some(PraosParams::mainnet()),
        }
    }
}

impl UtxoCallback for SnapshotPublisher {
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

impl PoolCallback for SnapshotPublisher {
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()> {
        info!("Received {} pools", pools.len());
        self.pools.extend(pools);
        // TODO: Accumulate pool data if needed or send in chunks to PoolState processor
        Ok(())
    }
}

impl StakeCallback for SnapshotPublisher {
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()> {
        info!("Received {} accounts", accounts.len());
        self.accounts.extend(accounts);
        // TODO: Accumulate account data if needed or send in chunks to AccountState processor
        Ok(())
    }
}

impl DRepCallback for SnapshotPublisher {
    fn on_dreps(&mut self, dreps: Vec<DRepInfo>) -> Result<()> {
        info!("Received {} DReps", dreps.len());
        self.dreps.extend(dreps);
        // TODO: Accumulate DRep data if needed or send in chunks to DRepState processor
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

        let epoch_bootstrap_data = self.build_epoch_bootstrap_message(&data);

        let spo_blocks = epoch_bootstrap_data.spo_blocks.clone();
        info!(
            "Publishing epoch bootstrap for epoch {} with {} SPO entries",
            data.epoch,
            spo_blocks.len(),
        );

        let message = Arc::new(Message::Snapshot(SnapshotMessage::Bootstrap(
            SnapshotStateMessage::EpochState(epoch_bootstrap_data),
        )));

        let context = self.context.clone();
        let snapshot_topic = self.snapshot_topic.clone();

        tokio::spawn(async move {
            context.publish(&snapshot_topic, message).await.unwrap_or_else(|e| {
                tracing::error!("Failed to publish epoch bootstrap message: {}", e)
            });
        });

        Ok(())
    }
}

impl SnapshotsCallback for SnapshotPublisher {
    fn on_snapshots(&mut self, snapshots: RawSnapshotsContainer) -> Result<()> {
        info!("📸 Raw Snapshots Data:");

        // Calculate total stakes and delegator counts from VMap data
        let mark_total: i64 = snapshots.mark.0.iter().map(|(_, amount)| amount).sum();
        let set_total: i64 = snapshots.set.0.iter().map(|(_, amount)| amount).sum();
        let go_total: i64 = snapshots.go.0.iter().map(|(_, amount)| amount).sum();

        info!(
            "  • Mark snapshot: {} delegators, {} total stake (ADA)",
            snapshots.mark.0.len(),
            mark_total as f64 / 1_000_000.0
        );
        info!(
            "  • Set snapshot: {} delegators, {} total stake (ADA)",
            snapshots.set.0.len(),
            set_total as f64 / 1_000_000.0
        );
        info!(
            "  • Go snapshot: {} delegators, {} total stake (ADA)",
            snapshots.go.0.len(),
            go_total as f64 / 1_000_000.0
        );
        info!("  • Fee: {} ADA", snapshots.fee as f64 / 1_000_000.0);

        // TODO: Send snapshot data to appropriate message bus topics
        // This could involve publishing messages for:
        // - Mark snapshot → MarkSnapshotState processor
        // - Set snapshot → SetSnapshotState processor
        // - Go snapshot → GoSnapshotState processor
        // - Fee data → FeesState processor

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
        // We could send a Resolver reference from here for large data, i.e. the UTXO set,
        // which could be a file reference. For a file reference, we'd extend the parser to
        // give us a callback value with the offset into the file; and we'd make the streaming
        // UTXO parser public and reusable, adding it to the resolver implementation.
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::protocol_params::Nonce;

    fn make_test_nonces() -> Nonces {
        Nonces {
            epoch: 509,
            active: Nonce::from([0u8; 32]),
            evolving: Nonce::from([1u8; 32]),
            candidate: Nonce::from([2u8; 32]),
            lab: Nonce::from([3u8; 32]),      // was: tail
            prev_lab: Nonce::from([4u8; 32]), // was: missing
        }
    }

    #[test]
    fn test_bootstrap_context_new() {
        let nonces = make_test_nonces();
        let genesis = GenesisValues::mainnet();

        let ctx = EpochContext::new(
            nonces.clone(),
            134956789, // slot
            11000000,  // block height
            509,       // epoch
            &genesis,
        );

        assert_eq!(ctx.nonces.epoch, 509);
        assert_eq!(ctx.last_block_height, 11000000);
        assert!(ctx.epoch_start_time > 0);
        assert!(ctx.epoch_end_time > ctx.epoch_start_time);
    }

    #[test]
    fn test_build_epoch_activity_with_context() {
        // This would require mocking Context, so just test the data flow concept
        let nonces = make_test_nonces();
        let genesis = GenesisValues::mainnet();

        let ctx = EpochContext::new(nonces.clone(), 134956789, 11000000, 509, &genesis);

        // Verify nonce conversion works
        assert_eq!(ctx.nonces, nonces);
    }
}
