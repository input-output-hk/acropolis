//! Acropolis Asset State module for Caryatid
//! Accepts native asset mint and burn events for supply and mint history tracking
//! as well as utxo delta events for CIP68 metadata and asset transactions tracking

use crate::{
    asset_registry::AssetRegistry,
    state::{AssetsStorageConfig, State},
};
use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::assets::{AssetsStateQuery, AssetsStateQueryResponse, DEFAULT_ASSETS_QUERY_TOPIC},
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};
pub mod asset_registry;
mod state;

// Subscription topics
const DEFAULT_ASSET_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("asset-deltas-subscribe-topic", "cardano.asset.deltas");
const DEFAULT_UTXO_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("utxo-deltas-subscribe-topic", "cardano.utxo.deltas");

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
        mut asset_deltas_subscription: Box<dyn Subscription<Message>>,
        mut utxo_deltas_subscription: Option<Box<dyn Subscription<Message>>>,
        storage_config: AssetsStorageConfig,
        registry: Arc<Mutex<AssetRegistry>>,
    ) -> Result<()> {
        if let Some(sub) = utxo_deltas_subscription.as_mut() {
            let _ = sub.read().await?;
            info!("Consumed initial message from utxo_deltas_subscription");
        }
        // Main loop of synchronised messages
        loop {
            // Get current state snapshot
            let mut state = {
                let mut h = history.lock().await;
                h.get_or_init_with(|| State::new(storage_config))
            };
            let current_block: BlockInfo;

            // Asset deltas are the synchroniser
            let (_, asset_msg) = asset_deltas_subscription.read().await?;
            match asset_msg.as_ref() {
                Message::Cardano((ref block_info, CardanoMessage::AssetDeltas(deltas_msg))) => {
                    // rollback only on asset deltas
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(block_info.number);
                    }
                    current_block = block_info.clone();

                    // Always handle the mint deltas (This is how assets get initialized)
                    {
                        let mut reg = registry.lock().await;
                        state = match state.handle_mint_deltas(&deltas_msg.deltas, &mut *reg) {
                            Ok(new_state) => new_state,
                            Err(e) => {
                                error!("Asset deltas handling error: {e:#}");
                                state
                            }
                        };
                    }

                    // Process CIP25 metadata updates
                    if storage_config.store_info {
                        let mut reg = registry.lock().await;
                        state = match state
                            .handle_cip25_metadata(&mut *reg, &deltas_msg.cip25_metadata_updates)
                        {
                            Ok(new_state) => new_state,
                            Err(e) => {
                                error!("CIP-25 metadata handling error: {e:#}");
                                state
                            }
                        };
                    }
                }
                other => {
                    error!("Unexpected message on asset-deltas subscription: {other:?}");
                    continue;
                }
            }

            // Handle UTxO deltas if subscription is registered (store-info or store-transactions enabled)
            if let Some(sub) = utxo_deltas_subscription.as_mut() {
                let (_, utxo_msg) = sub.read().await?;
                match utxo_msg.as_ref() {
                    Message::Cardano((
                        ref block_info,
                        CardanoMessage::UTXODeltas(utxo_deltas_msg),
                    )) => {
                        Self::check_sync(&current_block, block_info, "utxo");
                        let span =
                            info_span!("assets_state.handle_utxo", block = block_info.number);
                        let _enter = span.enter();

                        let mut reg = registry.lock().await;

                        state =
                            match state.handle_cip68_metadata(&utxo_deltas_msg.deltas, &mut *reg) {
                                Ok(new_state) => new_state,
                                Err(e) => {
                                    info!("CIP-68 metadata handling error: {e:#}");
                                    state
                                }
                            };
                    }
                    other => error!("Unexpected message on utxo-deltas subscription: {other:?}"),
                }
            }

            // Commit state at the end of the block
            history.lock().await.commit(current_block.number, state);
        }
    }

    /// Check for synchronisation
    fn check_sync(expected: &BlockInfo, actual: &BlockInfo, source: &str) {
        if expected.number != actual.number {
            error!(
                expected = expected.number,
                actual = actual.number,
                source = source,
                "Messages out of sync (expected block {}, got {} from {})",
                expected.number,
                actual.number,
                source,
            );
            panic!(
                "Message streams diverged: {} at {} vs {} from {}",
                source, expected.number, actual.number, source,
            );
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

        let mut utxo_deltas_subscribe_topic = String::new();
        if storage_config.store_info {
            utxo_deltas_subscribe_topic =
                get_string_flag(&config, DEFAULT_UTXO_DELTAS_SUBSCRIBE_TOPIC);
            info!("Creating subscriber on '{utxo_deltas_subscribe_topic}'");
        }

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

                let response = match query {
                    AssetsStateQuery::GetAssetsList => {
                        let reg = registry.lock().await;
                        match state.get_assets_list(&reg) {
                            Ok(list) => AssetsStateQueryResponse::AssetsList(list),
                            Err(e) => AssetsStateQueryResponse::Error(e.to_string()),
                        }
                    }
                    AssetsStateQuery::GetAssetInfo { policy, name } => {
                        let reg = registry.lock().await;
                        match reg.lookup_id(&policy, &name) {
                            Some(asset_id) => match state.get_asset_info(&asset_id, &reg) {
                                Ok(Some(info)) => AssetsStateQueryResponse::AssetInfo(info),
                                Ok(None) => AssetsStateQueryResponse::NotFound,
                                Err(e) => AssetsStateQueryResponse::Error(e.to_string()),
                            },
                            None => {
                                if state.config.store_info {
                                    AssetsStateQueryResponse::NotFound
                                } else {
                                    AssetsStateQueryResponse::Error(
                                        "asset info storage disabled in config".to_string(),
                                    )
                                }
                            }
                        }
                    }
                    AssetsStateQuery::GetAssetHistory { policy, name } => {
                        let reg = registry.lock().await;
                        match reg.lookup_id(&policy, &name) {
                            Some(asset_id) => match state.get_asset_history(&asset_id) {
                                Ok(Some(history)) => {
                                    AssetsStateQueryResponse::AssetHistory(history)
                                }
                                Ok(None) => AssetsStateQueryResponse::NotFound,
                                Err(e) => AssetsStateQueryResponse::Error(e.to_string()),
                            },
                            None => {
                                if state.config.store_history {
                                    AssetsStateQueryResponse::NotFound
                                } else {
                                    AssetsStateQueryResponse::Error(
                                        "asset history storage disabled in config".to_string(),
                                    )
                                }
                            }
                        }
                    }
                    AssetsStateQuery::GetAssetAddresses { policy, name } => {
                        let reg = registry.lock().await;
                        match reg.lookup_id(&policy, &name) {
                            Some(asset_id) => match state.get_asset_addresses(&asset_id) {
                                Ok(Some(addresses)) => {
                                    AssetsStateQueryResponse::AssetAddresses(addresses)
                                }
                                Ok(None) => AssetsStateQueryResponse::NotFound,
                                Err(e) => AssetsStateQueryResponse::Error(e.to_string()),
                            },
                            None => {
                                if state.config.store_addresses {
                                    AssetsStateQueryResponse::NotFound
                                } else {
                                    AssetsStateQueryResponse::Error(
                                        "asset addresses storage disabled in config".to_string(),
                                    )
                                }
                            }
                        }
                    }
                    AssetsStateQuery::GetAssetTransactions { policy, name } => {
                        let reg = registry.lock().await;
                        match reg.lookup_id(&policy, &name) {
                            Some(asset_id) => match state.get_asset_transactions(&asset_id) {
                                Ok(Some(txs)) => AssetsStateQueryResponse::AssetTransactions(txs),
                                Ok(None) => AssetsStateQueryResponse::NotFound,
                                Err(e) => AssetsStateQueryResponse::Error(e.to_string()),
                            },
                            None => {
                                if state.config.store_transactions {
                                    AssetsStateQueryResponse::NotFound
                                } else {
                                    AssetsStateQueryResponse::Error(
                                        "asset transactions storage disabled in config".to_string(),
                                    )
                                }
                            }
                        }
                    }
                    AssetsStateQuery::GetPolicyIdAssets { policy } => {
                        let reg = registry.lock().await;
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
        let asset_deltas_sub = context.subscribe(&asset_deltas_subscribe_topic).await?;
        let utxo_deltas_sub = if storage_config.store_info || storage_config.store_transactions {
            Some(context.subscribe(&utxo_deltas_subscribe_topic).await?)
        } else {
            None
        };

        // Start run task
        context.run(async move {
            Self::run(
                history_run,
                asset_deltas_sub,
                utxo_deltas_sub,
                storage_config,
                registry_run,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
