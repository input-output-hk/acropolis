//! Acropolis Address State module for Caryatid.
//! Consumes UTxO delta messages and indexes per-address
//! balances, transactions, and total sent/received amounts.

use std::sync::Arc;

use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::addresses::{
        AddressStateQuery, AddressStateQueryResponse, DEFAULT_ADDRESS_QUERY_TOPIC,
    },
    state_history::{StateHistory, StateHistoryStore},
    BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module, Subscription};
use config::Config;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

use crate::state::{AddressStorageConfig, State};
mod state;

// Subscription topics
const DEFAULT_ADDRESS_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("address-deltas-subscribe-topic", "cardano.address.delta");

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
        history: Arc<Mutex<StateHistory<State>>>,
        mut address_deltas_subscription: Option<Box<dyn Subscription<Message>>>,
        storage_config: AddressStorageConfig,
    ) -> Result<()> {
        // Main loop of synchronised messages
        loop {
            // Get current state snapshot
            let mut state = {
                let mut h = history.lock().await;
                h.get_or_init_with(|| State::new(storage_config))
            };

            // Handle UTxO deltas if subscription is registered (store-info or store-transactions enabled)
            if let Some(sub) = address_deltas_subscription.as_mut() {
                let (_, deltas_msg) = sub.read().await?;
                match deltas_msg.as_ref() {
                    Message::Cardano((
                        ref block_info,
                        CardanoMessage::AddressDeltas(address_deltas_msg),
                    )) => {
                        if block_info.status == BlockStatus::RolledBack {
                            state = history.lock().await.get_rolled_back_state(block_info.number);
                        }

                        state = match state.handle_address_deltas(&address_deltas_msg.deltas) {
                            Ok(new_state) => new_state,
                            Err(e) => {
                                error!("address deltas handling error: {e:#}");
                                state
                            }
                        };

                        // Commit state
                        {
                            let mut h = history.lock().await;
                            h.commit(block_info.number, state);
                        }
                    }
                    other => error!("Unexpected message on utxo-deltas subscription: {other:?}"),
                }
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

        let address_query_topic = get_string_flag(&config, DEFAULT_ADDRESS_QUERY_TOPIC);
        info!("Creating asset query handler on '{address_query_topic}'");

        // Initalize state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "AddressState",
            StateHistoryStore::default_block_store(),
        )));
        let history_run = history.clone();
        let query_history = history.clone();
        let tick_history = history.clone();

        // Query handler
        context.handle(&address_query_topic, move |message| {
            let history = query_history.clone();
            async move {
                let Message::StateQuery(StateQuery::Addresses(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Addresses(
                        AddressStateQueryResponse::Error("Invalid message for assets-state".into()),
                    )));
                };

                let state = history.lock().await.get_current_state();

                let response = match query {
                    AddressStateQuery::GetAddressUTxOs { address } => {
                        match state.get_address_utxos(&address) {
                            Ok(Some(utxos)) => AddressStateQueryResponse::AddressUTxOs(utxos),
                            Ok(None) => AddressStateQueryResponse::NotFound,
                            Err(e) => AddressStateQueryResponse::Error(e.to_string()),
                        }
                    }

                    AddressStateQuery::GetAddressTotals { address } => {
                        match state.get_address_totals(&address) {
                            Ok(totals) => AddressStateQueryResponse::AddressTotals(totals),
                            Err(e) => AddressStateQueryResponse::Error(e.to_string()),
                        }
                    }
                    AddressStateQuery::GetAddressTransactions { address } => {
                        match state.get_address_transactions(&address) {
                            Ok(Some(txs)) => AddressStateQueryResponse::AddressTransactions(txs),
                            Ok(None) => AddressStateQueryResponse::NotFound,
                            Err(e) => AddressStateQueryResponse::Error(e.to_string()),
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
        let address_deltas_sub = if let Some(topic) = &address_deltas_subscribe_topic {
            Some(context.subscribe(topic).await?)
        } else {
            None
        };

        // Start run task
        context.run(async move {
            Self::run(history_run, address_deltas_sub, storage_config)
                .await
                .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
