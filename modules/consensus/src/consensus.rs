//! Acropolis consensus module for Caryatid
//! Maintains a favoured chain based on offered options from multiple sources

pub mod consensus_tree;
pub mod tree_block;
pub mod tree_error;
pub mod tree_observer;

use acropolis_common::{
    messages::{
        BlockRejectedMessage, BlockWantedMessage, CardanoMessage, ConsensusMessage, Message,
        RawBlockMessage, StateTransitionMessage,
    },
    types::{BlockInfo, Point},
    validation::ValidationStatus,
    BlockHash, BlockIntent, BlockStatus,
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
use tree_observer::ConsensusTreeObserver;

const DEFAULT_SUBSCRIBE_BLOCKS_TOPIC: &str = "cardano.block.available";
const DEFAULT_PUBLISH_BLOCKS_TOPIC: &str = "cardano.block.proposed";
const DEFAULT_CONSENSUS_TOPIC: &str = "cardano.consensus";
const DEFAULT_PUBLISH_CONSENSUS_TOPIC: &str = "cardano.consensus";
const DEFAULT_VALIDATION_TIMEOUT: i64 = 60; // seconds
const DEFAULT_SECURITY_PARAMETER: u64 = 2160;

/// Consensus flow handling strategies.
#[derive(Clone, Copy, Debug, Default, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
enum ConsensusFlowMode {
    /// Direct flow: pass-through like main branch (no chain selection).
    #[default]
    Direct,
    /// Consensus flow: use offers/wants with chain selection.
    Consensus,
}

impl ConsensusFlowMode {
    fn from_config(config: &Config) -> Self {
        config.get::<ConsensusFlowMode>("consensus-flow-mode").unwrap_or_default()
    }
}

impl std::fmt::Display for ConsensusFlowMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsensusFlowMode::Direct => write!(f, "direct"),
            ConsensusFlowMode::Consensus => write!(f, "consensus"),
        }
    }
}

/// Events emitted by the consensus tree observer, queued for async publishing.
enum ObserverEvent {
    BlockProposed { hash: BlockHash },
    Rollback { to_block_number: u64 },
    BlockRejected { hash: BlockHash },
}

/// Shared event queue between the observer and the main loop.
type EventQueue = Arc<std::sync::Mutex<Vec<ObserverEvent>>>;

/// Observer that queues tree events for later async publishing.
struct QueueObserver {
    events: EventQueue,
}

impl ConsensusTreeObserver for QueueObserver {
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
    publish_blocks_topic: String,
    publish_consensus_topic: String,
    event_queue: EventQueue,
    tree: ConsensusTree,
    block_data: HashMap<BlockHash, (BlockInfo, RawBlockMessage)>,
    validator_topics: Vec<String>,
    validator_subscriptions: Vec<Box<dyn Subscription<Message>>>,
    validation_timeout: Duration,
    do_validation: bool,
    stats: ConsensusStats,
}

/// Periodic logging counters.
#[derive(Default)]
struct ConsensusStats {
    offered: u64,
    wanted: u64,
    available: u64,
    validated: u64,
    proposed: u64,
    rollbacks: u64,
    rejected: u64,
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
        let subscribe_blocks_topic = config
            .get_string("subscribe-blocks-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_BLOCKS_TOPIC.to_string());
        info!("Creating blocks subscriber on '{subscribe_blocks_topic}'");

        let publish_blocks_topic = config
            .get_string("publish-blocks-topic")
            .unwrap_or(DEFAULT_PUBLISH_BLOCKS_TOPIC.to_string());
        info!("Publishing blocks on '{publish_blocks_topic}'");

        let consensus_topic =
            config.get_string("consensus-topic").unwrap_or(DEFAULT_CONSENSUS_TOPIC.to_string());
        info!("Subscribing to consensus messages on '{consensus_topic}'");

        let publish_consensus_topic = config
            .get_string("publish-consensus-topic")
            .unwrap_or(DEFAULT_PUBLISH_CONSENSUS_TOPIC.to_string());

        let validator_topics: Vec<String> =
            config.get::<Vec<String>>("validators").unwrap_or_default();
        for topic in &validator_topics {
            info!("Validator: {topic}");
        }

        let validation_timeout = Duration::from_secs(
            config.get_int("validation-timeout").unwrap_or(DEFAULT_VALIDATION_TIMEOUT) as u64,
        );
        info!("Validation timeout {validation_timeout:?}");

        let security_parameter = config
            .get_int("security-parameter")
            .unwrap_or(DEFAULT_SECURITY_PARAMETER as i64) as u64;
        info!("Security parameter k={security_parameter}");

        // Subscribe for incoming blocks (PNI → CON: BlockAvailable)
        let block_subscription = context.subscribe(&subscribe_blocks_topic).await?;

        let flow_mode = ConsensusFlowMode::from_config(&config);
        info!("Consensus flow mode: {flow_mode}");

        // Subscribe for consensus messages (PNI → CON: BlockOffered, BlockRescinded)
        // TODO: Temporary until consensus flow fully works.
        let consensus_subscription = if flow_mode == ConsensusFlowMode::Consensus {
            Some(context.subscribe(&consensus_topic).await?)
        } else {
            None
        };

        // Subscribe all the validators
        let validator_subscriptions: Vec<_> =
            try_join_all(validator_topics.iter().map(|topic| context.subscribe(topic))).await?;

        let do_validation = !validator_subscriptions.is_empty();

        // Create the consensus tree with a queue-based observer
        let event_queue: EventQueue = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observer = Box::new(QueueObserver {
            events: event_queue.clone(),
        });
        let tree = ConsensusTree::new(security_parameter, observer);

        let mut runtime = ConsensusRuntime {
            context: context.clone(),
            publish_blocks_topic,
            publish_consensus_topic,
            event_queue,
            tree,
            block_data: HashMap::new(),
            validator_topics,
            validator_subscriptions,
            validation_timeout,
            do_validation,
            stats: ConsensusStats::default(),
        };

        context.run(async move {
            // TODO: Temporary until consensus flow fully works.
            match flow_mode {
                ConsensusFlowMode::Direct => {
                    runtime.run_direct(block_subscription).await;
                }
                ConsensusFlowMode::Consensus => {
                    let consensus_subscription = consensus_subscription
                        .expect("consensus subscription missing for consensus flow mode");
                    runtime.run_consensus(block_subscription, consensus_subscription).await;
                }
            }
        });

        Ok(())
    }
}

const STATS_LOG_INTERVAL: u64 = 100;

impl ConsensusStats {
    fn total(&self) -> u64 {
        self.offered + self.available
    }

    fn maybe_log(&self) {
        let total = self.total();
        if total > 0 && total % STATS_LOG_INTERVAL == 0 {
            info!(
                "Consensus stats: offered={}, wanted={}, available={}, validated={}, proposed={}, rollbacks={}, rejected={}",
                self.offered, self.wanted, self.available, self.validated, self.proposed, self.rollbacks, self.rejected
            );
        }
    }
}

impl ConsensusRuntime {
    /// Main select loop for consensus flow: dispatches incoming messages to handler functions.
    async fn run_consensus(
        &mut self,
        mut block_subscription: Box<dyn Subscription<Message>>,
        mut consensus_subscription: Box<dyn Subscription<Message>>,
    ) {
        loop {
            tokio::select! {
                result = block_subscription.read() => {
                    let Ok((_, message)) = result else {
                        error!("Block message read failed");
                        return;
                    };

                    match message.as_ref() {
                        Message::Cardano((raw_blk_info, CardanoMessage::BlockAvailable(raw_block))) => {
                            let block_info = if self.do_validation {
                                raw_blk_info.with_intent(BlockIntent::ValidateAndApply)
                            } else {
                                raw_blk_info.clone()
                            };

                            let span = info_span!("consensus", block = block_info.number);
                            self.handle_block_available_consensus(block_info, raw_block.clone())
                                .instrument(span)
                                .await;
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
                            self.stats.maybe_log();
                        }

                        Message::Consensus(ConsensusMessage::BlockRescinded(rescinded)) => {
                            let span = info_span!("consensus-rescinded", hash = %rescinded.hash);
                            self.handle_block_rescinded(rescinded.hash)
                                .instrument(span)
                                .await;
                        }

                        _ => debug!("Ignoring unknown consensus message"),
                    }
                }
            }
        }
    }

    /// Direct flow: pass-through BlockAvailable and Rollback (main-branch behavior).
    /// TODO: Temporary until consensus flow fully works
    async fn run_direct(&mut self, mut block_subscription: Box<dyn Subscription<Message>>) {
        loop {
            let Ok((_, message)) = block_subscription.read().await else {
                error!("Block message read failed");
                return;
            };

            match message.as_ref() {
                Message::Cardano((raw_blk_info, CardanoMessage::BlockAvailable(raw_block))) => {
                    let block_info = if self.do_validation {
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
                    // Pass rollback to all validators and state modules.
                    self.context
                        .message_bus
                        .publish(&self.publish_blocks_topic, message.clone())
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
            .publish(&self.publish_blocks_topic, block)
            .await
            .unwrap_or_else(|e| error!("Failed to publish: {e}"));

        if !self.do_validation {
            return;
        }

        let completed_tasks = Arc::new(Mutex::new(
            HashMap::<String, Option<Arc<Message>>>::from_iter(
                self.validator_topics.iter().map(|s| (s.clone(), None)),
            ),
        ));

        let all_say_go = match timeout(
            self.validation_timeout,
            try_join_all(self.validator_subscriptions.iter_mut().map(|s| async {
                let (topic, res) = s.read().await?;
                completed_tasks.lock().await.insert(topic.clone(), Some(res.clone()));
                Ok::<(String, Arc<Message>), anyhow::Error>((topic, res))
            })),
        )
        .await
        {
            Ok(Ok(results)) => {
                results.iter().fold(true, |all_ok, (topic, msg)| match msg.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::BlockValidation(status))) => {
                        match status {
                            ValidationStatus::Go => all_ok,
                            ValidationStatus::NoGo(err) => {
                                error!(
                                    block = block_info.number,
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
        };

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

    /// Consensus flow: require BlockOffered before BlockAvailable (except Mithril bootstrap).
    async fn handle_block_available_consensus(
        &mut self,
        block_info: BlockInfo,
        raw_block: RawBlockMessage,
    ) {
        // Parse header to extract parent hash
        let parent_hash = match extract_parent_hash(&block_info, &raw_block.header) {
            Ok(Some(h)) => h,
            Ok(None) => {
                // Genesis/epoch boundary block — no parent
                debug!(
                    "Block {} has no parent hash (genesis/EB)",
                    block_info.number
                );
                if self.tree.get_block(&block_info.hash).is_none() {
                    if block_info.status == BlockStatus::Immutable {
                        // Mithril bootstrap: genesis block arrives as BlockAvailable without prior BlockOffered.
                        // Use synthetic root so we can add the block with its real body.
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
                }

                // Tree was bootstrapped from BlockOffered (or Mithril); add genesis body so descendants can fire
                self.block_data.insert(block_info.hash, (block_info.clone(), raw_block.clone()));

                if let Err(e) = self.tree.add_block(block_info.hash, raw_block.body.clone()) {
                    error!("Failed to add genesis block body: {e}");
                }
                self.stats.available += 1;
                self.drain_and_publish_events().await;

                // Must consume validation results for any block we proposed, otherwise
                // the validator subscriptions drift out of sync (off-by-one).
                if self.do_validation {
                    if self
                        .tree
                        .favoured_tip()
                        .is_some_and(|tip| self.tree.chain_contains(block_info.hash, tip))
                    {
                        self.handle_validation(&block_info).await;
                    }
                }

                return;
            }
            Err(e) => {
                error!("Failed to parse block header: {e}");
                return;
            }
        };

        if block_info.number <= 2 {
            info!(
                block = block_info.number,
                hash = %block_info.hash,
                tree_empty = self.tree.is_empty(),
                parent_hash = %parent_hash,
                "Received BlockAvailable"
            );
        }

        let existing = self.tree.get_block(&block_info.hash);
        let existing = match existing {
            Some(e) => e,
            None => {
                if block_info.status == BlockStatus::Immutable {
                    // Mithril bootstrap: block arrives as BlockAvailable without prior BlockOffered
                    return self
                        .handle_immutable_bootstrap(block_info, raw_block, parent_hash)
                        .await;
                }
                error!(
                    "BlockAvailable for unknown block {} (not offered) — dropping",
                    block_info.hash
                );
                return;
            }
        };

        // If this block was previously offered, guard against conflicting metadata.
        if existing.parent != Some(parent_hash) || existing.number != block_info.number {
            warn!(
                "Ignoring block {} due to conflicting tree metadata",
                block_info.number
            );
            return;
        }

        // Store block data for later reconstruction
        self.block_data.insert(block_info.hash, (block_info.clone(), raw_block.clone()));

        // We already have the body — store it (idempotent if already stored).
        let had_body =
            self.tree.get_block(&block_info.hash).map(|b| b.body.is_some()).unwrap_or(false);

        if let Err(e) = self.tree.add_block(block_info.hash, raw_block.body.clone()) {
            error!("Failed to add block body: {e}");
        }

        // Collect and publish observer events
        self.drain_and_publish_events().await;

        self.stats.available += 1;

        // Validate only when this call newly supplied body for a favoured block.
        let should_validate = !had_body
            && self
                .tree
                .favoured_tip()
                .is_some_and(|tip| self.tree.chain_contains(block_info.hash, tip));
        if self.do_validation && should_validate {
            self.handle_validation(&block_info).await;
        }

        self.prune_block_data();

        // Prune periodically
        if let Err(e) = self.tree.prune() {
            error!("Prune failed: {e}");
        }
        self.prune_block_data();
    }

    /// Handle BlockAvailable for Immutable blocks from Mithril bootstrap (no prior BlockOffered).
    async fn handle_immutable_bootstrap(
        &mut self,
        block_info: BlockInfo,
        raw_block: RawBlockMessage,
        parent_hash: BlockHash,
    ) {
        if self.tree.is_empty() {
            let parent_number = block_info.number.wrapping_sub(1);
            if let Err(e) = self.tree.set_root(parent_hash, parent_number, 0) {
                error!("Failed to set root for Mithril bootstrap: {e}");
                return;
            }
            debug!(
                "Tree root set to parent {parent_hash} (block {}) for Mithril bootstrap",
                block_info.number.wrapping_sub(1)
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

        self.drain_and_publish_events().await;
        self.stats.available += 1;

        if self.do_validation
            && self
                .tree
                .favoured_tip()
                .is_some_and(|tip| self.tree.chain_contains(block_info.hash, tip))
        {
            self.handle_validation(&block_info).await;
        }

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
        // For block 0 (genesis/first block), use u64::MAX so that 0 == parent_number + 1 (wraparound).
        if self.tree.is_empty() {
            let parent_number = number.wrapping_sub(1);

            if let Err(e) = self.tree.set_root(parent_hash, parent_number, 0) {
                error!("Failed to set tree root from offered block: {e}");
                return;
            }
            debug!("Tree root set to parent {parent_hash} (block {parent_number})");
        }

        if self.tree.get_block(&parent_hash).is_none() {
            let parent_number = number.wrapping_sub(1);

            warn!(
                "Parent {parent_hash} not in tree for offered block {hash} — re-rooting tree at block {parent_number}"
            );
            self.tree = ConsensusTree::new(self.tree.k(), self.tree.take_observer());
            self.block_data.clear();
            if let Err(e) = self.tree.set_root(parent_hash, parent_number, 0) {
                error!("Failed to re-root tree: {e}");
                return;
            }
        }

        let wanted = match self.tree.check_block_wanted(hash, parent_hash, number, slot) {
            Ok(w) => w,
            Err(e) => {
                warn!("Offered block {hash} rejected: {e}");
                return;
            }
        };

        self.stats.offered += 1;
        self.stats.wanted += wanted.len() as u64;

        // Collect and publish observer events
        self.drain_and_publish_events().await;

        let wanted_msgs = build_block_wanted_messages(&self.tree, &wanted);
        self.publish_block_wanted_messages(&wanted_msgs).await;
    }

    /// Handle a BlockRescinded message: remove block, publish events.
    async fn handle_block_rescinded(&mut self, hash: BlockHash) {
        match self.tree.remove_block(hash) {
            Ok(newly_wanted) => {
                self.drain_and_publish_events().await;

                let wanted_msgs = build_block_wanted_messages(&self.tree, &newly_wanted);
                self.publish_block_wanted_messages(&wanted_msgs).await;
                self.prune_block_data();
            }
            Err(e) => {
                warn!("Failed to remove rescinded block {hash}: {e}");
            }
        }
    }

    /// Collect observer events, resolve to messages, and publish.
    async fn drain_and_publish_events(&mut self) {
        let raw_events: Vec<ObserverEvent> = self.event_queue.lock().unwrap().drain(..).collect();
        for event in &raw_events {
            match event {
                ObserverEvent::BlockProposed { .. } => self.stats.proposed += 1,
                ObserverEvent::Rollback { .. } => self.stats.rollbacks += 1,
                ObserverEvent::BlockRejected { .. } => self.stats.rejected += 1,
            }
        }
        let events = resolve_observer_events(
            raw_events,
            &self.publish_blocks_topic,
            &self.publish_consensus_topic,
            &self.block_data,
            &self.tree,
        );
        publish_messages(&self.context, events).await;
    }

    /// Publish `BlockWanted` messages for each hash.
    async fn publish_block_wanted_messages(&mut self, wanted: &[BlockWantedMessage]) {
        for wanted_block in wanted {
            let msg = Arc::new(Message::Consensus(ConsensusMessage::BlockWanted(
                wanted_block.clone(),
            )));
            self.context
                .message_bus
                .publish(&self.publish_consensus_topic, msg)
                .await
                .unwrap_or_else(|e| error!("Failed to publish BlockWanted: {e}"));
        }
    }

    /// Keep only metadata for blocks still present in the tree.
    fn prune_block_data(&mut self) {
        self.block_data.retain(|hash, _| self.tree.get_block(hash).is_some());
    }

    /// Collect validation responses and update the tree accordingly.
    async fn handle_validation(&mut self, block_info: &BlockInfo) {
        let completed_tasks = Arc::new(Mutex::new(
            HashMap::<String, Option<Arc<Message>>>::from_iter(
                self.validator_topics.iter().map(|s| (s.clone(), None)),
            ),
        ));

        let mut validation_results: Vec<(String, Arc<Message>)> = Vec::new();
        let all_say_go = match timeout(self.validation_timeout, async {
            for sub in self.validator_subscriptions.iter_mut() {
                let (topic, res) = sub.read().await?;
                completed_tasks.lock().await.insert(topic.clone(), Some(res.clone()));
                validation_results.push((topic, res));
            }
            Ok::<(), anyhow::Error>(())
        })
        .await
        {
            Ok(Ok(())) => {
                validation_results.iter().fold(true, |all_ok, (topic, msg)| match msg.as_ref() {
                    Message::Cardano((_, CardanoMessage::BlockValidation(status))) => {
                        match status {
                            ValidationStatus::Go => all_ok,
                            ValidationStatus::NoGo(err) => {
                                error!(
                                    block = block_info.number,
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
        };

        if all_say_go {
            self.stats.validated += 1;
            if let Err(e) = self.tree.mark_validated(block_info.hash) {
                error!("Failed to mark block validated: {e}");
            }
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
            self.drain_and_publish_events().await;
            self.prune_block_data();
        }
    }
}

/// Extract the parent hash from a raw block header using pallas.
fn extract_parent_hash(block_info: &BlockInfo, header_bytes: &[u8]) -> Result<Option<BlockHash>> {
    let header = MultiEraHeader::decode(block_info.era as u8, None, header_bytes)?;
    Ok(header.previous_hash().map(|h| BlockHash::from(*h)))
}

/// Resolve pre-drained observer events into publishable messages.
///
/// Sync function — does not hold tree references across await points.
fn resolve_observer_events(
    events: Vec<ObserverEvent>,
    publish_blocks_topic: &str,
    publish_consensus_topic: &str,
    block_data: &HashMap<BlockHash, (BlockInfo, RawBlockMessage)>,
    tree: &ConsensusTree,
) -> Vec<(String, Arc<Message>)> {
    let mut messages = Vec::new();

    for event in events {
        match event {
            ObserverEvent::BlockProposed { hash } => {
                if let Some((info, raw)) = block_data.get(&hash) {
                    debug!(
                        block = info.number,
                        hash = %hash,
                        "Publishing BlockProposed to validators"
                    );
                    let msg = Arc::new(Message::Cardano((
                        info.clone(),
                        CardanoMessage::BlockAvailable(raw.clone()),
                    )));
                    messages.push((publish_blocks_topic.to_string(), msg));
                } else {
                    warn!("No block data found for proposed block {hash}");
                }
            }
            ObserverEvent::Rollback { to_block_number } => {
                let point = find_point_at_number(tree, to_block_number);
                let block_info = find_block_info_at_number(block_data, tree, to_block_number);
                let msg = Arc::new(Message::Cardano((
                    block_info,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(point)),
                )));
                messages.push((publish_blocks_topic.to_string(), msg));
                info!("Rollback to block number {to_block_number}");
            }
            ObserverEvent::BlockRejected { hash } => {
                let slot = block_data
                    .get(&hash)
                    .map(|(info, _)| info.slot)
                    .or_else(|| tree.get_block(&hash).map(|b| b.slot))
                    .unwrap_or(0);
                let msg = Arc::new(Message::Consensus(ConsensusMessage::BlockRejected(
                    BlockRejectedMessage { hash, slot },
                )));
                messages.push((publish_consensus_topic.to_string(), msg));
            }
        }
    }

    messages
}

/// Publish a batch of collected messages to the bus.
async fn publish_messages(context: &Arc<Context<Message>>, messages: Vec<(String, Arc<Message>)>) {
    for (topic, msg) in messages {
        context
            .message_bus
            .publish(&topic, msg)
            .await
            .unwrap_or_else(|e| error!("Failed to publish to {topic}: {e}"));
    }
}

/// Build `BlockWanted` payloads from tree metadata.
fn build_block_wanted_messages(
    tree: &ConsensusTree,
    wanted: &[BlockHash],
) -> Vec<BlockWantedMessage> {
    wanted
        .iter()
        .map(|hash| BlockWantedMessage {
            hash: *hash,
            slot: tree.get_block(hash).map_or(0, |b| b.slot),
        })
        .collect()
}

/// Find the Point for a block at a given number by walking the tree.
fn find_point_at_number(tree: &ConsensusTree, number: u64) -> Point {
    let mut current = tree.favoured_tip();
    while let Some(h) = current {
        if let Some(b) = tree.get_block(&h) {
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

/// Construct a default BlockInfo with minimal fields populated.
fn default_block_info(number: u64, slot: u64, hash: BlockHash) -> BlockInfo {
    BlockInfo {
        status: acropolis_common::BlockStatus::Volatile,
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
        era: acropolis_common::Era::Conway,
    }
}

/// Find or construct a BlockInfo for a block at a given number.
fn find_block_info_at_number(
    block_data: &HashMap<BlockHash, (BlockInfo, RawBlockMessage)>,
    tree: &ConsensusTree,
    number: u64,
) -> BlockInfo {
    let mut current = tree.favoured_tip();
    while let Some(h) = current {
        if let Some(b) = tree.get_block(&h) {
            if b.number == number {
                if let Some((info, _)) = block_data.get(&h) {
                    return info.clone();
                }
                return default_block_info(b.number, b.slot, b.hash);
            }
            current = b.parent;
        } else {
            break;
        }
    }
    default_block_info(number, 0, BlockHash::default())
}
