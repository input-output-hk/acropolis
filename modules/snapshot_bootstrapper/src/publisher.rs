use acropolis_common::{
    epoch_snapshot::SnapshotsContainer,
    genesis_values::GenesisValues,
    ledger_state::SPOState,
    messages::{
        AccountsBootstrapMessage, CardanoMessage, DRepBootstrapMessage, EpochBootstrapMessage,
        GovernanceBootstrapMessage, GovernanceProposalRoots, Message, SnapshotMessage,
        SnapshotStateMessage, UTxOPartialState,
    },
    params::EPOCH_LENGTH,
    protocol_params::{Nonces, PraosParams},
    snapshot::{
        protocol_parameters::ProtocolParameters,
        streaming_snapshot::GovernanceProtocolParametersCallback, utxo::UtxoEntry,
        AccountsCallback, DRepCallback, EpochCallback, GovernanceProposal, GovernanceStateCallback,
        PoolCallback, ProposalCallback, SnapshotCallbacks, SnapshotMetadata, SnapshotsCallback,
        UtxoCallback,
    },
    stake_addresses::AccountState,
    BlockInfo, DRepCredential, DRepRecord, EpochBootstrapData, UTXOValue, UTxOIdentifier,
};
use anyhow::Result;
use caryatid_sdk::Context;
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

const UTXO_BATCH_SIZE: usize = 10_000;

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
    utxo_batch: Vec<(UTxOIdentifier, UTXOValue)>,
    utxo_batches_published: u64,
    pools: SPOState,
    accounts: Vec<AccountState>,
    dreps_len: usize,
    proposals: Vec<GovernanceProposal>,
    epoch_context: EpochContext,
    snapshot_fee: u64,
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
            utxo_batch: Vec::with_capacity(UTXO_BATCH_SIZE),
            utxo_batches_published: 0,
            pools: SPOState::new(),
            accounts: Vec::new(),
            dreps_len: 0,
            proposals: Vec::new(),
            epoch_context,
            snapshot_fee: 0,
        }
    }

    pub async fn publish_start(&self) -> Result<()> {
        let message = Arc::new(Message::Snapshot(SnapshotMessage::Startup));
        self.context.publish(&self.snapshot_topic, message).await.unwrap_or_else(|e| {
            tracing::error!("Failed to publish bootstrap startup message: {}", e);
        });
        Ok(())
    }

    pub async fn publish_snapshot_complete(&self) -> Result<()> {
        info!("Publishing Snapshot Complete on '{}'", self.snapshot_topic);
        let message = Arc::new(Message::Snapshot(SnapshotMessage::Complete));
        self.context.publish(&self.snapshot_topic, message).await.unwrap_or_else(|e| {
            tracing::error!("Failed to publish snapshot complete message: {}", e);
        });
        Ok(())
    }

    pub async fn publish_completion(&self, block_info: BlockInfo) -> Result<()> {
        info!(
            "Publishing SnapshotComplete on '{}' for block {} slot {} epoch {}",
            self.completion_topic, block_info.number, block_info.slot, block_info.epoch
        );
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
            total_fees: self.snapshot_fee,
            spo_blocks: data.spo_blocks_current.clone(),
            nonces: ctx.nonces.clone(),
            praos_params: Some(PraosParams::mainnet()),
        }
    }

    fn complete_batchers(&mut self) {
        if !self.utxo_batch.is_empty() {
            self.publish_utxo_batch();
        }
    }

    fn publish_utxo_batch(&mut self) {
        let batch_size = self.utxo_batch.len();
        self.utxo_batches_published += 1;

        if self.utxo_batches_published == 1 {
            info!(
                "Publishing first UTXO batch with {} UTXOs to topic '{}'",
                batch_size, self.snapshot_topic
            );
        } else if self.utxo_batches_published.is_multiple_of(100) {
            info!(
                "Published {} UTXO batches ({} UTXOs total)",
                self.utxo_batches_published, self.utxo_count
            );
        }

        let message = Arc::new(Message::Snapshot(SnapshotMessage::Bootstrap(
            SnapshotStateMessage::UTxOPartialState(UTxOPartialState {
                utxos: self.utxo_batch.clone(),
            }),
        )));

        // Clone what we need for the async task
        let context = self.context.clone();
        let snapshot_topic = self.snapshot_topic.clone();

        // Block on async publish since this callback is synchronous
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                if let Err(e) = context.publish(&snapshot_topic, message).await {
                    tracing::error!("Failed to publish UTXO batch: {}", e);
                }
            })
        });
        self.utxo_batch.clear();
    }
}

impl UtxoCallback for SnapshotPublisher {
    fn on_utxo(&mut self, utxo: UtxoEntry) -> Result<()> {
        self.utxo_count += 1;

        // Log progress every million UTXOs
        if self.utxo_count.is_multiple_of(1_000_000) {
            info!("Processed {} UTXOs", self.utxo_count);
        }

        self.utxo_batch.push((utxo.id, utxo.value));
        if self.utxo_batch.len() >= UTXO_BATCH_SIZE {
            self.publish_utxo_batch();
        }
        Ok(())
    }
}

impl PoolCallback for SnapshotPublisher {
    fn on_pools(&mut self, pools: SPOState) -> Result<()> {
        info!(
            "Received pools (current: {}, future: {}, retiring: {})",
            pools.pools.len(),
            pools.updates.len(),
            pools.retiring.len()
        );
        self.pools.extend(&pools);

        let message = Arc::new(Message::Snapshot(SnapshotMessage::Bootstrap(
            SnapshotStateMessage::SPOState(pools),
        )));

        let context = self.context.clone();
        let snapshot_topic = self.snapshot_topic.clone();

        // IMPORTANT: We use block_in_place + block_on to ensure each publish completes
        // before the callback returns. This guarantees message ordering. See on_accounts() for details.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                context.publish(&snapshot_topic, message).await.unwrap_or_else(|e| {
                    tracing::error!("Failed to publish SPO bootstrap message: {}", e)
                });
            })
        });

        Ok(())
    }
}

impl AccountsCallback for SnapshotPublisher {
    fn on_accounts(
        &mut self,
        data: acropolis_common::snapshot::AccountsBootstrapData,
    ) -> Result<()> {
        info!(
            "Publishing accounts bootstrap for epoch {} with {} accounts, {} pools ({} retiring), {} dreps, snapshots: {}",
            data.epoch,
            data.accounts.len(),
            data.pools.len(),
            data.retiring_pools.len(),
            data.dreps.len(),
            !data.snapshots.mark.spos.is_empty(),
        );

        // Convert the parsed data to the message type
        let message = AccountsBootstrapMessage {
            epoch: data.epoch,
            accounts: data.accounts,
            pools: data.pools,
            retiring_pools: data.retiring_pools,
            dreps: data.dreps,
            pots: data.pots,
            bootstrap_snapshots: data.snapshots,
        };

        let msg = Arc::new(Message::Snapshot(SnapshotMessage::Bootstrap(
            SnapshotStateMessage::AccountsState(message),
        )));

        let context = self.context.clone();
        let snapshot_topic = self.snapshot_topic.clone();

        // IMPORTANT: Complete batching senders now before what is to come, to ensure all
        // batched data is flushed. See next, more detailed, comment
        self.complete_batchers();

        // IMPORTANT: We use block_in_place + block_on to ensure each publish completes
        // before the callback returns. This guarantees message ordering.
        //
        // The StreamingSnapshotParser::parse() call is synchronous - callbacks like
        // on_accounts() and on_epoch() are invoked during parsing. If we spawned async
        // tasks here (fire-and-forget), they would race against publish_completion()
        // which runs immediately after parse() returns:
        //
        //   parse() starts
        //   on_accounts() spawns publish task, returns immediately
        //   on_epoch() spawns publish task, returns immediately
        //   parse() returns
        //   publish_completion().await  <-- could complete before spawned tasks!
        //
        // Since state modules (accounts_state, epochs_state, spo_state, etc.) subscribe
        // to both cardano.snapshot and cardano.snapshot.complete, they could receive
        // the completion signal before bootstrap data arrives, causing initialization
        // failures.
        //
        // By blocking here, we ensure all bootstrap messages are published before
        // parse() returns, and thus before publish_completion() is called.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                context.publish(&snapshot_topic, msg).await.unwrap_or_else(|e| {
                    tracing::error!("Failed to publish accounts bootstrap message: {}", e)
                });
            })
        });

        Ok(())
    }
}

impl DRepCallback for SnapshotPublisher {
    fn on_dreps(&mut self, epoch: u64, dreps: HashMap<DRepCredential, DRepRecord>) -> Result<()> {
        info!("Received {} DReps for epoch {}", dreps.len(), epoch);
        self.dreps_len += dreps.len();
        // Send a message to the DRepState
        let message = Arc::new(Message::Snapshot(SnapshotMessage::Bootstrap(
            SnapshotStateMessage::DRepState(DRepBootstrapMessage { dreps, epoch }),
        )));

        // Clone what we need for the async task
        let context = self.context.clone();
        let snapshot_topic = self.snapshot_topic.clone();

        // See comment in AccountsCallback::on_accounts for why we block here.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                context.publish(&snapshot_topic, message).await.unwrap_or_else(|e| {
                    tracing::error!("Failed to publish DRepBootstrap message: {}", e)
                });
            })
        });
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

impl GovernanceProtocolParametersCallback for SnapshotPublisher {
    fn on_gs_protocol_parameters(
        &mut self,
        _gs_previous_params: ProtocolParameters,
        _gs_current_params: ProtocolParameters,
        _gs_future_params: ProtocolParameters,
    ) -> Result<()> {
        info!("Received governance protocol parameters (current, previous, future)");
        // TODO: Publish protocol parameters to appropriate message bus topics
        // This could involve publishing messages for:
        // - CurrentProtocolParameters → ParametersState processor
        // - PreviousProtocolParameters → ParametersState processor
        // - FutureProtocolParameters → ParametersState processor
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

        // See comment in AccountsCallback::on_accounts for why we block here.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                context.publish(&snapshot_topic, message).await.unwrap_or_else(|e| {
                    tracing::error!("Failed to publish epoch bootstrap message: {}", e)
                });
            })
        });

        Ok(())
    }
}

impl SnapshotsCallback for SnapshotPublisher {
    fn on_snapshots(&mut self, snapshots: SnapshotsContainer) -> Result<()> {
        // Calculate totals from processed snapshots
        let mark_delegators: usize =
            snapshots.mark.spos.values().map(|spo| spo.delegators.len()).sum();
        let mark_stake: u64 = snapshots.mark.spos.values().map(|spo| spo.total_stake).sum();

        let set_delegators: usize =
            snapshots.set.spos.values().map(|spo| spo.delegators.len()).sum();
        let set_stake: u64 = snapshots.set.spos.values().map(|spo| spo.total_stake).sum();

        let go_delegators: usize = snapshots.go.spos.values().map(|spo| spo.delegators.len()).sum();
        let go_stake: u64 = snapshots.go.spos.values().map(|spo| spo.total_stake).sum();

        info!("Snapshots Data:");
        info!(
            "  Mark snapshot (epoch {}): {} SPOs, {} delegators, {} ADA",
            snapshots.mark.epoch,
            snapshots.mark.spos.len(),
            mark_delegators,
            mark_stake / 1_000_000
        );
        info!(
            "  Set snapshot (epoch {}): {} SPOs, {} delegators, {} ADA",
            snapshots.set.epoch,
            snapshots.set.spos.len(),
            set_delegators,
            set_stake / 1_000_000
        );
        info!(
            "  Go snapshot (epoch {}): {} SPOs, {} delegators, {} ADA",
            snapshots.go.epoch,
            snapshots.go.spos.len(),
            go_delegators,
            go_stake / 1_000_000
        );

        // Store the fee for use in epoch bootstrap message
        self.snapshot_fee = snapshots.fee;
        info!("  Snapshot fee: {} lovelace", self.snapshot_fee);

        Ok(())
    }
}

impl GovernanceStateCallback for SnapshotPublisher {
    fn on_governance_state(
        &mut self,
        state: acropolis_common::snapshot::GovernanceState,
    ) -> Result<()> {
        let epoch = state.epoch;

        info!(
            "Received governance state for epoch {}: {} proposals, {} vote records",
            epoch,
            state.proposals.len(),
            state.votes.len()
        );

        // Convert GovernanceState to ConwayVoting-compatible data
        let (proposals, votes) = state.to_conway_voting_data(epoch);

        // Convert proposal roots
        let proposal_roots = GovernanceProposalRoots {
            pparam_update: state.proposal_roots.pparam_update,
            hard_fork: state.proposal_roots.hard_fork,
            committee: state.proposal_roots.committee,
            constitution: state.proposal_roots.constitution,
        };

        // Extract enacted action IDs
        let enacted_action_ids: Vec<_> =
            state.enacted_actions.iter().map(|s| s.id.clone()).collect();

        // Build the bootstrap message
        let message = GovernanceBootstrapMessage {
            epoch,
            proposals,
            votes,
            committee: state.committee,
            constitution: state.constitution,
            proposal_roots,
            enacted_action_ids,
            expired_action_ids: state.expired_action_ids,
        };

        info!(
            "Publishing governance bootstrap: {} proposals, {} committee members, constitution: {}",
            message.proposals.len(),
            message.committee.as_ref().map(|c| c.members.len()).unwrap_or(0),
            message.constitution.anchor.url,
        );

        let msg = Arc::new(Message::Snapshot(SnapshotMessage::Bootstrap(
            SnapshotStateMessage::GovernanceState(message),
        )));

        let context = self.context.clone();
        let snapshot_topic = self.snapshot_topic.clone();

        // See comment in AccountsCallback::on_accounts for why we block here.
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                context.publish(&snapshot_topic, msg).await.unwrap_or_else(|e| {
                    tracing::error!("Failed to publish governance bootstrap message: {}", e)
                });
            })
        });

        Ok(())
    }
}

impl SnapshotCallbacks for SnapshotPublisher {
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()> {
        let total_blocks_previous: u32 =
            metadata.blocks_previous_epoch.iter().map(|p| p.block_count as u32).sum();
        let total_blocks_current: u32 =
            metadata.blocks_current_epoch.iter().map(|p| p.block_count as u32).sum();

        info!("Snapshot metadata for epoch {}", metadata.epoch);
        info!("  UTXOs: {:?}", metadata.utxo_count);
        info!(
            "  Pot balances: treasury={}, reserves={}, deposits={}",
            metadata.pot_balances.treasury,
            metadata.pot_balances.reserves,
            metadata.pot_balances.deposits
        );
        info!("  - Previous epoch blocks: {}", total_blocks_previous);
        info!("  - Current epoch blocks: {}", total_blocks_current);

        self.metadata = Some(metadata);
        Ok(())
    }

    fn on_complete(&mut self) -> Result<()> {
        info!("Snapshot parsing completed");
        info!("Final statistics:");
        info!("  - UTXOs processed: {}", self.utxo_count);
        info!(
            "  - Pools: {} (future: {}, retiring: {})",
            self.pools.pools.len(),
            self.pools.updates.len(),
            self.pools.retiring.len()
        );
        info!("  - Accounts: {}", self.accounts.len());
        info!("  - DReps: {}", self.dreps_len);
        info!("  - Proposals: {}", self.proposals.len());
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
            lab: Nonce::from([3u8; 32]),
            prev_lab: Nonce::from([4u8; 32]),
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
    fn test_epoch_context_stores_nonces() {
        // This would require mocking Context, so just test the data flow concept
        let nonces = make_test_nonces();
        let genesis = GenesisValues::mainnet();

        let ctx = EpochContext::new(nonces.clone(), 134956789, 11000000, 509, &genesis);

        // Verify nonce conversion works
        assert_eq!(ctx.nonces, nonces);
    }
}
