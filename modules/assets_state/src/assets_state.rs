//! Acropolis Asset State module for Caryatid
//! Accepts native asset mint and burn events a
//! and derives the Asset State in memory

use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::assets::{AssetsStateQueryResponse, DEFAULT_ASSETS_QUERY_TOPIC},
    state_history::{StateHistory, StateHistoryStore},
    BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

use crate::state::{AssetsStorageConfig, State};
mod state;

// Subscription topics
const DEFAULT_ASSET_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("asset-deltas-subscribe-topic", "cardano.asset.deltas");

// Configuration defaults
const DEFAULT_STORE_INFO: (&str, bool) = ("store-info", false);
const DEFAULT_STORE_HISTORY: (&str, bool) = ("store-history", false);
const DEFAULT_STORE_TRANSACTIONS: (&str, bool) = ("store-transactions", false);
const DEFAULT_STORE_ADDRESSES: (&str, bool) = ("store-addresses", false);

/// Assets State module
#[module(
    message_type(Message),
    name = "assets-state",
    description = "In-memory Assets State from asset mint and burn events"
)]
pub struct AssetsState;

impl AssetsState {
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut deltas_subscription: Box<dyn Subscription<Message>>,
        storage_config: AssetsStorageConfig,
    ) -> Result<()> {
        // Main loop of synchronised messages
        loop {
            match deltas_subscription.read().await?.1.as_ref() {
                Message::Cardano((block, CardanoMessage::AssetDeltas(message))) => {
                    let span = info_span!("assets_state.handle", epoch = block.epoch);
                    async {
                        // Get current state and current params
                        let mut state = {
                            let mut h = history.lock().await;
                            h.get_or_init_with(|| State::new(&storage_config))
                        };

                        // Handle rollback if needed
                        if block.status == BlockStatus::RolledBack {
                            state = history.lock().await.get_rolled_back_state(block.epoch);
                        }

                        // Process deltas
                        state
                            .handle_deltas(&message.deltas)
                            .inspect_err(|e| error!("Asset deltas handling error: {e:#}"))
                            .ok();

                        // Commit state
                        {
                            let mut h = history.lock().await;
                            h.commit(block.epoch, state);
                        }

                        Ok::<(), anyhow::Error>(())
                    }
                    .instrument(span)
                    .await?;
                }
                msg => error!("Unexpected message {msg:?} for enact state topic"),
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        fn get_bool_flag(config: &Config, key: (&str, bool)) -> bool {
            config.get_bool(key.0).unwrap_or(key.1)
        }

        fn get_string_flag(config: &Config, key: (&str, &str)) -> String {
            config.get_string(key.0).unwrap_or_else(|_| key.1.to_string())
        }

        // Get configuration flags and topis
        let storage_config = AssetsStorageConfig {
            _store_info: get_bool_flag(&config, DEFAULT_STORE_INFO),
            _store_history: get_bool_flag(&config, DEFAULT_STORE_HISTORY),
            _store_transactions: get_bool_flag(&config, DEFAULT_STORE_TRANSACTIONS),
            _store_addresses: get_bool_flag(&config, DEFAULT_STORE_ADDRESSES),
        };

        let asset_deltas_subscribe_topic =
            get_string_flag(&config, DEFAULT_ASSET_DELTAS_SUBSCRIBE_TOPIC);
        info!("Creating subscriber on '{asset_deltas_subscribe_topic}'");

        let assets_query_topic = get_string_flag(&config, DEFAULT_ASSETS_QUERY_TOPIC);
        info!("Creating DRep query handler on '{assets_query_topic}'");

        // Initalize state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "DRepState",
            StateHistoryStore::default_block_store(),
        )));
        let history_run = history.clone();
        let query_history = history.clone();
        let ticker_history = history.clone();

        // Query handler
        context.handle(&assets_query_topic, move |message| {
            let history = query_history.clone();
            async move {
                let Message::StateQuery(StateQuery::Assets(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Assets(
                        AssetsStateQueryResponse::Error("Invalid message for assets-state".into()),
                    )));
                };

                let _locked = history.lock().await;

                let response = match query {
                    _ => AssetsStateQueryResponse::Error(format!(
                        "Unimplemented assets query: {query:?}"
                    )),
                };
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Assets(
                    response,
                )))
            }
        });

        // Ticker to log stats
        let mut subscription = context.subscribe("clock.tick").await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        let span = info_span!("assets_state.tick", number = message.number);
                        async {
                            ticker_history
                                .lock()
                                .await
                                .get_current_state()
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

        // Subscribe to enabled topics
        let deltas_sub = context.subscribe(&asset_deltas_subscribe_topic).await?;

        // Start run task
        context.run(async move {
            Self::run(history_run, deltas_sub, storage_config)
                .await
                .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
