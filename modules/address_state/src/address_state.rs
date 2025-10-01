//! Acropolis Address State module for Caryatid.
//! Consumes UTxO delta messages and indexes per-address
//! balances, transactions, and total sent/received amounts.

use std::sync::Arc;

use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::addresses::{
        AddressStateQuery, AddressStateQueryResponse, DEFAULT_ADDRESS_QUERY_TOPIC,
    },
    BlockInfo, BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module, Subscription};
use config::Config;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

use crate::{
    address_store::AddressStore,
    state::{AddressStorageConfig, State},
};
mod address_store;
mod fjall_immutable_address_store;
use fjall_immutable_address_store::FjallImmutableAddressStore;
mod state;
mod volatile_index;

// Subscription topics
const DEFAULT_ADDRESS_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("address-deltas-subscribe-topic", "cardano.address.delta");
const DEFAULT_PARAMETERS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("parameters-subscribe-topic", "cardano.protocol.parameters");

// Configuration defaults
const DEFAULT_STORE_INFO: (&str, bool) = ("store-info", false);
const DEFAULT_STORE_TOTALS: (&str, bool) = ("store-totals", false);
const DEFAULT_STORE_TRANSACTIONS: (&str, bool) = ("store-transactions", false);

/// Address State module
#[module(
    message_type(Message),
    name = "address-state",
    description = "In-memory Address State from utxo delta events"
)]
pub struct AddressState;

impl AddressState {
    async fn run(
        state_mutex: Arc<Mutex<State>>,
        mut address_deltas_subscription: Option<Box<dyn Subscription<Message>>>,
        mut params_subscription: Option<Box<dyn Subscription<Message>>>,
        persist_epoch: Option<u64>,
        store: Option<Arc<dyn AddressStore>>,
    ) -> Result<()> {
        if let Some(sub) = params_subscription.as_mut() {
            let _ = sub.read().await?;
            info!("Consumed initial genesis params from params_subscription");
        }
        // Main loop of synchronised messages
        loop {
            let mut current_block: Option<BlockInfo> = None;

            let mut state = state_mutex.lock().await;
            // Handle UTxO deltas if subscription is registered (store-info or store-transactions enabled)
            if let Some(sub) = address_deltas_subscription.as_mut() {
                let (_, deltas_msg) = sub.read().await?;
                let new_epoch = match deltas_msg.as_ref() {
                    Message::Cardano((ref block_info, _)) => {
                        if block_info.status == BlockStatus::RolledBack {
                            state.volatile_entries.rollback_before(block_info.number);
                        } else {
                            state.volatile_entries.next_block();
                        }
                        current_block = Some(block_info.clone());
                        block_info.new_epoch && block_info.epoch > 0
                    }
                    _ => false,
                };

                if new_epoch {
                    if let Some(sub) = params_subscription.as_mut() {
                        let (_, message) = sub.read().await?;
                        if let Message::Cardano((
                            ref block_info,
                            CardanoMessage::ProtocolParams(params),
                        )) = message.as_ref()
                        {
                            Self::check_sync(&current_block, &block_info, "params");
                            state.volatile_entries.start_new_epoch(block_info.number);
                            if let Some(shelley) = &params.params.shelley {
                                state.volatile_entries.update_k(shelley.security_param);
                            }
                        }
                    }
                }

                match deltas_msg.as_ref() {
                    Message::Cardano((
                        ref block_info,
                        CardanoMessage::AddressDeltas(address_deltas_msg),
                    )) => {
                        // Skip processing for epochs already stored to DB
                        if let Some(min_epoch) = persist_epoch {
                            if block_info.epoch <= min_epoch {
                                continue;
                            }
                        }

                        // Update volatile entries
                        if let Err(e) = state.handle_address_deltas(&address_deltas_msg.deltas) {
                            error!("address deltas handling error: {e:#}");
                        }

                        if block_info.epoch > 0 {
                            // Compute the safe_block at which the previous epoch can be removed from volatile
                            let safe_block = state.volatile_entries.epoch_start_block
                                + state.volatile_entries.security_param_k;

                            // Persist to disk and prune from volatile when block number exceeds safe block
                            if block_info.number > safe_block {
                                if Some(block_info.epoch)
                                    != state.volatile_entries.last_persisted_epoch
                                {
                                    if let Some(address_store) = &store {
                                        let config = state.config.clone();
                                        state
                                            .volatile_entries
                                            .persist_all(address_store.as_ref(), &config)
                                            .await?;
                                    }
                                }
                            }
                        }
                    }
                    other => error!("Unexpected message on utxo-deltas subscription: {other:?}"),
                }
            }
        }
    }

    fn check_sync(expected: &Option<BlockInfo>, actual: &BlockInfo, source: &str) {
        if let Some(ref block) = expected {
            if block.number != actual.number {
                error!(
                    expected = block.number,
                    actual = actual.number,
                    source = source,
                    "Messages out of sync (expected certs block {}, got {} from {})",
                    block.number,
                    actual.number,
                    source,
                );
                panic!(
                    "Message streams diverged: certs at {} vs {} from {}",
                    block.number, actual.number, source
                );
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
        let storage_config = AddressStorageConfig {
            store_info: get_bool_flag(&config, DEFAULT_STORE_INFO),
            store_totals: get_bool_flag(&config, DEFAULT_STORE_TOTALS),
            store_transactions: get_bool_flag(&config, DEFAULT_STORE_TRANSACTIONS),
        };

        let address_deltas_subscribe_topic: Option<String> = if storage_config.any_enabled() {
            let topic = get_string_flag(&config, DEFAULT_ADDRESS_DELTAS_SUBSCRIBE_TOPIC);
            info!("Creating subscriber on '{topic}'");
            Some(topic)
        } else {
            None
        };

        let params_subscribe_topic: Option<String> = if storage_config.any_enabled() {
            let topic = get_string_flag(&config, DEFAULT_PARAMETERS_SUBSCRIBE_TOPIC);
            info!("Creating subscriber on '{topic}'");
            Some(topic)
        } else {
            None
        };

        let address_query_topic = get_string_flag(&config, DEFAULT_ADDRESS_QUERY_TOPIC);
        info!("Creating asset query handler on '{address_query_topic}'");

        // Initialize state history
        let state = Arc::new(Mutex::new(State::new(storage_config)));
        let state_run = state.clone();
        let state_query = state.clone();
        let state_tick = state.clone();

        // Initialize Fjall store
        let (store, persist_epoch): (Option<Arc<dyn AddressStore>>, Option<u64>) =
            if storage_config.any_enabled() {
                let path = config
                    .get_string("address_state.path")
                    .unwrap_or_else(|_| "./data/address_state".to_string());

                let store = FjallImmutableAddressStore::new(path)?;
                let persist_after = store.get_last_epoch_stored().await?;
                (
                    Some(Arc::new(store) as Arc<dyn AddressStore>),
                    persist_after,
                )
            } else {
                (None, None)
            };

        match persist_epoch {
            Some(epoch) => info!("Persist epoch marker found: {}", epoch),
            None => info!("No persist epoch marker found in store"),
        }
        let query_store = store.clone();
        let store_run = store.clone();

        // Query handler
        context.handle(&address_query_topic, move |message| {
            let state_mutex = state_query.clone();
            let store = query_store.clone();
            async move {
                let Message::StateQuery(StateQuery::Addresses(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Addresses(
                        AddressStateQueryResponse::Error("Invalid message for assets-state".into()),
                    )));
                };

                let state = state_mutex.lock().await;
                let response = match query {
                    AddressStateQuery::GetAddressUTxOs { address } => {
                        if let Some(ref s) = store {
                            match state.get_address_utxos(s.as_ref(), &address).await {
                                Ok(Some(utxos)) => AddressStateQueryResponse::AddressUTxOs(utxos),
                                Ok(None) => AddressStateQueryResponse::NotFound,
                                Err(e) => AddressStateQueryResponse::Error(e.to_string()),
                            }
                        } else {
                            AddressStateQueryResponse::Error("Address store not initialized".into())
                        }
                    }
                    AddressStateQuery::GetAddressTransactions { address } => {
                        if let Some(ref s) = store {
                            match state.get_address_transactions(s.as_ref(), &address).await {
                                Ok(Some(txs)) => {
                                    AddressStateQueryResponse::AddressTransactions(txs)
                                }
                                Ok(None) => AddressStateQueryResponse::NotFound,
                                Err(e) => AddressStateQueryResponse::Error(e.to_string()),
                            }
                        } else {
                            AddressStateQueryResponse::Error("Address store not initialized".into())
                        }
                    }
                    AddressStateQuery::GetAddressTotals { address } => {
                        if let Some(ref s) = store {
                            match state.get_address_totals(s.as_ref(), &address).await {
                                Ok(totals) => AddressStateQueryResponse::AddressTotals(totals),
                                Err(e) => AddressStateQueryResponse::Error(e.to_string()),
                            }
                        } else {
                            AddressStateQueryResponse::Error("Address store not initialized".into())
                        }
                    }
                };
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Addresses(
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
                        let span = info_span!("address_state.tick", number = message.number);
                        async {
                            let state = state_tick.lock().await;
                            if let Err(e) = state.tick() {
                                error!("Tick error: {e}");
                            }
                        }
                        .instrument(span)
                        .await;
                    }
                }
            }
        });

        // Subscribe to enabled topics
        let address_deltas_sub = if let Some(topic) = &address_deltas_subscribe_topic {
            Some(context.subscribe(topic).await?)
        } else {
            None
        };

        let params_sub = if let Some(topic) = &params_subscribe_topic {
            Some(context.subscribe(topic).await?)
        } else {
            None
        };

        // Start run task
        context.run(async move {
            Self::run(
                state_run,
                address_deltas_sub,
                params_sub,
                persist_epoch,
                store_run,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
