//! Acropolis consensus module for Caryatid
//! Maintains a favoured chain based on offered options from multiple sources

pub mod consensus_tree;
pub mod tree_block;
pub mod tree_error;
pub mod tree_observer;

use acropolis_common::params::SECURITY_PARAMETER_K;
use acropolis_common::{
    configuration::{get_bool_flag, get_string_flag, get_u64_flag, BlockFlowMode},
    genesis_values::GenesisValues,
    messages::{
        BlockRejectedMessage, BlockWantedMessage, CardanoMessage, ConsensusMessage, Message,
        RawBlockMessage, StateTransitionMessage,
    },
    types::{BlockInfo, Point},
    validation::ValidationStatus,
    BlockHash, BlockIntent, BlockStatus, Era,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use consensus_tree::ConsensusTree;
use futures::future::try_join_all;
use pallas::ledger::traverse::MultiEraHeader;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::{sync::Mutex, time::timeout};
use tracing::{debug, error, info, info_span, warn, Instrument};
use tree_error::ConsensusTreeError;
use tree_observer::ConsensusTreeObserver;

const DEFAULT_BLOCKS_AVAILABLE_TOPIC: (&str, &str) =
    ("blocks-available-topic", "cardano.block.available");
const DEFAULT_BLOCKS_PROPOSED_TOPIC: (&str, &str) =
    ("blocks-proposed-topic", "cardano.block.proposed");
const DEFAULT_CONSENSUS_OFFERS_TOPIC: (&str, &str) =
    ("consensus-offers-topic", "cardano.consensus.offers");
const DEFAULT_CONSENSUS_WANTS_TOPIC: (&str, &str) =
    ("consensus-wants-topic", "cardano.consensus.wants");
const DEFAULT_FORCE_VALIDATION: (&str, bool) = ("force-validation", true);
const DEFAULT_VALIDATION_TIMEOUT: (&str, u64) = ("validation-timeout", 60); // seconds
const DEFAULT_GENESIS_COMPLETION_TOPIC: (&str, &str) =
    ("genesis-completion-topic", "cardano.sequence.bootstrapped");

/// Events emitted by the consensus tree observer, queued for async publishing.
enum ObserverEvent {
    BlockProposed { hash: BlockHash },
    Rollback { to_block_number: u64 },
    BlockRejected { hash: BlockHash },
}

/// Shared event queue between the observer and the main loop.
type EventQueue = Arc<std::sync::Mutex<Vec<ObserverEvent>>>;

/// Return type of a timed-out validator read batch: outer `Err` = timeout, inner `Err` = read failure.
type ValidationBatchResult =
    Result<anyhow::Result<Vec<(String, Arc<Message>)>>, tokio::time::error::Elapsed>;

/// Observer that queues tree events for later async publishing.
struct QueuingConsensusTreeObserver {
    events: EventQueue,
}

impl ConsensusTreeObserver for QueuingConsensusTreeObserver {
    fn block_proposed(&self, _number: u64, hash: BlockHash, _body: &[u8]) {
        self.events.lock().unwrap().push(ObserverEvent::BlockProposed { hash });
    }

    fn rollback(&self, to_block_number: u64) {
        self.events.lock().unwrap().push(ObserverEvent::Rollback { to_block_number });
    }

    fn block_rejected(&self, hash: BlockHash) {
        self.events.lock().unwrap().push(ObserverEvent::BlockRejected { hash });
    }
}

/// Bundled runtime state for the consensus loop.
struct ConsensusRuntime {
    context: Arc<Context<Message>>,
    blocks_proposed_topic: String,
    consensus_wants_topic: String,
    event_queue: EventQueue,
    /// The downstream modules are expecting two mechanisms for handling rollbacks:
    /// - CardanoMessage::StateTransition message
    /// - After a rollback is published, the first later proposed block must be marked as `RolledBack`
    ///
    /// `pending_post_rollback_marker` is addressing the second mechanism
    pending_post_rollback_marker: Option<u64>,
    tree: ConsensusTree,
    /// Cache of full block payloads for re-publication.
    ///
    /// The consensus tree intentionally stores only chain-selection metadata
    /// (hash/number/slot/parent/status/body) and is kept message-agnostic.
    /// `BlockInfo` carries Cardano-specific metadata (era/epoch flags, status,
    /// timestamps) and we *mutate intent* before validation, so keeping the
    /// original `Arc<Message>` would re-publish the wrong intent. This cache
    /// lets us reconstruct a correct `BlockAvailable` when `block_proposed`
    /// fires without coupling the tree to message schemas.
    /// This might be a subject for optimization should the size of the cache
    /// turns out to be unacceptable.
    block_data: HashMap<BlockHash, (BlockInfo, RawBlockMessage)>,
    validator_topics: Vec<String>,
    validator_subscriptions: Vec<Box<dyn Subscription<Message>>>,
    validation_timeout: Duration,
    do_validation: bool,
    stats: ConsensusStats,
}

/// Periodic logging counters.
struct ConsensusStats {
    offered: u64,
    wanted: u64,
    available: u64,
    validated: u64,
    proposed: u64,
    rollbacks: u64,
    rejected: u64,
    parent_missing: u64,
    last_logged_at: std::time::Instant,
}

impl Default for ConsensusStats {
    fn default() -> Self {
        Self {
            offered: 0,
            wanted: 0,
            available: 0,
            validated: 0,
            proposed: 0,
            rollbacks: 0,
            rejected: 0,
            parent_missing: 0,
            last_logged_at: std::time::Instant::now(),
        }
    }
}

impl ConsensusStats {
    fn maybe_log(&mut self) {
        if self.last_logged_at.elapsed() >= Duration::from_secs(60) {
            info!(
                "Consensus stats: offered={}, wanted={}, available={}, validated={}, proposed={}, rollbacks={}, rejected={}, parent_missing={}",
                self.offered, self.wanted, self.available, self.validated, self.proposed, self.rollbacks, self.rejected, self.parent_missing
            );
            self.last_logged_at = std::time::Instant::now();
        }
    }
}

/// Consensus module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "consensus",
    description = "Consensus algorithm"
)]
pub struct Consensus;

impl Consensus {
    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let blocks_available_topic = get_string_flag(&config, DEFAULT_BLOCKS_AVAILABLE_TOPIC);
        info!("Subscribing to blocks on '{blocks_available_topic}'");

        let blocks_proposed_topic = get_string_flag(&config, DEFAULT_BLOCKS_PROPOSED_TOPIC);
        info!("Publishing proposed blocks on '{blocks_proposed_topic}'");

        let consensus_offers_topic = get_string_flag(&config, DEFAULT_CONSENSUS_OFFERS_TOPIC);
        info!("Subscribing to consensus offers on '{consensus_offers_topic}'");

        let consensus_wants_topic = get_string_flag(&config, DEFAULT_CONSENSUS_WANTS_TOPIC);

        let validator_topics: Vec<String> =
            config.get::<Vec<String>>("validators").unwrap_or_default();
        for topic in &validator_topics {
            info!("Validator: {topic}");
        }

        let flow_mode = BlockFlowMode::from_config(&config);
        info!("Consensus flow mode: {flow_mode}");

        let validation_timeout =
            Duration::from_secs(get_u64_flag(&config, DEFAULT_VALIDATION_TIMEOUT));
        info!("Validation timeout {validation_timeout:?}");

        let force_validation = get_bool_flag(&config, DEFAULT_FORCE_VALIDATION);
        info!("Force validation and chain selection: {force_validation}");

        let genesis_completion_topic = get_string_flag(&config, DEFAULT_GENESIS_COMPLETION_TOPIC);
        info!("Subscribing to genesis completion on '{genesis_completion_topic}'");

        // Subscribe for incoming blocks (BlockAvailable)
        let block_subscription = context.subscribe(&blocks_available_topic).await?;

        // Subscribe for consensus messages (BlockOffered, BlockRescinded)
        // TODO: Temporary until consensus flow fully works.
        let consensus_subscription = if flow_mode == BlockFlowMode::Consensus {
            Some(context.subscribe(&consensus_offers_topic).await?)
        } else {
            None
        };

        // Subscribe to genesis completion for initial security parameter
        let mut genesis_subscription = context.subscribe(&genesis_completion_topic).await?;

        // Subscribe all the validators
        let validator_subscriptions: Vec<_> =
            try_join_all(validator_topics.iter().map(|topic| context.subscribe(topic))).await?;

        let do_validation = !validator_subscriptions.is_empty();

        let run_context = context.clone();
        context.run(async move {
            let genesis_k = Self::wait_genesis_values(&mut genesis_subscription)
                .await
                .map(|gv| {
                    info!(
                        k = gv.security_param,
                        "Security parameter k set from genesis values"
                    );
                    gv.security_param
                })
                .unwrap_or_else(|e| {
                    warn!(
                        "Failed to receive genesis values, using default k={}: {e:#}",
                        SECURITY_PARAMETER_K
                    );
                    SECURITY_PARAMETER_K
                });

            let event_queue: EventQueue = Arc::new(std::sync::Mutex::new(Vec::new()));
            let observer = Box::new(QueuingConsensusTreeObserver {
                events: event_queue.clone(),
            });
            let tree = ConsensusTree::new(genesis_k, observer);

            let mut runtime = ConsensusRuntime {
                context: run_context,
                blocks_proposed_topic,
                consensus_wants_topic,
                event_queue,
                pending_post_rollback_marker: None,
                tree,
                block_data: HashMap::new(),
                validator_topics,
                validator_subscriptions,
                validation_timeout,
                do_validation,
                stats: ConsensusStats::default(),
            };
            // TODO: Temporary until consensus flow fully works.
            match flow_mode {
                BlockFlowMode::Direct => {
                    runtime.run_direct(block_subscription, force_validation).await;
                }
                BlockFlowMode::Consensus => {
                    let consensus_subscription = consensus_subscription
                        .expect("consensus subscription missing for consensus flow mode");
                    runtime
                        .run_consensus(block_subscription, consensus_subscription, force_validation)
                        .await;
                }
            }
        });

        Ok(())
    }

    async fn wait_genesis_values(
        subscription: &mut Box<dyn Subscription<Message>>,
    ) -> Result<GenesisValues> {
        let (_, message) = subscription.read().await?;
        match message.as_ref() {
            Message::Cardano((_, CardanoMessage::GenesisComplete(complete))) => {
                Ok(complete.values.clone())
            }
            msg => anyhow::bail!("Unexpected message on genesis completion topic: {msg:?}"),
        }
    }
}

impl ConsensusRuntime {
    /// Main select loop for consensus flow: dispatches incoming messages to handler functions.
    async fn run_consensus(
        &mut self,
        mut block_subscription: Box<dyn Subscription<Message>>,
        mut consensus_subscription: Box<dyn Subscription<Message>>,
        force_validation: bool,
    ) {
        // If force_validation is disabled, treat immutable Mithril replay blocks
        // as pass-through. Once volatile blocks begin, resume normal consensus flow.
        let mut mithril_passthrough_active = !force_validation;
        loop {
            tokio::select! {
                result = block_subscription.read() => {
                    let Ok((_, message)) = result else {
                        error!("Block message read failed");
                        return;
                    };

                    match message.as_ref() {
                        Message::Cardano((raw_blk_info, CardanoMessage::BlockAvailable(raw_block))) => {
                            let block_info = if !mithril_passthrough_active && self.do_validation {
                                raw_blk_info.with_intent(BlockIntent::ValidateAndApply)
                            } else {
                                raw_blk_info.clone()
                            };

                            if mithril_passthrough_active && block_info.status == BlockStatus::Immutable {
                                let span = info_span!("consensus", block = block_info.number);
                                self.handle_block_available_direct(block_info, raw_block.clone())
                                    .instrument(span)
                                    .await;
                            } else {
                                if mithril_passthrough_active {
                                    info!(
                                        block = block_info.number,
                                        slot = block_info.slot,
                                        "Mithril pass-through complete; switching to consensus flow"
                                    );
                                    mithril_passthrough_active = false;
                                }
                                let span = info_span!("consensus", block = block_info.number);
                                self.handle_block_available_consensus(block_info, raw_block.clone())
                                    .instrument(span)
                                    .await;
                            }

                            self.stats.maybe_log();
                        }

                        _ => debug!("Ignoring non-BlockAvailable message on blocks topic"),
                    }
                }

                result = consensus_subscription.read() => {
                    let Ok((_, message)) = result else {
                        error!("Consensus message read failed");
                        return;
                    };

                    match message.as_ref() {
                        Message::Consensus(ConsensusMessage::BlockOffered(offered)) => {
                            let span = info_span!("consensus-offered", hash = %offered.hash);
                            self.handle_block_offered(offered.hash, offered.parent_hash, offered.number, offered.slot)
                                .instrument(span)
                                .await;
                        }

                        Message::Consensus(ConsensusMessage::BlockRescinded(rescinded)) => {
                            let span = info_span!("consensus-rescinded", hash = %rescinded.hash);
                            self.handle_block_rescinded(rescinded.hash)
                                .instrument(span)
                                .await;
                        }

                        _ => debug!("Ignoring unknown consensus message"),
                    }

                    self.stats.maybe_log();
                }
            }
        }
    }

    /// Consensus flow: require BlockOffered before BlockAvailable (except Mithril bootstrap).
    async fn handle_block_available_consensus(
        &mut self,
        block_info: BlockInfo,
        raw_block: RawBlockMessage,
    ) {
        // Fast path: if the block was previously offered, the tree already knows its
        // parent hash. We can skip the expensive CBOR header parse and use the
        // trusted tree metadata. Downstream validators (KES, VRF) still verify the
        // header cryptographically, so this is safe.
        if let Some(existing) = self.tree.get_block(&block_info.hash) {
            if existing.number != block_info.number {
                warn!(
                    block = block_info.number,
                    hash = %block_info.hash,
                    tree_number = existing.number,
                    "BlockAvailable number conflicts with tree — dropping"
                );
                return;
            }

            self.block_data.insert(block_info.hash, (block_info.clone(), raw_block.clone()));

            let had_body = existing.body.is_some();
            if let Err(e) = self.tree.add_block(block_info.hash, raw_block.body.clone()) {
                error!("Failed to add block body: {e}");
            }

            let proposed = self.drain_and_publish_events().await;
            self.stats.available += 1;

            if !had_body {
                self.validate_proposed_blocks(&proposed).await;
            }

            if let Err(e) = self.tree.prune() {
                error!("Prune failed: {e}");
            }
            self.prune_block_data();
            return;
        }

        // Slow path: block not in tree (Mithril bootstrap or unexpected).
        // Parse the CBOR header to discover the parent hash.
        let parent_hash = match Self::extract_parent_hash(block_info.era, &raw_block.header) {
            Ok(Some(h)) => h,
            Ok(None) => {
                debug!(
                    "Block {} has no parent hash (genesis/EB)",
                    block_info.number
                );
                if block_info.status == BlockStatus::Immutable {
                    let genesis_root = BlockHash::default();
                    debug!(
                        "Adding Immutable genesis block {} from Mithril bootstrap",
                        block_info.number
                    );
                    if let Err(e) =
                        self.tree.set_root(genesis_root, block_info.number.wrapping_sub(1), 0)
                    {
                        error!("Failed to set root for Immutable genesis: {e}");
                        return;
                    }
                    if let Err(e) = self.tree.check_block_wanted(
                        block_info.hash,
                        genesis_root,
                        block_info.number,
                        block_info.slot,
                    ) {
                        error!("Failed to add Immutable genesis block: {e}");
                        return;
                    }
                    self.stats.offered += 1;
                } else {
                    error!(
                        "BlockAvailable for unknown block {} (not offered) — dropping",
                        block_info.hash
                    );
                    return;
                }

                self.block_data.insert(block_info.hash, (block_info.clone(), raw_block.clone()));

                if let Err(e) = self.tree.add_block(block_info.hash, raw_block.body.clone()) {
                    error!("Failed to add genesis block body: {e}");
                }
                self.stats.available += 1;
                let proposed = self.drain_and_publish_events().await;
                self.validate_proposed_blocks(&proposed).await;

                return;
            }
            Err(e) => {
                error!("Failed to parse block header: {e}");
                return;
            }
        };

        if block_info.status == BlockStatus::Immutable {
            return self.handle_immutable_bootstrap(block_info, raw_block, parent_hash).await;
        }

        error!(
            block = block_info.number,
            hash = %block_info.hash,
            "BlockAvailable for unknown block (not offered, not immutable) — dropping"
        );
    }

    /// Handle BlockAvailable for Immutable blocks from Mithril bootstrap (no prior BlockOffered).
    async fn handle_immutable_bootstrap(
        &mut self,
        block_info: BlockInfo,
        raw_block: RawBlockMessage,
        parent_hash: BlockHash,
    ) {
        if self.tree.is_empty() {
            // Same rule as offered flow: for block 0 we need parent_number = u64::MAX,
            // so use wrapping_sub(1) instead of saturating_sub(1).
            let parent_number = block_info.number.wrapping_sub(1);
            if let Err(e) = self.tree.set_root(parent_hash, parent_number, 0) {
                error!("Failed to set root for Mithril bootstrap: {e}");
                return;
            }
            debug!(
                "Tree root set to parent {parent_hash} (block {}) for Mithril bootstrap",
                parent_number
            );
        }

        let wanted = match self.tree.check_block_wanted(
            block_info.hash,
            parent_hash,
            block_info.number,
            block_info.slot,
        ) {
            Ok(w) => w,
            Err(e) => {
                warn!(
                    "Immutable bootstrap block {} rejected: {e}",
                    block_info.number
                );
                return;
            }
        };

        self.stats.offered += 1;
        self.stats.wanted += wanted.len() as u64;
        self.block_data.insert(block_info.hash, (block_info.clone(), raw_block.clone()));

        if let Err(e) = self.tree.add_block(block_info.hash, raw_block.body.clone()) {
            error!("Failed to add Immutable block body: {e}");
        }

        let proposed = self.drain_and_publish_events().await;
        self.stats.available += 1;

        self.validate_proposed_blocks(&proposed).await;

        self.prune_block_data();
        if let Err(e) = self.tree.prune() {
            error!("Prune failed: {e}");
        }
        self.prune_block_data();
    }

    /// Handle a BlockOffered message: check_block_wanted, publish wanted hashes.
    async fn handle_block_offered(
        &mut self,
        hash: BlockHash,
        parent_hash: BlockHash,
        number: u64,
        slot: u64,
    ) {
        // Bootstrap the tree with a virtual root when the first offer arrives.
        // For block 0 (genesis/first block), wrapping_sub(1) yields u64::MAX, so
        // 0 == parent_number + 1 remains true under wrapping arithmetic.
        if self.tree.is_empty() {
            let parent_number = number.wrapping_sub(1);

            if let Err(e) = self.tree.set_root(parent_hash, parent_number, 0) {
                error!("Failed to set tree root from offered block: {e}");
                return;
            }
            debug!("Tree root set to parent {parent_hash} (block {parent_number})");
        }

        if self.tree.get_block(&parent_hash).is_none() {
            self.stats.parent_missing += 1;
            debug!(
                block = number,
                %hash,
                %parent_hash,
                "Parent not in tree for offered block — ignoring"
            );
            return;
        }

        let wanted = match self.tree.check_block_wanted(hash, parent_hash, number, slot) {
            Ok(w) => w,
            Err(e) => {
                warn!(block = number, %hash, "Offered block rejected: {e}");
                return;
            }
        };

        self.stats.offered += 1;
        self.stats.wanted += wanted.len() as u64;

        // Collect and publish observer events
        let _ = self.drain_and_publish_events().await;

        let wanted_msgs = self.build_block_wanted_messages(&wanted);
        self.publish_block_wanted_messages(&wanted_msgs).await;
    }

    /// Handle a BlockRescinded message: remove block, publish events.
    async fn handle_block_rescinded(&mut self, hash: BlockHash) {
        match self.tree.remove_block(hash) {
            Ok(newly_wanted) => {
                let _ = self.drain_and_publish_events().await;

                let wanted_msgs = self.build_block_wanted_messages(&newly_wanted);
                self.publish_block_wanted_messages(&wanted_msgs).await;
                self.prune_block_data();
            }
            Err(ConsensusTreeError::BlockNotInTree { .. }) => {
                // Rescinds can arrive for stale/off-fork hashes already removed by an
                // earlier rollback/prune. Treat as idempotent.
                debug!("Ignoring rescinded block not in tree: {hash}");
            }
            Err(e) => {
                warn!("Failed to remove rescinded block {hash}: {e}");
            }
        }
    }

    /// Collect observer events, resolve to messages, and publish.
    ///
    /// Returns hashes for blocks that were proposed in this batch.
    async fn drain_and_publish_events(&mut self) -> Vec<BlockHash> {
        let raw_events: Vec<ObserverEvent> = self.event_queue.lock().unwrap().drain(..).collect();
        let mut proposed = Vec::new();
        for event in &raw_events {
            match event {
                ObserverEvent::BlockProposed { hash } => {
                    self.stats.proposed += 1;
                    proposed.push(*hash);
                }
                ObserverEvent::Rollback { .. } => self.stats.rollbacks += 1,
                ObserverEvent::BlockRejected { .. } => self.stats.rejected += 1,
            }
        }
        let events = self.resolve_observer_events(raw_events);

        // Publish all events
        for (topic, msg) in events {
            self.context
                .message_bus
                .publish(&topic, msg)
                .await
                .unwrap_or_else(|e| error!("Failed to publish to {topic}: {e}"));
        }
        proposed
    }

    /// Publish `BlockWanted` messages for each hash.
    async fn publish_block_wanted_messages(&mut self, wanted: &[BlockWantedMessage]) {
        for wanted_block in wanted {
            let msg = Arc::new(Message::Consensus(ConsensusMessage::BlockWanted(
                wanted_block.clone(),
            )));
            self.context
                .message_bus
                .publish(&self.consensus_wants_topic, msg)
                .await
                .unwrap_or_else(|e| error!("Failed to publish BlockWanted: {e}"));
        }
    }

    /// Keep only metadata for blocks still present in the tree.
    fn prune_block_data(&mut self) {
        self.block_data.retain(|hash, _| self.tree.get_block(hash).is_some());
    }

    /// Consume validation responses for each block proposed in this batch.
    ///
    /// A single tree update can propose multiple contiguous blocks. We must
    /// consume one validator response set per proposed block to keep
    /// subscriptions aligned.
    async fn validate_proposed_blocks(&mut self, proposed: &[BlockHash]) {
        if !self.do_validation || proposed.is_empty() {
            return;
        }

        debug!(
            count = proposed.len(),
            "Validating batch of proposed blocks"
        );

        for &hash in proposed {
            let Some((block_info, _)) = self.block_data.get(&hash) else {
                warn!("No block data for proposed block {hash}, skipping validation");
                continue;
            };
            let block_info = block_info.clone();

            if !block_info.intent.do_validation() {
                debug!(block = block_info.number, "Skipping validation (intent)");
                continue;
            }

            self.handle_validation(&block_info).await;
        }
    }

    /// Collect validation responses and update the tree accordingly.
    async fn handle_validation(&mut self, block_info: &BlockInfo) {
        let completed_tasks = Arc::new(Mutex::new(
            HashMap::<String, Option<Arc<Message>>>::from_iter(
                self.validator_topics.iter().map(|s| (s.clone(), None)),
            ),
        ));

        let all_say_go = Self::all_validators_say_go(
            block_info.number,
            timeout(self.validation_timeout, async {
                let mut results = Vec::new();
                for sub in self.validator_subscriptions.iter_mut() {
                    let (topic, res) = sub.read().await?;
                    completed_tasks.lock().await.insert(topic.clone(), Some(res.clone()));
                    results.push((topic, res));
                }
                Ok::<Vec<(String, Arc<Message>)>, anyhow::Error>(results)
            })
            .await,
        );

        if all_say_go {
            self.stats.validated += 1;

            if let Err(e) = self.tree.mark_validated(block_info.hash) {
                error!("Failed to mark block validated: {e}");
            }

            // There is nothing more to be done here. Since the blocks on the favoured chain
            // are 'proposed', i.e., already sent to listening downstream modules, it is up to
            // the downstream modules to handle rollbacks, should that happen on 'NoGo' below.
        } else {
            error!(
                block = block_info.number,
                "Validation rejected block, results available:"
            );
            let completed = completed_tasks.lock().await;
            for (topic, msg) in completed.iter() {
                error!("Topic {topic}, result {msg:?}");
            }
            drop(completed);

            // Mark rejected in tree — triggers observer events
            if let Err(e) = self.tree.mark_rejected(block_info.hash) {
                error!("Failed to mark block rejected: {e}");
            }

            // Collect and publish events from mark_rejected
            let _ = self.drain_and_publish_events().await;
            self.prune_block_data();
        }
    }

    /// Resolve pre-drained observer events into publishable messages.
    ///
    /// Sync function — does not hold tree references across await points.
    fn resolve_observer_events(
        &mut self,
        events: Vec<ObserverEvent>,
    ) -> Vec<(String, Arc<Message>)> {
        let mut messages = Vec::new();

        for event in events {
            match event {
                ObserverEvent::BlockProposed { hash } => {
                    if let Some((info, raw)) = self.block_data.get(&hash).cloned() {
                        let info = self.block_info_for_proposal(&info);
                        debug!(
                            block = info.number,
                            hash = %hash,
                            "Publishing BlockProposed to validators"
                        );
                        // Reconstruct the original BlockAvailable payload from the cache:
                        // the consensus tree does not store full BlockInfo, only chain metadata.
                        let msg = Arc::new(Message::Cardano((
                            info,
                            CardanoMessage::BlockAvailable(raw),
                        )));
                        messages.push((self.blocks_proposed_topic.to_string(), msg));
                    } else {
                        warn!("No block data found for proposed block {hash}");
                    }
                }
                ObserverEvent::Rollback { to_block_number } => {
                    let point = self.find_point_at_number(to_block_number);
                    let block_info = self.find_block_info_at_number(to_block_number);
                    self.pending_post_rollback_marker = Some(to_block_number);
                    let msg = Arc::new(Message::Cardano((
                        block_info,
                        CardanoMessage::StateTransition(StateTransitionMessage::Rollback(point)),
                    )));
                    messages.push((self.blocks_proposed_topic.to_string(), msg));
                    info!("Rollback to block number {to_block_number}");
                }
                ObserverEvent::BlockRejected { hash } => {
                    let slot = self
                        .block_data
                        .get(&hash)
                        .map(|(info, _)| info.slot)
                        .or_else(|| self.tree.get_block(&hash).map(|b| b.slot))
                        .unwrap_or(0);
                    let msg = Arc::new(Message::Consensus(ConsensusMessage::BlockRejected(
                        BlockRejectedMessage { hash, slot },
                    )));
                    messages.push((self.consensus_wants_topic.to_string(), msg));
                }
            }
        }

        messages
    }

    fn block_info_for_proposal(&mut self, info: &BlockInfo) -> BlockInfo {
        match self.pending_post_rollback_marker {
            Some(rollback_to) if info.number > rollback_to => {
                self.pending_post_rollback_marker = None;
                info.with_status(BlockStatus::RolledBack)
            }
            _ => info.clone(),
        }
    }

    // Validator responses are accepted without checking that response BlockInfo.hash
    // matches the block currently being validated. Correctness therefore relies on a
    // strict FIFO invariant across all paths: each published block yields exactly one
    // response per validator, and consensus consumes exactly one response per validator
    // per published block.
    //
    // If that invariant is broken (for example, a block is published to validators but
    // not consumed in consensus), queues can drift and responses may be applied to the
    // wrong block silently. The hardening path is hash correlation at read time plus an
    // audit that all produced responses are eventually consumed.
    //
    // This helper is shared by direct and consensus flows.
    fn all_validators_say_go(block_number: u64, result: ValidationBatchResult) -> bool {
        match result {
            Ok(Ok(results)) => {
                results.iter().fold(true, |all_ok, (topic, msg)| match msg.as_ref() {
                    Message::Cardano((_, CardanoMessage::BlockValidation(status))) => {
                        match status {
                            ValidationStatus::Go => all_ok,
                            ValidationStatus::NoGo(err) => {
                                error!(
                                    block = block_number,
                                    ?err,
                                    "Validation failure: {topic}, result {msg:?}"
                                );
                                false
                            }
                        }
                    }
                    _ => {
                        error!("Unexpected validation message type: {msg:?}");
                        false
                    }
                })
            }
            Ok(Err(e)) => {
                error!("Failed to read validations: {e}");
                false
            }
            Err(_) => {
                error!("Timeout waiting for validation responses");
                false
            }
        }
    }

    /// Decode the parent hash from a raw block header.
    fn extract_parent_hash(era: Era, header_bytes: &[u8]) -> Result<Option<BlockHash>> {
        let header = MultiEraHeader::decode(era as u8, None, header_bytes)?;
        Ok(header.previous_hash().map(|h| BlockHash::from(*h)))
    }

    /// Build `BlockWanted` payloads from tree metadata.
    fn build_block_wanted_messages(&self, wanted: &[BlockHash]) -> Vec<BlockWantedMessage> {
        wanted
            .iter()
            .map(|hash| BlockWantedMessage {
                hash: *hash,
                slot: self.tree.get_block(hash).map_or(0, |b| b.slot),
            })
            .collect()
    }

    /// Find the Point for a block at a given number by walking the tree.
    fn find_point_at_number(&self, number: u64) -> Point {
        let mut current = self.tree.favoured_tip();
        while let Some(h) = current {
            if let Some(b) = self.tree.get_block(&h) {
                if b.number == number {
                    return Point::Specific {
                        hash: b.hash,
                        slot: b.slot,
                    };
                }
                current = b.parent;
            } else {
                break;
            }
        }
        Point::Origin
    }

    /// Find or construct a BlockInfo for a block at a given number.
    fn find_block_info_at_number(&self, number: u64) -> BlockInfo {
        let mut current = self.tree.favoured_tip();
        while let Some(h) = current {
            if let Some(b) = self.tree.get_block(&h) {
                if b.number == number {
                    if let Some((info, _)) = self.block_data.get(&h) {
                        return info.with_status(BlockStatus::RolledBack);
                    }
                    return Self::default_rollback_block_info(b.number, b.slot, b.hash);
                }
                current = b.parent;
            } else {
                break;
            }
        }
        Self::default_rollback_block_info(number, 0, BlockHash::default())
    }

    /// Construct a default BlockInfo with minimal fields populated.
    fn default_rollback_block_info(number: u64, slot: u64, hash: BlockHash) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::RolledBack,
            intent: BlockIntent::Apply,
            slot,
            number,
            hash,
            epoch: 0,
            epoch_slot: 0,
            new_epoch: false,
            is_new_era: false,
            tip_slot: None,
            timestamp: 0,
            era: Era::Conway,
        }
    }

    /// Direct flow: pass-through BlockAvailable and Rollback (main-branch behavior).
    /// TODO: Temporary until consensus flow fully works
    async fn run_direct(
        &mut self,
        mut block_subscription: Box<dyn Subscription<Message>>,
        force_validation: bool,
    ) {
        loop {
            let Ok((_, message)) = block_subscription.read().await else {
                error!("Block message read failed");
                return;
            };

            match message.as_ref() {
                Message::Cardano((raw_blk_info, CardanoMessage::BlockAvailable(raw_block))) => {
                    let block_info = if force_validation && self.do_validation {
                        raw_blk_info.with_intent(BlockIntent::ValidateAndApply)
                    } else {
                        raw_blk_info.clone()
                    };

                    let span = info_span!("consensus", block = block_info.number);
                    self.handle_block_available_direct(block_info, raw_block.clone())
                        .instrument(span)
                        .await;
                }

                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                )) => {
                    self.context
                        .message_bus
                        .publish(&self.blocks_proposed_topic, message.clone())
                        .await
                        .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                }

                _ => error!("Unexpected message type: {message:?}"),
            }
        }
    }

    /// Direct flow: pass-through like main branch.
    /// TODO: Temporary until consensus flow fully works
    async fn handle_block_available_direct(
        &mut self,
        block_info: BlockInfo,
        raw_block: RawBlockMessage,
    ) {
        // Send to all validators and state modules
        let block = Arc::new(Message::Cardano((
            block_info.clone(),
            CardanoMessage::BlockAvailable(raw_block),
        )));

        self.context
            .message_bus
            .publish(&self.blocks_proposed_topic, block)
            .await
            .unwrap_or_else(|e| error!("Failed to publish: {e}"));

        if !self.do_validation || !block_info.intent.do_validation() {
            return;
        }

        let completed_tasks = Arc::new(Mutex::new(
            HashMap::<String, Option<Arc<Message>>>::from_iter(
                self.validator_topics.iter().map(|s| (s.clone(), None)),
            ),
        ));

        let all_say_go = Self::all_validators_say_go(
            block_info.number,
            timeout(
                self.validation_timeout,
                try_join_all(self.validator_subscriptions.iter_mut().map(|s| async {
                    let (topic, res) = s.read().await?;
                    completed_tasks.lock().await.insert(topic.clone(), Some(res.clone()));
                    Ok::<(String, Arc<Message>), anyhow::Error>((topic, res))
                })),
            )
            .await,
        );

        if !all_say_go {
            error!(
                block = block_info.number,
                "Validation rejected block, results available:"
            );
            let completed_tasks = completed_tasks.lock().await;
            for (topic, msg) in completed_tasks.iter() {
                error!("Topic {topic}, result {msg:?}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use caryatid_sdk::{context::Context, mock_bus::MockBus};
    use config::{Config, FileFormat};
    use tokio::sync::watch;

    fn hash(byte: u8) -> BlockHash {
        BlockHash::from([byte; 32])
    }

    fn raw_block(byte: u8) -> RawBlockMessage {
        RawBlockMessage {
            header: vec![byte],
            body: vec![byte],
        }
    }

    fn block_info(number: u64, hash: BlockHash) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Volatile,
            intent: BlockIntent::Apply,
            slot: number,
            number,
            hash,
            epoch: 0,
            epoch_slot: number,
            new_epoch: false,
            is_new_era: false,
            tip_slot: None,
            timestamp: number,
            era: Era::Conway,
        }
    }

    fn test_runtime() -> ConsensusRuntime {
        let config = Config::builder()
            .add_source(config::File::from_str("", FileFormat::Toml))
            .build()
            .unwrap();
        let bus = Arc::new(MockBus::<Message>::new(&config));
        let (_tx, rx) = watch::channel(false);
        let context = Arc::new(Context::new(Arc::new(config), bus, rx));

        let event_queue: EventQueue = Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut tree = ConsensusTree::new(
            SECURITY_PARAMETER_K,
            Box::new(QueuingConsensusTreeObserver {
                events: event_queue.clone(),
            }),
        );
        tree.set_root(hash(1), 2370, 420_859).unwrap();

        let mut block_data = HashMap::new();
        block_data.insert(hash(1), (block_info(2370, hash(1)), raw_block(1)));
        block_data.insert(hash(2), (block_info(2371, hash(2)), raw_block(2)));
        block_data.insert(hash(3), (block_info(2372, hash(3)), raw_block(3)));
        block_data.insert(hash(4), (block_info(2361, hash(4)), raw_block(4)));

        ConsensusRuntime {
            context,
            blocks_proposed_topic: DEFAULT_BLOCKS_PROPOSED_TOPIC.1.to_string(),
            consensus_wants_topic: DEFAULT_CONSENSUS_WANTS_TOPIC.1.to_string(),
            event_queue,
            pending_post_rollback_marker: None,
            tree,
            block_data,
            validator_topics: Vec::new(),
            validator_subscriptions: Vec::new(),
            validation_timeout: Duration::from_secs(1),
            do_validation: false,
            stats: ConsensusStats::default(),
        }
    }

    fn message_at(messages: &[(String, Arc<Message>)], index: usize) -> &Message {
        messages[index].1.as_ref()
    }

    #[test]
    fn rollback_marks_first_subsequent_proposed_block() {
        let mut runtime = test_runtime();

        let messages = runtime.resolve_observer_events(vec![
            ObserverEvent::Rollback {
                to_block_number: 2370,
            },
            ObserverEvent::BlockProposed { hash: hash(2) },
        ]);

        assert_eq!(messages.len(), 2);

        match message_at(&messages, 0) {
            Message::Cardano((
                info,
                CardanoMessage::StateTransition(StateTransitionMessage::Rollback(point)),
            )) => {
                assert_eq!(info.number, 2370);
                assert_eq!(info.status, BlockStatus::RolledBack);
                assert_eq!(
                    *point,
                    Point::Specific {
                        hash: hash(1),
                        slot: 420_859,
                    }
                );
            }
            other => panic!("unexpected rollback message: {other:?}"),
        }

        match message_at(&messages, 1) {
            Message::Cardano((info, CardanoMessage::BlockAvailable(_))) => {
                assert_eq!(info.number, 2371);
                assert_eq!(info.status, BlockStatus::RolledBack);
            }
            other => panic!("unexpected proposed message: {other:?}"),
        }
    }

    #[test]
    fn only_first_proposed_block_after_rollback_is_marked() {
        let mut runtime = test_runtime();

        let messages = runtime.resolve_observer_events(vec![
            ObserverEvent::Rollback {
                to_block_number: 2370,
            },
            ObserverEvent::BlockProposed { hash: hash(2) },
            ObserverEvent::BlockProposed { hash: hash(3) },
        ]);

        assert_eq!(messages.len(), 3);

        match message_at(&messages, 1) {
            Message::Cardano((info, CardanoMessage::BlockAvailable(_))) => {
                assert_eq!(info.number, 2371);
                assert_eq!(info.status, BlockStatus::RolledBack);
            }
            other => panic!("unexpected first proposed message: {other:?}"),
        }

        match message_at(&messages, 2) {
            Message::Cardano((info, CardanoMessage::BlockAvailable(_))) => {
                assert_eq!(info.number, 2372);
                assert_eq!(info.status, BlockStatus::Volatile);
            }
            other => panic!("unexpected second proposed message: {other:?}"),
        }
    }

    #[test]
    fn newer_rollback_replaces_pending_marker_before_next_proposal() {
        let mut runtime = test_runtime();

        let messages = runtime.resolve_observer_events(vec![
            ObserverEvent::Rollback {
                to_block_number: 2370,
            },
            ObserverEvent::Rollback {
                to_block_number: 2360,
            },
            ObserverEvent::BlockProposed { hash: hash(4) },
        ]);

        assert_eq!(messages.len(), 3);

        match message_at(&messages, 2) {
            Message::Cardano((info, CardanoMessage::BlockAvailable(_))) => {
                assert_eq!(info.number, 2361);
                assert_eq!(info.status, BlockStatus::RolledBack);
            }
            other => panic!("unexpected proposed message after second rollback: {other:?}"),
        }
    }
}
