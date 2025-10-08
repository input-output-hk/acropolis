//! Acropolis Address State module for Caryatid.
//! Consumes address delta messages and indexes per-address
//! utxos, transactions, and total sent/received amounts.

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
use tracing::{error, info};

use crate::{
    immutable_address_store::ImmutableAddressStore,
    state::{AddressStorageConfig, State},
};
mod immutable_address_store;
mod state;
mod volatile_addresses;

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
    description = "In-memory Address State from address delta events"
)]
pub struct AddressState;

impl AddressState {
    async fn run(
        state_mutex: Arc<Mutex<State>>,
        mut address_deltas_subscription: Box<dyn Subscription<Message>>,
        mut params_subscription: Box<dyn Subscription<Message>>,
        persist_after: Option<u64>,
        store: Arc<ImmutableAddressStore>,
    ) -> Result<()> {
        let _ = params_subscription.read().await?;
        info!("Consumed initial genesis params from params_subscription");

        // Main loop of synchronised messages
        loop {
            // Address deltas are the synchroniser
            let (_, deltas_msg) = address_deltas_subscription.read().await?;
            let (current_block, new_epoch) = match deltas_msg.as_ref() {
                Message::Cardano((info, _)) => (info.clone(), info.new_epoch && info.epoch > 0),
                _ => continue,
            };

            if current_block.status == BlockStatus::RolledBack {
                let mut state = state_mutex.lock().await;
                state.volatile.rollback_before(current_block.number);
                state.volatile.next_block();
            }

            // Read params message on epoch bounday to update rollback window
            // length if needed and set epoch start block for volatile pruning
            if new_epoch {
                let (_, message) = params_subscription.read().await?;
                if let Message::Cardano((ref block_info, CardanoMessage::ProtocolParams(params))) =
                    message.as_ref()
                {
                    Self::check_sync(&current_block, &block_info, "params");
                    let mut state = state_mutex.lock().await;
                    state.volatile.start_new_epoch(block_info.number);
                    if let Some(shelley) = &params.params.shelley {
                        state.volatile.update_k(shelley.security_param);
                    }
                }
            }

            // Process address deltas into volatile and persist to disk if a full epoch is out of rollback window
            match deltas_msg.as_ref() {
                Message::Cardano((
                    ref block_info,
                    CardanoMessage::AddressDeltas(address_deltas_msg),
                )) => {
                    let mut state = state_mutex.lock().await;
                    // Skip processing for epochs already stored to DB
                    if let Some(min_epoch) = persist_after {
                        if block_info.epoch <= min_epoch {
                            state.volatile.next_block();
                            continue;
                        }
                    }

                    // Add deltas to volatile
                    if let Err(e) = state.apply_address_deltas(&address_deltas_msg.deltas) {
                        error!("address deltas handling error: {e:#}");
                    }

                    // Persist full epoch to disk if ready
                    if state.ready_to_prune(&current_block) {
                        let config = state.config.clone();
                        state.volatile.persist_all(store.as_ref(), &config).await?;
                    }

                    state.volatile.next_block();
                }
                other => error!("Unexpected message on address-deltas subscription: {other:?}"),
            }
        }
    }

    fn check_sync(expected: &BlockInfo, actual: &BlockInfo, source: &str) {
        if expected.number != actual.number {
            error!(
                expected = expected.number,
                actual = actual.number,
                source = source,
                "Messages out of sync (expected deltas block {}, got {} from {})",
                expected.number,
                actual.number,
                source,
            );
            panic!(
                "Message streams diverged: deltas at {} vs {} from {}",
                expected.number, actual.number, source
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

        // Get configuration flags and query topic
        let storage_config = AddressStorageConfig {
            store_info: get_bool_flag(&config, DEFAULT_STORE_INFO),
            store_totals: get_bool_flag(&config, DEFAULT_STORE_TOTALS),
            store_transactions: get_bool_flag(&config, DEFAULT_STORE_TRANSACTIONS),
        };

        let address_query_topic = get_string_flag(&config, DEFAULT_ADDRESS_QUERY_TOPIC);
        info!("Creating asset query handler on '{address_query_topic}'");

        // Initialize state history
        let state = Arc::new(Mutex::new(State::new(storage_config)));
        let state_run = state.clone();
        let state_query = state.clone();

        // Initialize Fjall store
        let store = if storage_config.any_enabled() {
            let path = config
                .get_string("address_state.path")
                .unwrap_or_else(|_| "./data/address_state".to_string());
            let store = ImmutableAddressStore::new(path)?;
            Some(Arc::new(store))
        } else {
            None
        };
        let query_store = store.clone();

        // Query handler
        context.handle(&address_query_topic, move |message| {
            let state_mutex = state_query.clone();
            let store = query_store.clone();
            async move {
                let Message::StateQuery(StateQuery::Addresses(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Addresses(
                        AddressStateQueryResponse::Error(
                            "Invalid message for address-state".into(),
                        ),
                    )));
                };

                let state = state_mutex.lock().await;
                let response = match query {
                    AddressStateQuery::GetAddressUTxOs { address } => {
                        if let Some(ref s) = store {
                            match state.get_address_utxos(s, &address).await {
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
                            match state.get_address_transactions(s, &address).await {
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

        if let Some(store) = store {
            // Get subscribe topics
            let address_deltas_subscribe_topic =
                get_string_flag(&config, DEFAULT_ADDRESS_DELTAS_SUBSCRIBE_TOPIC);
            info!("Creating subscriber on '{address_deltas_subscribe_topic}'");
            let params_subscribe_topic =
                get_string_flag(&config, DEFAULT_PARAMETERS_SUBSCRIBE_TOPIC);
            info!("Creating subscriber on '{params_subscribe_topic}'");

            // Subscribe to enabled topics
            let address_deltas_sub = context.subscribe(&address_deltas_subscribe_topic).await?;
            let params_sub = context.subscribe(&params_subscribe_topic).await?;

            let persist_after = store.get_last_epoch_stored().await?;
            // Start run task
            context.run(async move {
                Self::run(
                    state_run,
                    address_deltas_sub,
                    params_sub,
                    persist_after,
                    store,
                )
                .await
                .unwrap_or_else(|e| error!("Failed: {e}"));
            });
        }

        Ok(())
    }
}
