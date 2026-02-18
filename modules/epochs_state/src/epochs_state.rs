//! Acropolis epochs state module for Caryatid
//! Unpacks block bodies to get transaction fees

use acropolis_common::{
    caryatid::{RollbackWrapper, ValidationContext},
    configuration::StartupMode,
    declare_cardano_reader,
    messages::{
        BlockTxsMessage, CardanoMessage, EpochBootstrapMessage, GenesisCompleteMessage, Message,
        ProtocolParamsMessage, RawBlockMessage, SnapshotMessage, SnapshotStateMessage, StateQuery,
        StateQueryResponse, StateTransitionMessage,
    },
    queries::{
        epochs::{
            EpochsStateQuery, EpochsStateQueryResponse, LatestEpoch, DEFAULT_EPOCHS_QUERY_TOPIC,
        },
        errors::QueryError,
    },
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus,
};
use anyhow::{anyhow, bail, Result};
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

declare_cardano_reader!(
    BootstrapReader,
    "bootstrapped-subscribe-topic",
    "cardano.sequence.bootstrapped",
    GenesisComplete,
    GenesisCompleteMessage
);
declare_cardano_reader!(
    BlockReader,
    "block-subscribe-topic",
    "cardano.block.proposed",
    BlockAvailable,
    RawBlockMessage
);
declare_cardano_reader!(
    TxsReader,
    "block-txs-subscribe-topic",
    "cardano.block.txs",
    BlockInfoMessage,
    BlockTxsMessage
);
declare_cardano_reader!(
    ParamsReader,
    "protocol-parameters-subscribe-topic",
    "cardano.protocol.parameters",
    ProtocolParams,
    ProtocolParamsMessage
);

const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

const DEFAULT_EPOCH_ACTIVITY_PUBLISH_TOPIC: (&str, &str) =
    ("epoch-activity-publish-topic", "cardano.epoch.activity");
const DEFAULT_EPOCH_NONCE_PUBLISH_TOPIC: (&str, &str) =
    ("epoch-nonce-publish-topic", "cardano.epoch.nonce");
const DEFAULT_VALIDATION_OUTCOME_PUBLISH_TOPIC: (&str, &str) =
    ("validation-publish-topic", "cardano.validation.epochs");

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
                    let block_number = epoch_data.last_block_height;
                    let mut state = history.lock().await.get_or_init_with(|| State::new(genesis));
                    Self::handle_bootstrap(&mut state, epoch_data);
                    history.lock().await.bootstrap_init_with(state, block_number);
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
        context: Arc<Context<Message>>,
        mut bootstrap_reader: BootstrapReader, //mut bootstrapped_subscription: Box<dyn Subscription<Message>>,
        mut params_reader: ParamsReader,
        mut block_reader: BlockReader,
        mut txs_reader: TxsReader,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        mut epoch_activity_publisher: EpochActivityPublisher,
        mut epoch_nonce_publisher: EpochNoncePublisher,
        validation_topic: String,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        let (_, genesis) = bootstrap_reader.read_skip_rollbacks().await?;

        // Wait for the snapshot bootstrap (if available)
        Self::wait_for_bootstrap(history.clone(), snapshot_subscription, &genesis.values).await?;

        // Consume initial protocol parameters and block txs published at epoch 0.
        // These messages are published when the first new_epoch block flows through the
        // pipeline (governance_state → parameters_state, utxo_state → block_txs).
        // Without consuming them here, they desync the readers in the main loop
        // (epoch N boundary reads epoch N-1 params instead of epoch N params).
        if !is_snapshot_mode {
            let (_, initial_params) = params_reader.read_skip_rollbacks().await?;
            let mut state = history.lock().await.get_or_init_with(|| State::new(&genesis.values));
            state.handle_protocol_parameters(&initial_params);
            history.lock().await.commit(0, state);
            let _ = txs_reader.read_skip_rollbacks().await?;
        }

        loop {
            let mut ctx = ValidationContext::new(&context, &validation_topic);

            // Get a mutable state
            let mut state = history.lock().await.get_or_init_with(|| State::new(&genesis.values));

            match ctx.consume_sync(block_reader.read_with_rollbacks().await)? {
                RollbackWrapper::Normal((blk_info, blk_msg)) => {
                    if blk_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(blk_info.number);
                    }
                    let is_new_epoch = blk_info.new_epoch && blk_info.epoch > 0;

                    if is_new_epoch {
                        if let Some((_, params)) = ctx
                            .consume("protocol params", params_reader.read_skip_rollbacks().await)
                        {
                            state.handle_protocol_parameters(&params);
                        }

                        let ea = state.end_epoch(&blk_info);
                        // publish epoch activity message
                        ctx.handle(
                            "publish epoch activity",
                            epoch_activity_publisher.publish(&blk_info, ea).await,
                        );
                    }

                    let header = ctx.handle(
                        "epochs_state.decode_header",
                        match MultiEraHeader::decode(blk_info.era as u8, None, &blk_msg.header) {
                            Err(e) => Err(anyhow!("Can't decode header {}: {e}", blk_info.slot)),
                            Ok(res) => Ok(Some(res)),
                        },
                    );

                    let span = info_span!("epochs_state.evolve_nonces", block = blk_info.number);
                    span.in_scope(|| {
                        if let Some(header) = header.as_ref() {
                            ctx.handle(
                                "evolve_nonces",
                                state.evolve_nonces(&genesis.values, &blk_info, header),
                            )
                        }
                    });

                    // At the beginning of epoch, publish the newly evolved active nonce
                    // for that epoch
                    if is_new_epoch {
                        let active_nonce = state.get_active_nonce();
                        ctx.handle(
                            "publish epoch nonce",
                            epoch_nonce_publisher.publish(&blk_info, active_nonce).await,
                        );
                    }

                    let span = info_span!("epochs_state.handle_mint", block = blk_info.number);
                    span.in_scope(|| {
                        if let Some(header) = header.as_ref() {
                            state.handle_mint(
                                &blk_info,
                                header.issuer_vkey(),
                                header.as_byron().is_some(),
                            );
                        }
                    });
                }
                RollbackWrapper::Rollback(raw_message) => {
                    ctx.handle(
                        "publishing rollback message",
                        epoch_activity_publisher.publish_rollback(raw_message).await,
                    );
                }
            }

            // Handle block txs second so new epoch's state don't get counted in the last one
            if let Some((blk_info, msg)) =
                ctx.consume("block_txs", txs_reader.read_skip_rollbacks().await)
            {
                let span = info_span!("epochs_state.handle_block_txs", block = blk_info.number);
                span.in_scope(|| state.handle_block_txs(&blk_info, &msg));
            }

            // Commit the new state
            if let Some(block_info) = ctx.get_current_block_opt() {
                history.lock().await.commit(block_info.number, state);
            }

            ctx.publish("epochs_state").await;
        }
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Subscription topics
        let bootstrap_reader = BootstrapReader::new(&context, &config).await?;
        let params_reader = ParamsReader::new(&context, &config).await?;
        let txs_reader = TxsReader::new(&context, &config).await?;
        let block_reader = BlockReader::new(&context, &config).await?;

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

        let validation_outcome_topic = config
            .get_string(DEFAULT_VALIDATION_OUTCOME_PUBLISH_TOPIC.0)
            .unwrap_or(DEFAULT_VALIDATION_OUTCOME_PUBLISH_TOPIC.1.to_string());
        info!("Publishing validation outcomes on '{validation_outcome_topic}'");

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

        // Only subscribe to Snapshot if we're using Snapshot to start-up
        let is_snapshot_mode = StartupMode::from_config(config.as_ref()).is_snapshot();
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

        let context_clone = context.clone();

        // Start the run task
        context.run(async move {
            Self::run(
                history,
                context_clone,
                bootstrap_reader,
                params_reader,
                block_reader,
                txs_reader,
                snapshot_subscription,
                epoch_activity_publisher,
                epoch_nonce_publisher,
                validation_outcome_topic,
                is_snapshot_mode,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
