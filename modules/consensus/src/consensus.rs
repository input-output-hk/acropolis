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
    BlockHash, BlockIntent,
};
use anyhow::Result;
use caryatid_sdk::{module, Context};
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
        let mut block_subscription = context.subscribe(&subscribe_blocks_topic).await?;

        // Subscribe for consensus messages (PNI → CON: BlockOffered, BlockRescinded)
        let mut consensus_subscription = context.subscribe(&consensus_topic).await?;

        // Subscribe all the validators
        let mut validator_subscriptions: Vec<_> =
            try_join_all(validator_topics.iter().map(|topic| context.subscribe(topic))).await?;

        let do_validation = !validator_subscriptions.is_empty();

        // Create the consensus tree with a queue-based observer
        let event_queue: EventQueue = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observer = Box::new(QueueObserver {
            events: event_queue.clone(),
        });
        let mut tree = ConsensusTree::new(security_parameter, observer);

        // Map block hash → (BlockInfo, RawBlockMessage) for reconstructing published messages
        let mut block_data: HashMap<BlockHash, (BlockInfo, RawBlockMessage)> = HashMap::new();

        context.clone().run(async move {
            loop {
                // Wait for either a block or consensus message
                tokio::select! {
                    result = block_subscription.read() => {
                        let Ok((_, message)) = result else {
                            error!("Block message read failed");
                            return;
                        };

                        match message.as_ref() {
                            Message::Cardano((raw_blk_info, CardanoMessage::BlockAvailable(raw_block))) => {
                                let block_info = if do_validation {
                                    raw_blk_info.with_intent(BlockIntent::ValidateAndApply)
                                } else {
                                    raw_blk_info.clone()
                                };

                                let span = info_span!("consensus", block = block_info.number);

                                async {
                                    // Parse header to extract parent hash
                                    let parent_hash = match extract_parent_hash(&block_info, &raw_block.header) {
                                        Ok(Some(h)) => h,
                                        Ok(None) => {
                                            // Genesis/epoch boundary block — no parent
                                            debug!("Block {} has no parent hash (genesis/EB)", block_info.number);
                                            if tree.is_empty() {
                                                if let Err(e) = tree.set_root(block_info.hash, block_info.number, block_info.slot) {
                                                    error!("Failed to set root: {e}");
                                                    return;
                                                }
                                                block_data.insert(block_info.hash, (block_info.clone(), raw_block.clone()));

                                                let msg = Arc::new(Message::Cardano((
                                                    block_info.clone(),
                                                    CardanoMessage::BlockAvailable(raw_block.clone()),
                                                )));
                                                context.message_bus.publish(&publish_blocks_topic, msg).await
                                                    .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                                            }
                                            return;
                                        }
                                        Err(e) => {
                                            error!("Failed to parse block header: {e}");
                                            return;
                                        }
                                    };

                                    // If tree is empty, set root using parent as virtual root
                                    if tree.is_empty() {
                                        let parent_number = block_info.number.saturating_sub(1);
                                        if let Err(e) = tree.set_root(parent_hash, parent_number, 0) {
                                            error!("Failed to set tree root: {e}");
                                            return;
                                        }
                                        debug!("Tree root set to parent block {parent_number}");
                                    }

                                    // Store block data for later reconstruction
                                    block_data.insert(block_info.hash, (block_info.clone(), raw_block.clone()));

                                    let mut wanted = Vec::new();
                                    let block_already_registered = tree.get_block(&block_info.hash).is_some();
                                    if !block_already_registered {
                                        // Register block with the tree (chain selection)
                                        wanted = match tree.check_block_wanted(
                                            block_info.hash,
                                            parent_hash,
                                            block_info.number,
                                            block_info.slot,
                                        ) {
                                            Ok(w) => w,
                                            Err(e) => {
                                                warn!("Block {} rejected by tree: {e}", block_info.number);
                                                return;
                                            }
                                        };
                                    }

                                    // If this block was previously offered, guard against conflicting metadata.
                                    if let Some(existing) = tree.get_block(&block_info.hash) {
                                        if existing.parent != Some(parent_hash) || existing.number != block_info.number {
                                            warn!(
                                                "Ignoring block {} due to conflicting tree metadata",
                                                block_info.number
                                            );
                                            return;
                                        }
                                    }

                                    // Notify PNI about any newly-wanted blocks on a chain switch.
                                    let newly_wanted: Vec<BlockHash> = wanted
                                        .iter()
                                        .copied()
                                        .filter(|h| *h != block_info.hash)
                                        .collect();
                                    publish_block_wanted_messages(
                                        &context,
                                        &publish_consensus_topic,
                                        &build_block_wanted_messages(&tree, &newly_wanted),
                                    )
                                    .await;

                                    // We already have the body — store it (idempotent if already stored).
                                    let had_body = tree
                                        .get_block(&block_info.hash)
                                        .map(|b| b.body.is_some())
                                        .unwrap_or(false);
                                    if let Err(e) = tree.add_block(block_info.hash, raw_block.body.clone()) {
                                        error!("Failed to add block body: {e}");
                                    }

                                    // Collect and publish observer events
                                    let events = collect_observer_events(
                                        &event_queue,
                                        &publish_blocks_topic,
                                        &publish_consensus_topic,
                                        &block_data,
                                        &tree,
                                    );
                                    publish_messages(&context, events).await;

                                    // Validate only when this call newly supplied body for a favoured block.
                                    let should_validate = !had_body
                                        && tree
                                            .favoured_tip()
                                            .is_some_and(|tip| tree.chain_contains(block_info.hash, tip));
                                    if do_validation && should_validate {
                                        handle_validation(
                                            &block_info,
                                            &validator_topics,
                                            &mut validator_subscriptions,
                                            validation_timeout,
                                            &mut tree,
                                            &event_queue,
                                            &context,
                                            &publish_blocks_topic,
                                            &publish_consensus_topic,
                                            &mut block_data,
                                        ).await;
                                    }

                                    prune_block_data(&mut block_data, &tree);

                                    // Prune periodically
                                    if let Err(e) = tree.prune() {
                                        error!("Prune failed: {e}");
                                    }
                                    prune_block_data(&mut block_data, &tree);
                                }
                                .instrument(span)
                                .await;
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
                                async {
                                    // Derive block number from parent in tree
                                    let number = match tree.get_block(&offered.parent_hash) {
                                        Some(parent) => parent.number + 1,
                                        None => {
                                            warn!("Parent {} not in tree for offered block {}", offered.parent_hash, offered.hash);
                                            return;
                                        }
                                    };

                                    let wanted = match tree.check_block_wanted(
                                        offered.hash,
                                        offered.parent_hash,
                                        number,
                                        offered.slot,
                                    ) {
                                        Ok(w) => w,
                                        Err(e) => {
                                            warn!("Offered block {} rejected: {e}", offered.hash);
                                            return;
                                        }
                                    };

                                    // Collect and publish observer events
                                    let events = collect_observer_events(
                                        &event_queue,
                                        &publish_blocks_topic,
                                        &publish_consensus_topic,
                                        &block_data,
                                        &tree,
                                    );
                                    publish_messages(&context, events).await;

                                    publish_block_wanted_messages(
                                        &context,
                                        &publish_consensus_topic,
                                        &build_block_wanted_messages(&tree, &wanted),
                                    )
                                    .await;
                                }
                                .instrument(span)
                                .await;
                            }

                            Message::Consensus(ConsensusMessage::BlockRescinded(rescinded)) => {
                                let span = info_span!("consensus-rescinded", hash = %rescinded.hash);
                                async {
                                    match tree.remove_block(rescinded.hash) {
                                        Ok(newly_wanted) => {
                                            // Collect and publish observer events
                                            let events = collect_observer_events(
                                                &event_queue,
                                                &publish_blocks_topic,
                                                &publish_consensus_topic,
                                                &block_data,
                                                &tree,
                                            );
                                            publish_messages(&context, events).await;

                                            publish_block_wanted_messages(
                                                &context,
                                                &publish_consensus_topic,
                                                &build_block_wanted_messages(&tree, &newly_wanted),
                                            )
                                            .await;
                                            prune_block_data(&mut block_data, &tree);
                                        }
                                        Err(e) => {
                                            warn!("Failed to remove rescinded block {}: {e}", rescinded.hash);
                                        }
                                    }
                                }
                                .instrument(span)
                                .await;
                            }

                            _ => debug!("Ignoring unknown consensus message"),
                        }
                    }
                }
            }
        });

        Ok(())
    }
}

/// Extract the parent hash from a raw block header using pallas.
fn extract_parent_hash(block_info: &BlockInfo, header_bytes: &[u8]) -> Result<Option<BlockHash>> {
    let header = MultiEraHeader::decode(block_info.era as u8, None, header_bytes)?;
    Ok(header.previous_hash().map(|h| BlockHash::from(*h)))
}

/// Collect observer events and resolve them into publishable messages.
///
/// Sync function — does not hold tree references across await points.
fn collect_observer_events(
    event_queue: &EventQueue,
    publish_blocks_topic: &str,
    publish_consensus_topic: &str,
    block_data: &HashMap<BlockHash, (BlockInfo, RawBlockMessage)>,
    tree: &ConsensusTree,
) -> Vec<(String, Arc<Message>)> {
    let events: Vec<ObserverEvent> = event_queue.lock().unwrap().drain(..).collect();
    let mut messages = Vec::new();

    for event in events {
        match event {
            ObserverEvent::BlockProposed { hash } => {
                if let Some((info, raw)) = block_data.get(&hash) {
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

/// Publish `BlockWanted` messages for each hash.
async fn publish_block_wanted_messages(
    context: &Arc<Context<Message>>,
    publish_consensus_topic: &str,
    wanted: &[BlockWantedMessage],
) {
    for wanted_block in wanted {
        let msg = Arc::new(Message::Consensus(ConsensusMessage::BlockWanted(
            wanted_block.clone(),
        )));
        context
            .message_bus
            .publish(publish_consensus_topic, msg)
            .await
            .unwrap_or_else(|e| error!("Failed to publish BlockWanted: {e}"));
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

/// Keep only metadata for blocks still present in the tree.
fn prune_block_data(
    block_data: &mut HashMap<BlockHash, (BlockInfo, RawBlockMessage)>,
    tree: &ConsensusTree,
) {
    block_data.retain(|hash, _| tree.get_block(hash).is_some());
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
                return BlockInfo {
                    status: acropolis_common::BlockStatus::Volatile,
                    intent: BlockIntent::Apply,
                    slot: b.slot,
                    number: b.number,
                    hash: b.hash,
                    epoch: 0,
                    epoch_slot: 0,
                    new_epoch: false,
                    is_new_era: false,
                    tip_slot: None,
                    timestamp: 0,
                    era: acropolis_common::Era::Conway,
                };
            }
            current = b.parent;
        } else {
            break;
        }
    }
    BlockInfo {
        status: acropolis_common::BlockStatus::Volatile,
        intent: BlockIntent::Apply,
        slot: 0,
        number,
        hash: BlockHash::default(),
        epoch: 0,
        epoch_slot: 0,
        new_epoch: false,
        is_new_era: false,
        tip_slot: None,
        timestamp: 0,
        era: acropolis_common::Era::Conway,
    }
}

/// Collect validation responses and update the tree accordingly.
#[allow(clippy::too_many_arguments)]
async fn handle_validation(
    block_info: &BlockInfo,
    validator_topics: &[String],
    validator_subscriptions: &mut [Box<dyn caryatid_sdk::Subscription<Message>>],
    validation_timeout: Duration,
    tree: &mut ConsensusTree,
    event_queue: &EventQueue,
    context: &Arc<Context<Message>>,
    publish_blocks_topic: &str,
    publish_consensus_topic: &str,
    block_data: &mut HashMap<BlockHash, (BlockInfo, RawBlockMessage)>,
) {
    let completed_tasks = Arc::new(Mutex::new(
        HashMap::<String, Option<Arc<Message>>>::from_iter(
            validator_topics.iter().map(|s| (s.clone(), None)),
        ),
    ));

    let mut validation_results: Vec<(String, Arc<Message>)> = Vec::new();
    let all_say_go = match timeout(validation_timeout, async {
        for sub in validator_subscriptions.iter_mut() {
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
                Message::Cardano((_, CardanoMessage::BlockValidation(status))) => match status {
                    ValidationStatus::Go => all_ok,
                    ValidationStatus::NoGo(err) => {
                        error!(
                            block = block_info.number,
                            ?err,
                            "Validation failure: {topic}, result {msg:?}"
                        );
                        false
                    }
                },
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
        if let Err(e) = tree.mark_validated(block_info.hash) {
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
        if let Err(e) = tree.mark_rejected(block_info.hash) {
            error!("Failed to mark block rejected: {e}");
        }

        // Collect and publish events from mark_rejected
        let events = collect_observer_events(
            event_queue,
            publish_blocks_topic,
            publish_consensus_topic,
            block_data,
            tree,
        );
        publish_messages(context, events).await;
        prune_block_data(block_data, tree);
    }
}
