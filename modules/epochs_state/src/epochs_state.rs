//! Acropolis epochs state module for Caryatid
//! Unpacks block bodies to get transaction fees

use acropolis_common::configuration::StartupMethod;
use acropolis_common::messages::{EpochBootstrapMessage, SnapshotMessage, SnapshotStateMessage};
use acropolis_common::{
    caryatid::SubscriptionExt,
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse, StateTransitionMessage},
    queries::{
        epochs::{
            EpochsStateQuery, EpochsStateQueryResponse, LatestEpoch, DEFAULT_EPOCHS_QUERY_TOPIC,
        },
        errors::QueryError,
    },
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{message_bus::Subscription, module, Context};
use config::Config;
use pallas::ledger::traverse::MultiEraHeader;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, warn};

mod epoch_activity_publisher;
mod epoch_nonce_publisher;
mod state;
use crate::{
    epoch_activity_publisher::EpochActivityPublisher, epoch_nonce_publisher::EpochNoncePublisher,
};
use state::State;

const DEFAULT_BOOTSTRAPPED_SUBSCRIBE_TOPIC: (&str, &str) = (
    "bootstrapped-subscribe-topic",
    "cardano.sequence.bootstrapped",
);
const DEFAULT_BLOCK_SUBSCRIBE_TOPIC: (&str, &str) =
    ("block-subscribe-topic", "cardano.block.proposed");
const DEFAULT_BLOCK_TXS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("block-txs-subscribe-topic", "cardano.block.txs");
const DEFAULT_PROTOCOL_PARAMETERS_SUBSCRIBE_TOPIC: (&str, &str) = (
    "protocol-parameters-subscribe-topic",
    "cardano.protocol.parameters",
);
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

const DEFAULT_EPOCH_ACTIVITY_PUBLISH_TOPIC: (&str, &str) =
    ("epoch-activity-publish-topic", "cardano.epoch.activity");
const DEFAULT_EPOCH_NONCE_PUBLISH_TOPIC: (&str, &str) =
    ("epoch-nonce-publish-topic", "cardano.epoch.nonce");

/// Epochs State module
#[module(
    message_type(Message),
    name = "epochs-state",
    description = "Epochs state"
)]
pub struct EpochsState;

impl EpochsState {
    /// Handle bootstrap message from snapshot
    fn handle_bootstrap(state: &mut State, epoch_data: &EpochBootstrapMessage) {
        // Initialize epoch state from snapshot data
        state.bootstrap(epoch_data);

        info!(
            "Epoch state bootstrapped successfully for epoch {}",
            epoch_data.epoch
        );
    }

    /// Wait for and process snapshot bootstrap messages
    async fn wait_for_bootstrap(
        history: Arc<Mutex<StateHistory<State>>>,
        mut snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        genesis: &acropolis_common::genesis_values::GenesisValues,
    ) -> Result<()> {
        // Check we're subscribed to snapshot messages first
        let snapshot_subscription = match snapshot_subscription.as_mut() {
            Some(sub) => sub,
            None => {
                warn!("No snapshot subscription available, using default state");
                return Ok(());
            }
        };

        info!("Waiting for snapshot bootstrap messages...");

        loop {
            let (_, message) = snapshot_subscription.read().await?;

            match message.as_ref() {
                Message::Snapshot(SnapshotMessage::Startup) => {
                    info!("Received snapshot startup signal, awaiting bootstrap data...");
                }
                Message::Snapshot(SnapshotMessage::Bootstrap(
                    SnapshotStateMessage::EpochState(epoch_data),
                )) => {
                    let mut state = history.lock().await.get_or_init_with(|| State::new(genesis));
                    Self::handle_bootstrap(&mut state, epoch_data);
                    history.lock().await.commit(epoch_data.epoch, state);
                    info!("Epoch state bootstrap complete");
                }
                Message::Snapshot(SnapshotMessage::Complete) => {
                    info!("Snapshot complete, exiting bootstrap loop");
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    /// Run loop
    #[allow(clippy::too_many_arguments)]
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut bootstrapped_subscription: Box<dyn Subscription<Message>>,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        mut block_subscription: Box<dyn Subscription<Message>>,
        mut block_txs_subscription: Box<dyn Subscription<Message>>,
        mut protocol_parameters_subscription: Box<dyn Subscription<Message>>,
        mut epoch_activity_publisher: EpochActivityPublisher,
        mut epoch_nonce_publisher: EpochNoncePublisher,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        let (_, bootstrapped_message) = bootstrapped_subscription.read().await?;
        let genesis = match bootstrapped_message.as_ref() {
            Message::Cardano((_, CardanoMessage::GenesisComplete(complete))) => {
                complete.values.clone()
            }
            _ => panic!("Unexpected message in genesis completion topic: {bootstrapped_message:?}"),
        };

        // Wait for the snapshot bootstrap (if available)
        Self::wait_for_bootstrap(history.clone(), snapshot_subscription, &genesis).await?;

        // Consume initial protocol parameters (only needed for genesis bootstrap)
        if !is_snapshot_mode {
            let _ = protocol_parameters_subscription.read().await?;
        }

        loop {
            // Get a mutable state
            let mut state = history.lock().await.get_or_init_with(|| State::new(&genesis));
            let mut current_block: Option<BlockInfo> = None;

            // Handle blocks first
            let (_, message) = block_subscription.read().await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::BlockAvailable(block_msg))) => {
                    // handle rollback here
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(block_info.number);
                    }
                    current_block = Some(block_info.clone());
                    let is_new_epoch = block_info.new_epoch && block_info.epoch > 0;

                    // read protocol parameters if new epoch
                    if is_new_epoch {
                        let (_, protocol_parameters_msg) =
                            protocol_parameters_subscription.read_ignoring_rollbacks().await?;
                        if let Message::Cardano((_, CardanoMessage::ProtocolParams(params))) =
                            protocol_parameters_msg.as_ref()
                        {
                            state.handle_protocol_parameters(params);
                        }

                        let ea = state.end_epoch(block_info);
                        // publish epoch activity message
                        epoch_activity_publisher.publish(block_info, ea).await.unwrap_or_else(
                            |e| error!("Failed to publish epoch activity messages: {e}"),
                        );
                    }

                    let span = info_span!("epochs_state.decode_header", block = block_info.number);
                    let mut header = None;
                    span.in_scope(|| {
                        header = match MultiEraHeader::decode(
                            block_info.era as u8,
                            None,
                            &block_msg.header,
                        ) {
                            Ok(header) => Some(header),
                            Err(e) => {
                                error!("Can't decode header {}: {e}", block_info.slot);
                                None
                            }
                        };
                    });

                    let span = info_span!("epochs_state.evolve_nonces", block = block_info.number);
                    span.in_scope(|| {
                        if let Some(header) = header.as_ref() {
                            if let Err(e) = state.evolve_nonces(&genesis, block_info, header) {
                                error!("Error handling block header: {e}");
                            }
                        }
                    });

                    // At the beginning of epoch, publish the newly evolved active nonce
                    // for that epoch
                    if is_new_epoch {
                        let active_nonce = state.get_active_nonce();
                        epoch_nonce_publisher
                            .publish(block_info, active_nonce)
                            .await
                            .unwrap_or_else(|e| {
                                error!("Failed to publish epoch nonce messages: {e}")
                            });
                    }

                    let span = info_span!("epochs_state.handle_mint", block = block_info.number);
                    span.in_scope(|| {
                        if let Some(header) = header.as_ref() {
                            state.handle_mint(block_info, header.issuer_vkey());
                        }
                    });
                }

                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                )) => {
                    // publish epoch activity rollback message
                    epoch_activity_publisher.publish_rollback(message).await.unwrap_or_else(|e| {
                        error!("Failed to publish epoch activity rollback: {e}")
                    });
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            // Handle block txs second so new epoch's state don't get counted in the last one
            let (_, message) = block_txs_subscription.read_ignoring_rollbacks().await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::BlockInfoMessage(txs_msg))) => {
                    let span =
                        info_span!("epochs_state.handle_block_txs", block = block_info.number);
                    span.in_scope(|| {
                        Self::check_sync(&current_block, block_info);
                        state.handle_block_txs(block_info, txs_msg);
                    });
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            // Commit the new state
            if let Some(block_info) = current_block {
                history.lock().await.commit(block_info.number, state);
            }
        }
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Subscription topics
        let bootstrapped_subscribe_topic = config
            .get_string(DEFAULT_BOOTSTRAPPED_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_BOOTSTRAPPED_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber for bootstrapped on '{bootstrapped_subscribe_topic}'");

        let block_subscribe_topic = config
            .get_string(DEFAULT_BLOCK_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_BLOCK_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber for blocks on '{block_subscribe_topic}'");

        let block_txs_subscribe_topic = config
            .get_string(DEFAULT_BLOCK_TXS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_BLOCK_TXS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber for block txs on '{block_txs_subscribe_topic}'");

        let protocol_parameters_subscribe_topic = config
            .get_string(DEFAULT_PROTOCOL_PARAMETERS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_PROTOCOL_PARAMETERS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber for protocol parameters on '{protocol_parameters_subscribe_topic}'");

        let snapshot_subscribe_topic = config
            .get_string(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber for snapshot on '{snapshot_subscribe_topic}'");

        // Publish topic
        let epoch_activity_publish_topic = config
            .get_string(DEFAULT_EPOCH_ACTIVITY_PUBLISH_TOPIC.0)
            .unwrap_or(DEFAULT_EPOCH_ACTIVITY_PUBLISH_TOPIC.1.to_string());
        info!("Publishing EpochActivityMessage on '{epoch_activity_publish_topic}'");

        let epoch_nonce_publish_topic = config
            .get_string(DEFAULT_EPOCH_NONCE_PUBLISH_TOPIC.0)
            .unwrap_or(DEFAULT_EPOCH_NONCE_PUBLISH_TOPIC.1.to_string());
        info!("Publishing EpochNonceMessage on '{epoch_nonce_publish_topic}'");

        // query topic
        let epochs_query_topic = config
            .get_string(DEFAULT_EPOCHS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_EPOCHS_QUERY_TOPIC.1.to_string());
        info!("Creating query handler on '{}'", epochs_query_topic);

        // state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "epochs_state",
            StateHistoryStore::default_block_store(),
        )));
        let history_query = history.clone();

        // Subscribe
        let bootstrapped_subscription = context.subscribe(&bootstrapped_subscribe_topic).await?;
        let block_subscription = context.subscribe(&block_subscribe_topic).await?;
        let protocol_parameters_subscription =
            context.subscribe(&protocol_parameters_subscribe_topic).await?;
        let block_txs_subscription = context.subscribe(&block_txs_subscribe_topic).await?;

        // Only subscribe to Snapshot if we're using Snapshot to start-up
        let is_snapshot_mode = StartupMethod::from_config(config.as_ref()).is_snapshot();
        let snapshot_subscription = if is_snapshot_mode {
            Some(context.subscribe(&snapshot_subscribe_topic).await?)
        } else {
            None
        };

        // Publisher
        let epoch_activity_publisher =
            EpochActivityPublisher::new(context.clone(), epoch_activity_publish_topic);
        let epoch_nonce_publisher =
            EpochNoncePublisher::new(context.clone(), epoch_nonce_publish_topic);

        // handle epochs query
        context.handle(&epochs_query_topic, move |message| {
            let history = history_query.clone();

            async move {
                let Message::StateQuery(StateQuery::Epochs(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Epochs(
                        EpochsStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for epochs-state",
                        )),
                    )));
                };

                let state = history.lock().await.get_current_state();
                let response = match query {
                    EpochsStateQuery::GetLatestEpoch => {
                        EpochsStateQueryResponse::LatestEpoch(LatestEpoch {
                            epoch: state.get_epoch_info(),
                        })
                    }

                    EpochsStateQuery::GetLatestEpochBlocksMintedByPool { spo_id } => {
                        EpochsStateQueryResponse::LatestEpochBlocksMintedByPool(
                            state.get_latest_epoch_blocks_minted_by_pool(spo_id),
                        )
                    }

                    _ => EpochsStateQueryResponse::Error(QueryError::not_implemented(format!(
                        "Unimplemented query variant: {query:?}"
                    ))),
                };
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Epochs(
                    response,
                )))
            }
        });

        // Start the run task
        context.run(async move {
            Self::run(
                history,
                bootstrapped_subscription,
                snapshot_subscription,
                block_subscription,
                block_txs_subscription,
                protocol_parameters_subscription,
                epoch_activity_publisher,
                epoch_nonce_publisher,
                is_snapshot_mode,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }

    /// Check for synchronisation
    fn check_sync(expected: &Option<BlockInfo>, actual: &BlockInfo) {
        if let Some(ref block) = expected {
            if block.number != actual.number {
                error!(
                    expected = block.number,
                    actual = actual.number,
                    "Messages out of sync"
                );
            }
        }
    }
}
