//! Acropolis Asset State module for Caryatid
//! Accepts native asset mint and burn events for supply and mint history tracking
//! as well as utxo delta events for CIP68 metadata and asset transactions tracking

use crate::{
    address_state::AddressState,
    asset_registry::AssetRegistry,
    state::{AssetsStorageConfig, State, StoreTransactions},
};
use acropolis_common::{
    caryatid::SubscriptionExt,
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::{
        assets::{AssetsStateQuery, AssetsStateQueryResponse, DEFAULT_ASSETS_QUERY_TOPIC},
        errors::QueryError,
    },
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};
mod address_state;
pub mod asset_registry;
mod state;

// Subscription topics
const DEFAULT_ASSET_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("asset-deltas-subscribe-topic", "cardano.asset.deltas");
const DEFAULT_UTXO_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("utxo-deltas-subscribe-topic", "cardano.utxo.deltas");
const DEFAULT_ADDRESS_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("address-deltas-subscribe-topic", "cardano.address.deltas");

// Configuration defaults
const DEFAULT_STORE_ASSETS: (&str, bool) = ("store-assets", false);
const DEFAULT_STORE_INFO: (&str, bool) = ("store-info", false);
const DEFAULT_STORE_HISTORY: (&str, bool) = ("store-history", false);
const DEFAULT_STORE_TRANSACTIONS: (&str, &str) = ("store-transactions", "none");
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
        address_state: Option<Arc<Mutex<AddressState>>>,
        mut asset_deltas_subscription: Box<dyn Subscription<Message>>,
        mut utxo_deltas_subscription: Option<Box<dyn Subscription<Message>>>,
        mut address_deltas_subscription: Option<Box<dyn Subscription<Message>>>,
        storage_config: AssetsStorageConfig,
        registry: Arc<Mutex<AssetRegistry>>,
    ) -> Result<()> {
        if let Some(sub) = utxo_deltas_subscription.as_mut() {
            let _ = sub.read_ignoring_rollbacks().await?;
            info!("Consumed initial message from utxo_deltas_subscription");
        }
        if let Some(sub) = address_deltas_subscription.as_mut() {
            let _ = sub.read_ignoring_rollbacks().await?;
            info!("Consumed initial message from address_deltas_subscription");
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
            let (_, asset_msg) = asset_deltas_subscription.read_ignoring_rollbacks().await?;
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
                        state = match state.handle_mint_deltas(&deltas_msg.deltas, &mut reg) {
                            Ok((new_state, updated_asset_ids)) => {
                                if let Some(ref address_state) = address_state {
                                    let mut address_state = address_state.lock().await;
                                    address_state.new_block(
                                        block_info.number,
                                        &updated_asset_ids,
                                        &reg,
                                    );
                                }
                                new_state
                            }
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
                            .handle_cip25_metadata(&mut reg, &deltas_msg.cip25_metadata_updates)
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
                let (_, utxo_msg) = sub.read_ignoring_rollbacks().await?;
                match utxo_msg.as_ref() {
                    Message::Cardano((
                        ref block_info,
                        CardanoMessage::UTXODeltas(utxo_deltas_msg),
                    )) => {
                        Self::check_sync(&current_block, block_info, "utxo");

                        if storage_config.store_info {
                            let reg = registry.lock().await;
                            state = match state.handle_cip68_metadata(&utxo_deltas_msg.deltas, &reg)
                            {
                                Ok(new_state) => new_state,
                                Err(e) => {
                                    error!("CIP-68 metadata handling error: {e:#}");
                                    state
                                }
                            };
                        }

                        if storage_config.store_transactions.is_enabled() {
                            let reg = registry.lock().await;
                            state = match state.handle_transactions(&utxo_deltas_msg.deltas, &reg) {
                                Ok(new_state) => new_state,
                                Err(e) => {
                                    error!("Transactions handling error: {e:#}");
                                    state
                                }
                            };
                        }
                    }
                    other => error!("Unexpected message on utxo-deltas subscription: {other:?}"),
                }
            }

            if let Some(sub) = address_deltas_subscription.as_mut() {
                let (_, address_msg) = sub.read_ignoring_rollbacks().await?;
                match address_msg.as_ref() {
                    Message::Cardano((
                        ref block_info,
                        CardanoMessage::AddressDeltas(address_deltas_msg),
                    )) => {
                        Self::check_sync(&current_block, block_info, "address");

                        let reg = registry.lock().await;
                        if let Some(ref address_state) = address_state {
                            let mut address_state = address_state.lock().await;
                            match address_state
                                .handle_address_deltas(&address_deltas_msg.deltas, &reg)
                            {
                                Ok(new_state) => new_state,
                                Err(e) => {
                                    error!("Address deltas handling error: {e:#}");
                                }
                            };
                        };
                    }
                    other => error!("Unexpected message on address-deltas subscription: {other:?}"),
                }
            }

            // Commit state
            {
                let mut h = history.lock().await;
                h.commit(current_block.number, state);
            }
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

        fn get_transactions_flag(config: &Config, key: (&str, &str)) -> StoreTransactions {
            let val = get_string_flag(config, key);
            match val.as_str() {
                "none" => StoreTransactions::None,
                "all" => StoreTransactions::All,
                s => {
                    if let Ok(n) = s.parse::<u64>() {
                        StoreTransactions::Last(n)
                    } else {
                        StoreTransactions::None
                    }
                }
            }
        }

        // Get configuration flags and topics
        let storage_config = AssetsStorageConfig {
            store_assets: get_bool_flag(&config, DEFAULT_STORE_ASSETS),
            store_info: get_bool_flag(&config, DEFAULT_STORE_INFO),
            store_history: get_bool_flag(&config, DEFAULT_STORE_HISTORY),
            store_transactions: get_transactions_flag(&config, DEFAULT_STORE_TRANSACTIONS),
            store_addresses: get_bool_flag(&config, DEFAULT_STORE_ADDRESSES),
            index_by_policy: get_bool_flag(&config, DEFAULT_INDEX_BY_POLICY),
        };

        let asset_deltas_subscribe_topic =
            get_string_flag(&config, DEFAULT_ASSET_DELTAS_SUBSCRIBE_TOPIC);
        info!("Creating subscriber on '{asset_deltas_subscribe_topic}'");

        let utxo_deltas_subscribe_topic: Option<String> =
            if storage_config.store_info || storage_config.store_transactions.is_enabled() {
                let topic = get_string_flag(&config, DEFAULT_UTXO_DELTAS_SUBSCRIBE_TOPIC);
                info!("Creating subscriber on '{topic}'");
                Some(topic)
            } else {
                None
            };

        let address_deltas_subscribe_topic: Option<String> = if storage_config.store_addresses {
            let topic = get_string_flag(&config, DEFAULT_ADDRESS_DELTAS_SUBSCRIBE_TOPIC);
            info!("Creating subscriber on '{topic}'");
            Some(topic)
        } else {
            None
        };

        let assets_query_topic = get_string_flag(&config, DEFAULT_ASSETS_QUERY_TOPIC);
        info!("Creating asset query handler on '{assets_query_topic}'");

        // Initialize state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "AssetsState",
            StateHistoryStore::default_block_store(),
        )));
        let address_state = if storage_config.store_addresses {
            Some(Arc::new(Mutex::new(AddressState::new())))
        } else {
            None
        };
        let history_run = history.clone();
        let query_history = history.clone();
        let tick_history = history.clone();
        let address_state_run = address_state.clone();
        let query_address_state = address_state.clone();

        // Initialize asset registry
        let registry = Arc::new(Mutex::new(asset_registry::AssetRegistry::new()));
        let registry_run = registry.clone();
        let query_registry = registry.clone();

        // Query handler
        context.handle(&assets_query_topic, move |message| {
            let history = query_history.clone();
            let address_state = query_address_state.clone();
            let registry = query_registry.clone();
            async move {
                let Message::StateQuery(StateQuery::Assets(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Assets(
                        AssetsStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for assets-state",
                        )),
                    )));
                };

                let state = {
                    let h = history.lock().await;
                    h.get_current_state()
                };

                let response = match query {
                    AssetsStateQuery::GetAssetsList => {
                        let reg = registry.lock().await;
                        match state.get_assets_list(&reg) {
                            Ok(list) => AssetsStateQueryResponse::AssetsList(list),
                            Err(e) => AssetsStateQueryResponse::Error(QueryError::internal_error(
                                e.to_string(),
                            )),
                        }
                    }
                    AssetsStateQuery::GetAssetInfo { policy, name } => {
                        let reg = registry.lock().await;
                        match reg.lookup_id(policy, name) {
                            Some(asset_id) => match state.get_asset_info(&asset_id, &reg) {
                                Ok(Some(info)) => AssetsStateQueryResponse::AssetInfo(info),
                                Ok(None) => {
                                    AssetsStateQueryResponse::Error(QueryError::not_found(format!(
                                        "Asset {}:{}",
                                        hex::encode(policy),
                                        hex::encode(name.as_slice())
                                    )))
                                }
                                Err(e) => AssetsStateQueryResponse::Error(
                                    QueryError::internal_error(e.to_string()),
                                ),
                            },
                            None => {
                                if state.config.store_info && state.config.store_assets {
                                    AssetsStateQueryResponse::Error(QueryError::not_found(format!(
                                        "Asset {}:{}",
                                        hex::encode(policy),
                                        hex::encode(name.as_slice())
                                    )))
                                } else {
                                    AssetsStateQueryResponse::Error(QueryError::storage_disabled(
                                        "asset info",
                                    ))
                                }
                            }
                        }
                    }
                    AssetsStateQuery::GetAssetHistory { policy, name } => {
                        let reg = registry.lock().await;
                        match reg.lookup_id(policy, name) {
                            Some(asset_id) => match state.get_asset_history(&asset_id) {
                                Ok(Some(history)) => {
                                    AssetsStateQueryResponse::AssetHistory(history)
                                }
                                Ok(None) => {
                                    AssetsStateQueryResponse::Error(QueryError::not_found(format!(
                                        "Asset history for {}:{}",
                                        hex::encode(policy),
                                        hex::encode(name.as_slice())
                                    )))
                                }
                                Err(e) => AssetsStateQueryResponse::Error(
                                    QueryError::internal_error(e.to_string()),
                                ),
                            },
                            None => {
                                if state.config.store_history {
                                    AssetsStateQueryResponse::Error(QueryError::not_found(format!(
                                        "Asset history for {}:{}",
                                        hex::encode(policy),
                                        hex::encode(name.as_slice())
                                    )))
                                } else {
                                    AssetsStateQueryResponse::Error(QueryError::storage_disabled(
                                        "asset history",
                                    ))
                                }
                            }
                        }
                    }
                    AssetsStateQuery::GetAssetAddresses { policy, name } => match address_state {
                        Some(address_state) => {
                            let reg = registry.lock().await;
                            let address_state = address_state.lock().await;
                            match reg.lookup_id(policy, name) {
                                Some(asset_id) => {
                                    match address_state.get_asset_addresses(&asset_id) {
                                        Ok(Some(addresses)) => {
                                            AssetsStateQueryResponse::AssetAddresses(addresses)
                                        }
                                        Ok(None) => AssetsStateQueryResponse::Error(
                                            QueryError::not_found(format!(
                                                "Asset addresses for {}:{}",
                                                hex::encode(policy),
                                                hex::encode(name.as_slice())
                                            )),
                                        ),
                                        Err(e) => AssetsStateQueryResponse::Error(
                                            QueryError::internal_error(e.to_string()),
                                        ),
                                    }
                                }
                                None => {
                                    if state.config.store_addresses {
                                        AssetsStateQueryResponse::Error(QueryError::not_found(
                                            format!(
                                                "Asset addresses for {}:{}",
                                                hex::encode(policy),
                                                hex::encode(name.as_slice())
                                            ),
                                        ))
                                    } else {
                                        AssetsStateQueryResponse::Error(
                                            QueryError::storage_disabled("asset addresses"),
                                        )
                                    }
                                }
                            }
                        }
                        None => AssetsStateQueryResponse::Error(QueryError::storage_disabled(
                            "asset addresses",
                        )),
                    },
                    AssetsStateQuery::GetAssetTransactions { policy, name } => {
                        let reg = registry.lock().await;
                        match reg.lookup_id(policy, name) {
                            Some(asset_id) => match state.get_asset_transactions(&asset_id) {
                                Ok(Some(txs)) => AssetsStateQueryResponse::AssetTransactions(txs),
                                Ok(None) => {
                                    AssetsStateQueryResponse::Error(QueryError::not_found(format!(
                                        "Asset transactions for {}:{}",
                                        hex::encode(policy),
                                        hex::encode(name.as_slice())
                                    )))
                                }
                                Err(e) => AssetsStateQueryResponse::Error(
                                    QueryError::internal_error(e.to_string()),
                                ),
                            },
                            None => {
                                if state.config.store_transactions.is_enabled() {
                                    AssetsStateQueryResponse::Error(QueryError::not_found(format!(
                                        "Asset transactions for {}:{}",
                                        hex::encode(policy),
                                        hex::encode(name.as_slice())
                                    )))
                                } else {
                                    AssetsStateQueryResponse::Error(QueryError::storage_disabled(
                                        "asset transactions",
                                    ))
                                }
                            }
                        }
                    }
                    AssetsStateQuery::GetPolicyIdAssets { policy } => {
                        let reg = registry.lock().await;
                        match state.get_policy_assets(policy, &reg) {
                            Ok(Some(assets)) => AssetsStateQueryResponse::PolicyIdAssets(assets),
                            Ok(None) => AssetsStateQueryResponse::Error(QueryError::not_found(
                                format!("Assets for policy {}", hex::encode(policy)),
                            )),
                            Err(e) => AssetsStateQueryResponse::Error(QueryError::internal_error(
                                e.to_string(),
                            )),
                        }
                    }
                    AssetsStateQuery::GetAssetsMetadata { assets } => {
                        let reg = registry.lock().await;
                        match state.get_assets_metadata(assets, &reg) {
                            Ok(Some(assets)) => AssetsStateQueryResponse::AssetsMetadata(assets),
                            Ok(None) => AssetsStateQueryResponse::Error(QueryError::not_found(
                                "One or more assets not found in registry".to_string(),
                            )),
                            Err(e) => AssetsStateQueryResponse::Error(QueryError::internal_error(
                                e.to_string(),
                            )),
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
        let utxo_deltas_sub = if let Some(topic) = &utxo_deltas_subscribe_topic {
            Some(context.subscribe(topic).await?)
        } else {
            None
        };
        let address_deltas_sub = if let Some(topic) = &address_deltas_subscribe_topic {
            Some(context.subscribe(topic).await?)
        } else {
            None
        };

        // Start run task
        context.run(async move {
            Self::run(
                history_run,
                address_state_run,
                asset_deltas_sub,
                utxo_deltas_sub,
                address_deltas_sub,
                storage_config,
                registry_run,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
