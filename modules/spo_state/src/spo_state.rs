//! Acropolis SPO state module for Caryatid
//! Accepts certificate events and derives the SPO state in memory

use acropolis_common::{
    messages::{
        CardanoMessage, Message, SnapshotDumpMessage, SnapshotMessage, SnapshotStateMessage,
        StateQuery, StateQueryResponse,
    },
    queries::pools::{PoolsList, PoolsListWithInfo, PoolsStateQuery, PoolsStateQueryResponse},
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod state;
use state::State;
mod rest;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.certificates";
const DEFAULT_SPO_STATE_TOPIC: &str = "cardano.spo.state";
const POOLS_STATE_TOPIC: &str = "pools-state";
/// SPO State module
#[module(
    message_type(Message),
    name = "spo-state",
    description = "In-memory SPO State from certificate events"
)]
pub struct SPOState;

impl SPOState {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let subscribe_topic =
            config.get_string("subscribe-topic").unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

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

        // Subscribe for certificate messages
        let mut subscription = context.subscribe(&subscribe_topic).await?;
        let context_subscribe = context.clone();
        let state_subscribe = state.clone();
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };

                match message.as_ref() {
                    Message::Cardano((block, CardanoMessage::TxCertificates(tx_certs_msg))) => {
                        let span = info_span!("spo_state.handle_tx_certs", block = block.number);
                        async {
                            let mut state = state_subscribe.lock().await;
                            let maybe_message = state
                                .handle_tx_certs(block, tx_certs_msg)
                                .inspect_err(|e| error!("Messaging handling error: {e}"))
                                .ok();

                            if let Some(Some(message)) = maybe_message {
                                context_subscribe
                                    .message_bus
                                    .publish(&spo_state_topic, message)
                                    .await
                                    .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                            }
                        }
                        .instrument(span)
                        .await;
                    }
                    _ => error!("Unexpected message type: {message:?}"),
                }
            }
        });

        // Ticker to log stats
        let state_tick = state.clone();
        let mut subscription = context.subscribe("clock.tick").await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        let span = info_span!("spo_state.tick", number = message.number);
                        async {
                            state_tick
                                .lock()
                                .await
                                .tick()
                                .await
                                .inspect_err(|e| error!("Tick error: {e}"))
                                .ok();
                        }
                        .instrument(span)
                        .await;
                    }
                }
            }
        });

        Ok(())
    }
}
