//! Acropolis SPO state module for Caryatid
//! Accepts certificate events and derives the SPO state in memory

use acropolis_common::{
    messages::{
        CardanoMessage, Message, SnapshotDumpMessage, SnapshotMessage, SnapshotStateMessage,
        StateQuery, StateQueryResponse,
    },
    queries::pools::{
        PoolsActiveStakes, PoolsList, PoolsListWithInfo, PoolsMetadataExtended, PoolsStateQuery,
        PoolsStateQueryResponse, PoolsTotalBlocksMinted,
    },
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod state;
use state::State;
mod metadata;
mod rest;
mod spo_state_publisher;
use crate::spo_state_publisher::SPOStatePublisher;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.certificates";
const DEFAULT_CLOCK_TICK_TOPIC: &str = "clock.tick";
const DEFAULT_SPO_STATE_TOPIC: &str = "cardano.spo.state";
const DEFAULT_SPDD_SUBSCRIBE_TOPIC: &str = "cardano.spo.distribution";
const DEFAULT_EPOCH_ACTIVITY_TOPIC: &str = "cardano.epoch.activity";

const POOLS_STATE_TOPIC: &str = "pools-state";
/// SPO State module
#[module(
    message_type(Message),
    name = "spo-state",
    description = "In-memory SPO State from certificate events"
)]
pub struct SPOState;

impl SPOState {
    /// Async run loop for certificate messages
    async fn run_certs_subscription(
        state: Arc<Mutex<State>>,
        mut spo_state_publisher: SPOStatePublisher,
        mut certs_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        loop {
            // Subscribe to certificate messages
            let (_, certs_message) = certs_subscription.read().await?;
            match certs_message.as_ref() {
                Message::Cardano((block, CardanoMessage::TxCertificates(tx_certs_msg))) => {
                    let span = info_span!("spo_state.handle_tx_certs", block = block.number);
                    async {
                        let mut state = state.lock().await;
                        let maybe_message = state
                            .handle_tx_certs(block, tx_certs_msg)
                            .inspect_err(|e| error!("Messaging handling error: {e}"));

                        if let Ok((maybe_message, pools_metadata)) = maybe_message {
                            if let Some(message) = maybe_message {
                                if let Err(e) = spo_state_publisher.publish(message).await {
                                    error!("Error publishing SPO State: {e:#}")
                                }
                            }

                            state.handle_pools_metadata(pools_metadata).await;
                        }
                    }
                    .instrument(span)
                    .await;
                }
                _ => error!("Unexpected message type: {certs_message:?}"),
            }
        }
    }

    /// Async run loop for clock tick messages
    async fn run_clock_tick_subscription(
        state: Arc<Mutex<State>>,
        mut clock_tick_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        loop {
            // Subscribe to clock tick messages
            let (_, tick_message) = clock_tick_subscription.read().await?;
            if let Message::Clock(tick_message) = tick_message.as_ref() {
                if (tick_message.number % 60) == 0 {
                    let span = info_span!("spo_state.tick", number = tick_message.number);
                    async {
                        let state = state.lock().await;
                        state.tick().await.inspect_err(|e| error!("Tick error: {e}")).ok();
                    }
                    .instrument(span)
                    .await;
                }
            }
        }
    }

    /// Async run loop for SPDD messages
    async fn run_spdd_subscription(
        state: Arc<Mutex<State>>,
        mut spdd_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        loop {
            // Subscribe to accounts-state's SPDD messsages
            let (_, spdd_message) = spdd_subscription.read().await?;
            if let Message::Cardano((
                block_info,
                CardanoMessage::SPOStakeDistribution(spdd_message),
            )) = spdd_message.as_ref()
            {
                let mut state = state.lock().await;
                state.handle_spdd(block_info, spdd_message)
            }
        }
    }

    /// Async run loop for epoch activity messages
    async fn run_epoch_activity_subscription(
        state: Arc<Mutex<State>>,
        mut epoch_activity_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        loop {
            // Subscribe to accounts-state's SPDD messsages
            let (_, spdd_message) = epoch_activity_subscription.read().await?;
            if let Message::Cardano((
                block_info,
                CardanoMessage::EpochActivity(epoch_activity_message),
            )) = spdd_message.as_ref()
            {
                let mut state = state.lock().await;
                state.handle_epoch_activity(block_info, epoch_activity_message)
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let subscribe_topic =
            config.get_string("subscribe-topic").unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let clock_tick_topic =
            config.get_string("clock-tick-topic").unwrap_or(DEFAULT_CLOCK_TICK_TOPIC.to_string());
        info!("Creating subscriber on '{clock_tick_topic}'");

        let spdd_topic =
            config.get_string("spdd-topic").unwrap_or(DEFAULT_SPDD_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{spdd_topic}'");

        let epoch_activity_topic = config
            .get_string("epoch-activity-topic")
            .unwrap_or(DEFAULT_EPOCH_ACTIVITY_TOPIC.to_string());
        info!("Creating subscriber on '{epoch_activity_topic}'");

        let maybe_snapshot_topic = config
            .get_string("snapshot-topic")
            .ok()
            .inspect(|snapshot_topic| info!("Creating subscriber on '{snapshot_topic}'"));

        let spo_state_topic = config
            .get_string("publish-spo-state-topic")
            .unwrap_or(DEFAULT_SPO_STATE_TOPIC.to_string());
        info!("Creating SPO state publisher on '{spo_state_topic}'");

        let state = Arc::new(Mutex::new(State::new()));

        // handle pools-state
        let state_rest_blockfrost = state.clone();
        context.handle(POOLS_STATE_TOPIC, move |message| {
            let state = state_rest_blockfrost.clone();
            async move {
                let Message::StateQuery(StateQuery::Pools(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Pools(
                        PoolsStateQueryResponse::Error("Invalid message for pools-state".into()),
                    )));
                };

                let guard = state.lock().await;

                let response = match query {
                    PoolsStateQuery::GetPoolsList => {
                        let pools_list = PoolsList {
                            pool_operators: guard.list_pool_operators(),
                        };
                        PoolsStateQueryResponse::PoolsList(pools_list)
                    }
                    PoolsStateQuery::GetPoolsListWithInfo => {
                        let pools_list_with_info = PoolsListWithInfo {
                            pools: guard.list_pools_with_info(),
                        };
                        PoolsStateQueryResponse::PoolsListWithInfo(pools_list_with_info)
                    }
                    PoolsStateQuery::GetPoolsMetadataExtended { pools_operators } => {
                        let pools_metadata_extended = PoolsMetadataExtended {
                            pools_metadata_extended: guard
                                .list_pools_metadata_extended(pools_operators),
                        };
                        PoolsStateQueryResponse::PoolsMetadataExtended(pools_metadata_extended)
                    }
                    PoolsStateQuery::GetPoolsActiveStakes {
                        pools_operators,
                        epoch,
                    } => {
                        if let Some((active_stakes, total_active_stake)) =
                            guard.get_pools_active_stakes(pools_operators, *epoch)
                        {
                            PoolsStateQueryResponse::PoolsActiveStakes(PoolsActiveStakes {
                                active_stakes,
                                total_active_stake,
                            })
                        } else {
                            PoolsStateQueryResponse::PoolsActiveStakes(PoolsActiveStakes {
                                active_stakes: vec![0; pools_operators.len()],
                                total_active_stake: 0,
                            })
                        }
                    }

                    PoolsStateQuery::GetPoolsTotalBlocksMinted { vrf_key_hashes } => {
                        if let Some(total_blocks_minted) =
                            guard.get_total_blocks_minted(vrf_key_hashes)
                        {
                            PoolsStateQueryResponse::PoolsTotalBlocksMinted(
                                PoolsTotalBlocksMinted {
                                    total_blocks_minted,
                                },
                            )
                        } else {
                            PoolsStateQueryResponse::PoolsTotalBlocksMinted(
                                PoolsTotalBlocksMinted {
                                    total_blocks_minted: vec![0; vrf_key_hashes.len()],
                                },
                            )
                        }
                    }

                    _ => PoolsStateQueryResponse::Error(format!(
                        "Unimplemented query variant: {:?}",
                        query
                    )),
                };

                Arc::new(Message::StateQueryResponse(StateQueryResponse::Pools(
                    response,
                )))
            }
        });

        // Subscribe for snapshot messages, if allowed
        if let Some(snapshot_topic) = maybe_snapshot_topic {
            let mut subscription = context.subscribe(&snapshot_topic).await?;
            let context_snapshot = context.clone();
            let state_snapshot = state.clone();
            context.run(async move {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };

                match message.as_ref() {
                    Message::Snapshot(SnapshotMessage::Bootstrap(
                        SnapshotStateMessage::SPOState(spo_state),
                    )) => {
                        let mut state = state_snapshot.lock().await;
                        state.bootstrap(spo_state.clone());
                    }
                    Message::Snapshot(SnapshotMessage::DumpRequest(SnapshotDumpMessage {
                        block_height,
                    })) => {
                        info!("inspecting state at block height {}", block_height);
                        let state = state_snapshot.lock().await;
                        let maybe_spo_state = state.dump(*block_height);

                        if let Some(spo_state) = maybe_spo_state {
                            context_snapshot
                                .message_bus
                                .publish(
                                    &snapshot_topic,
                                    Arc::new(Message::Snapshot(SnapshotMessage::Dump(
                                        SnapshotStateMessage::SPOState(spo_state),
                                    ))),
                                )
                                .await
                                .unwrap_or_else(|e| error!("failed to publish snapshot dump: {e}"))
                        }
                    }
                    _ => error!("Unexpected message type: {message:?}"),
                }
            });
        }

        // Publishers
        let spo_state_publisher = SPOStatePublisher::new(context.clone(), spo_state_topic);

        // Subscriptions
        let certs_subscription = context.subscribe(&subscribe_topic).await?;
        let clock_tick_subscription = context.subscribe(&clock_tick_topic).await?;
        let spdd_subscription = context.subscribe(&spdd_topic).await?;
        let epoch_activity_subscription = context.subscribe(&epoch_activity_topic).await?;

        // Start run task
        let run_certs_state = state.clone();
        let run_clock_tick_state = state.clone();
        let run_spdd_state = state.clone();
        let run_epoch_activity_state = state.clone();

        context.run(async move {
            Self::run_certs_subscription(run_certs_state, spo_state_publisher, certs_subscription)
                .await
                .unwrap_or_else(|e| error!("Failed to run SPO Certs Subscription: {e}"));
        });

        context.run(async move {
            Self::run_clock_tick_subscription(run_clock_tick_state, clock_tick_subscription)
                .await
                .unwrap_or_else(|e| error!("Failed to run SPO Clock Tick Subscription: {e}"));
        });

        context.run(async move {
            Self::run_spdd_subscription(run_spdd_state, spdd_subscription)
                .await
                .unwrap_or_else(|e| error!("Failed to run SPO SPDD Subscription: {e}"));
        });

        context.run(async move {
            Self::run_epoch_activity_subscription(
                run_epoch_activity_state,
                epoch_activity_subscription,
            )
            .await
            .unwrap_or_else(|e| error!("Failed to run SPO Epoch Activity Subscription: {e}"));
        });

        Ok(())
    }
}
