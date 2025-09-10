//! Acropolis Asset State module for Caryatid
//! Accepts native asset mint and burn events
//! and derives the Asset State in memory

use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::assets::{AssetsStateQuery, AssetsStateQueryResponse, DEFAULT_ASSETS_QUERY_TOPIC},
    state_history::{StateHistory, StateHistoryStore},
    BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

use crate::{
    asset_registry::AssetRegistry,
    state::{AssetsStorageConfig, State},
};
pub mod asset_registry;
mod state;

// Subscription topics
const DEFAULT_ASSET_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("asset-deltas-subscribe-topic", "cardano.asset.deltas");

// Configuration defaults
const DEFAULT_STORE_ASSETS: (&str, bool) = ("store-assets", false);
const DEFAULT_STORE_INFO: (&str, bool) = ("store-info", false);
const DEFAULT_STORE_HISTORY: (&str, bool) = ("store-history", false);
const DEFAULT_STORE_TRANSACTIONS: (&str, bool) = ("store-transactions", false);
const DEFAULT_STORE_ADDRESSES: (&str, bool) = ("store-addresses", false);
const DEFAULT_INDEX_BY_POLICY: (&str, bool) = ("index-by-policy", false);
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
        registry: Arc<Mutex<AssetRegistry>>,
    ) -> Result<()> {
        // Main loop of synchronised messages
        loop {
            match deltas_subscription.read().await?.1.as_ref() {
                Message::Cardano((block, CardanoMessage::AssetDeltas(message))) => {
                    let span = info_span!("assets_state.handle", number = block.number);
                    async {
                        // Get current state and current params
                        let mut state = {
                            let mut h = history.lock().await;
                            h.get_or_init_with(|| State::new(storage_config.clone()))
                        };

                        // Handle rollback if needed
                        if block.status == BlockStatus::RolledBack {
                            state = history.lock().await.get_rolled_back_state(block.number);
                        }

                        // Process mint deltas
                        if storage_config.store_assets {
                            let mut reg = registry.lock().await;
                            state = match state.handle_mint_deltas(&message.deltas, &mut *reg) {
                                Ok(new_state) => new_state,
                                Err(e) => {
                                    error!("Asset deltas handling error: {e:#}");
                                    state
                                }
                            };
                        }

                        // Commit state
                        {
                            let mut h = history.lock().await;
                            h.commit(block.number, state);
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
            store_assets: get_bool_flag(&config, DEFAULT_STORE_ASSETS),
            store_info: get_bool_flag(&config, DEFAULT_STORE_INFO),
            store_history: get_bool_flag(&config, DEFAULT_STORE_HISTORY),
            store_transactions: get_bool_flag(&config, DEFAULT_STORE_TRANSACTIONS),
            store_addresses: get_bool_flag(&config, DEFAULT_STORE_ADDRESSES),
            index_by_policy: get_bool_flag(&config, DEFAULT_INDEX_BY_POLICY),
        };

        let asset_deltas_subscribe_topic =
            get_string_flag(&config, DEFAULT_ASSET_DELTAS_SUBSCRIBE_TOPIC);
        info!("Creating subscriber on '{asset_deltas_subscribe_topic}'");

        let assets_query_topic = get_string_flag(&config, DEFAULT_ASSETS_QUERY_TOPIC);
        info!("Creating asset query handler on '{assets_query_topic}'");

        // Initalize state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "AssetsState",
            StateHistoryStore::default_block_store(),
        )));
        let history_run = history.clone();
        let query_history = history.clone();
        let tick_history = history.clone();

        // Initialize asset registry
        let registry = Arc::new(Mutex::new(asset_registry::AssetRegistry::new()));
        let registry_run = registry.clone();
        let query_registry = registry.clone();

        // Query handler
        context.handle(&assets_query_topic, move |message| {
            let history = query_history.clone();
            let registry = query_registry.clone();
            async move {
                let Message::StateQuery(StateQuery::Assets(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Assets(
                        AssetsStateQueryResponse::Error("Invalid message for assets-state".into()),
                    )));
                };

                let state = history.lock().await.get_current_state();
                let reg = registry.lock().await;

                let response = match query {
                    AssetsStateQuery::GetAssetsList => match state.get_assets_list(&*reg) {
                        Ok(list) => AssetsStateQueryResponse::AssetsList(list),
                        Err(e) => AssetsStateQueryResponse::Error(e.to_string()),
                    },
                    AssetsStateQuery::GetAssetInfo { policy, name } => {
                        match reg.lookup_id(&policy, &name) {
                            Some(asset_id) => match state.get_asset_info(&asset_id) {
                                Ok(Some(info)) => AssetsStateQueryResponse::AssetInfo(info),
                                Ok(None) => AssetsStateQueryResponse::NotFound,
                                Err(e) => AssetsStateQueryResponse::Error(e.to_string()),
                            },
                            None => AssetsStateQueryResponse::NotFound,
                        }
                    }
                    AssetsStateQuery::GetAssetHistory { policy, name } => {
                        match reg.lookup_id(&policy, &name) {
                            Some(asset_id) => match state.get_asset_history(&asset_id) {
                                Ok(Some(history)) => {
                                    AssetsStateQueryResponse::AssetHistory(history)
                                }
                                Ok(None) => AssetsStateQueryResponse::NotFound,
                                Err(e) => AssetsStateQueryResponse::Error(e.to_string()),
                            },
                            None => AssetsStateQueryResponse::NotFound,
                        }
                    }
                    AssetsStateQuery::GetAssetAddresses { policy, name } => {
                        match reg.lookup_id(&policy, &name) {
                            Some(asset_id) => match state.get_asset_addresses(&asset_id) {
                                Ok(Some(addresses)) => {
                                    AssetsStateQueryResponse::AssetAddresses(addresses)
                                }
                                Ok(None) => AssetsStateQueryResponse::NotFound,
                                Err(e) => AssetsStateQueryResponse::Error(e.to_string()),
                            },
                            None => AssetsStateQueryResponse::NotFound,
                        }
                    }
                    AssetsStateQuery::GetAssetTransactions { policy, name } => {
                        match reg.lookup_id(&policy, &name) {
                            Some(asset_id) => match state.get_asset_transactions(&asset_id) {
                                Ok(Some(txs)) => AssetsStateQueryResponse::AssetTransactions(txs),
                                Ok(None) => AssetsStateQueryResponse::NotFound,
                                Err(e) => AssetsStateQueryResponse::Error(e.to_string()),
                            },
                            None => AssetsStateQueryResponse::NotFound,
                        }
                    }
                    AssetsStateQuery::GetPolicyIdAssets { policy } => {
                        match state.get_policy_assets(&policy, &reg) {
                            Ok(Some(assets)) => AssetsStateQueryResponse::PolicyIdAssets(assets),
                            Ok(None) => AssetsStateQueryResponse::NotFound,
                            Err(e) => AssetsStateQueryResponse::Error(e.to_string()),
                        }
                    }
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
                    if message.number % 60 == 0 {
                        let span = info_span!("assets_state.tick", number = message.number);
                        async {
                            let guard = tick_history.lock().await;
                            if let Some(state) = guard.current() {
                                if let Err(e) = state.tick() {
                                    error!("Tick error: {e}");
                                }
                            } else {
                                info!("no state yet");
                            }
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
            Self::run(history_run, deltas_sub, storage_config, registry_run)
                .await
                .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
