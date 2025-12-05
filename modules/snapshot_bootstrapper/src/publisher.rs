use acropolis_common::protocol_params::{Nonces, PraosParams};
use acropolis_common::snapshot::{RawSnapshotsContainer, SnapshotsCallback};
use acropolis_common::{
    genesis_values::GenesisValues,
    messages::{
        AccountsBootstrapMessage, CardanoMessage, EpochBootstrapMessage, Message, PotBalances,
        SnapshotMessage, SnapshotStateMessage,
    },
    params::EPOCH_LENGTH,
    snapshot::streaming_snapshot::{
        DRepCallback, DRepInfo, EpochCallback, GovernanceProposal, PoolCallback, PoolInfo,
        ProposalCallback, SnapshotCallbacks, SnapshotMetadata, StakeCallback, UtxoCallback,
        UtxoEntry,
    },
    stake_addresses::AccountState,
    types::{PoolRegistration, Ratio, VrfKeyHash},
    BlockInfo, EpochBootstrapData, PoolId, StakeAddress,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::Context;
use std::str::FromStr;
use std::sync::Arc;
use tracing::{info, warn};

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

    /// This is a band-aid for now.
    ///
    /// Convert PoolInfo (API format with strings) to PoolRegistration (internal format)
    /// This does all the parsing and validation work so the state module can just use the data
    fn convert_pool_info_to_registration(pool_info: &PoolInfo) -> Result<PoolRegistration> {
        let pool_id = PoolId::from_str(&pool_info.pool_id)
            .map_err(|e| anyhow!("Failed to parse pool ID '{}': {}", pool_info.pool_id, e))?;

        // Parse VRF key hash from hex
        let vrf_bytes = hex::decode(&pool_info.vrf_key_hash)
            .map_err(|e| anyhow!("Failed to decode VRF key hash: {}", e))?;
        if vrf_bytes.len() != 32 {
            return Err(anyhow!(
                "VRF key hash must be 32 bytes, got {}",
                vrf_bytes.len()
            ));
        }
        let vrf_key_hash = VrfKeyHash::new(vrf_bytes.try_into().unwrap());

        // Parse reward account from hex-encoded binary (29 bytes)
        let reward_account = {
            let bytes = hex::decode(&pool_info.reward_account)
                .map_err(|e| anyhow!("Failed to decode reward account: {}", e))?;
            StakeAddress::from_binary(&bytes)
                .map_err(|e| anyhow!("Failed to parse reward account from binary: {}", e))?
        };

        // Parse pool owners from hex-encoded binary (29 bytes each)
        let mut pool_owners = Vec::new();
        for owner_str in &pool_info.pool_owners {
            let bytes = hex::decode(owner_str)
                .map_err(|e| anyhow!("Failed to decode pool owner: {}", e))?;
            let owner_addr = StakeAddress::from_binary(&bytes)
                .map_err(|e| anyhow!("Failed to parse pool owner from binary: {}", e))?;
            pool_owners.push(owner_addr);
        }

        // Convert margin from f64 to Ratio
        let margin = Ratio {
            numerator: (pool_info.margin * 1_000_000.0) as u64,
            denominator: 1_000_000,
        };

        Ok(PoolRegistration {
            operator: pool_id,
            vrf_key_hash,
            pledge: pool_info.pledge,
            cost: pool_info.cost,
            margin,
            reward_account,
            pool_owners,
            relays: Vec::new(),  // TODO: Read from relays in parser
            pool_metadata: None, // TODO: Read pool metadata
        })
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

        // Now that we have all the data, publish the complete accounts bootstrap message
        let metadata = self.metadata.as_ref();
        let epoch = metadata.map(|m| m.epoch).unwrap_or(0);

        let pots = metadata
            .map(|m| PotBalances {
                treasury: m.pot_balances.treasury,
                reserves: m.pot_balances.reserves,
                deposits: m.pot_balances.deposits,
            })
            .unwrap_or(PotBalances {
                treasury: 0,
                reserves: 0,
                deposits: 0,
            });

        let snapshots = metadata.and_then(|m| m.snapshots.clone());

        // Convert PoolInfo to PoolRegistration (internal format)
        let mut pools = Vec::new();
        let mut retiring_pools = Vec::new();
        for pool_info in &self.pools {
            match Self::convert_pool_info_to_registration(pool_info) {
                Ok(pool_reg) => {
                    if pool_info.retirement_epoch.is_some() {
                        retiring_pools.push(pool_reg.operator);
                    }
                    pools.push(pool_reg);
                }
                Err(e) => {
                    warn!("Failed to convert pool {}: {}", pool_info.pool_id, e);
                }
            }
        }

        // Convert DRepInfo to (DRepCredential, deposit) tuples
        let dreps = self
            .dreps
            .iter()
            .map(|drep_info| (drep_info.drep_id.clone(), drep_info.deposit))
            .collect::<Vec<_>>();

        let accounts_bootstrap_message = AccountsBootstrapMessage {
            epoch,
            accounts: self.accounts.clone(),
            pools,
            retiring_pools,
            dreps,
            pots,
            snapshots,
        };

        info!(
            "Publishing accounts bootstrap for epoch {} with {} accounts, {} pools ({} retiring), {} dreps",
            epoch,
            accounts_bootstrap_message.accounts.len(),
            accounts_bootstrap_message.pools.len(),
            accounts_bootstrap_message.retiring_pools.len(),
            accounts_bootstrap_message.dreps.len(),
        );

        let message = Arc::new(Message::Snapshot(SnapshotMessage::Bootstrap(
            SnapshotStateMessage::AccountsState(accounts_bootstrap_message),
        )));

        let context = self.context.clone();
        let snapshot_topic = self.snapshot_topic.clone();

        tokio::spawn(async move {
            context.publish(&snapshot_topic, message).await.unwrap_or_else(|e| {
                tracing::error!("Failed to publish accounts bootstrap message: {}", e)
            });
        });

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
