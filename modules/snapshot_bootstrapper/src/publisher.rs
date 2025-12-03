use acropolis_common::{
    messages::{CardanoMessage, Message},
    snapshot::{
        streaming_snapshot::{
            DRepCallback, DRepInfo, GovernanceProposal, PoolCallback, PoolInfo, ProposalCallback,
            SnapshotCallbacks, SnapshotMetadata, StakeCallback, UtxoCallback, UtxoEntry,
        },
        RawSnapshotsContainer, SnapshotsCallback,
    },
    stake_addresses::AccountState,
    BlockInfo,
};
use anyhow::Result;
use caryatid_sdk::Context;
use std::sync::Arc;
use tracing::info;

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
        let message = Arc::new(Message::Snapshot(
            acropolis_common::messages::SnapshotMessage::Startup,
        ));
        self.context.publish(&self.snapshot_topic, message).await
    }

    pub async fn publish_completion(&self, block_info: BlockInfo) -> Result<()> {
        let message = Arc::new(Message::Cardano((
            block_info,
            CardanoMessage::SnapshotComplete,
        )));
        self.context.publish(&self.completion_topic, message).await
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
        // TODO: Accumulate proposal data if needed or send in chunks to ProposalState processor
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
