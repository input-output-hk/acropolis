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
use tokio::sync::{mpsc, Mutex};
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
const DEFAULT_ADDRESS_DB_PATH: (&str, &str) = ("db-path", "./db");
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
    ) -> Result<()> {
        let _ = params_subscription.read().await?;
        info!("Consumed initial genesis params from params_subscription");

        // Background task to persist epochs sequentialy
        const MAX_PENDING_PERSISTS: usize = 1;
        let (persist_tx, mut persist_rx) =
            mpsc::channel::<(u64, Arc<ImmutableAddressStore>, AddressStorageConfig)>(
                MAX_PENDING_PERSISTS,
            );
        tokio::spawn(async move {
            while let Some((epoch, store, config)) = persist_rx.recv().await {
                if let Err(e) = store.persist_epoch(epoch, &config).await {
                    error!("failed to persist epoch {epoch}: {e}");
                }
            }
        });

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
                    let (should_prune, store, config, epoch);
                    {
                        let mut state = state_mutex.lock().await;
                        // Skip processing for epochs already stored to DB
                        if let Some(min_epoch) = state.config.skip_until {
                            if block_info.epoch <= min_epoch {
                                state.volatile.next_block();
                                continue;
                            }
                        }

                        // Add deltas to volatile
                        if let Err(e) = state.apply_address_deltas(&address_deltas_msg.deltas) {
                            error!("address deltas handling error: {e:#}");
                        }

                        store = state.immutable.clone();
                        config = state.config.clone();
                        epoch = block_info.epoch;

                        // Move volatile deltas for an epoch to ImmutableAddressStore if out of rollback window
                        should_prune = state.ready_to_prune(&current_block);
                        if should_prune {
                            state.prune_volatile().await;
                        }
                    }

                    if should_prune {
                        if let Err(e) =
                            persist_tx.send((epoch, store.clone(), config.clone())).await
                        {
                            panic!("persistence worker crashed: {e}");
                        }
                    }

                    {
                        let mut state = state_mutex.lock().await;
                        state.volatile.next_block();
                    }
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
            db_path: get_string_flag(&config, DEFAULT_ADDRESS_DB_PATH),
            skip_until: None,
            store_info: get_bool_flag(&config, DEFAULT_STORE_INFO),
            store_totals: get_bool_flag(&config, DEFAULT_STORE_TOTALS),
            store_transactions: get_bool_flag(&config, DEFAULT_STORE_TRANSACTIONS),
        };

        let address_query_topic = get_string_flag(&config, DEFAULT_ADDRESS_QUERY_TOPIC);
        info!("Creating asset query handler on '{address_query_topic}'");

        // Initialize state
        let state = State::new(&storage_config).await?;
        let state_mutex = Arc::new(Mutex::new(state));
        let state_run = state_mutex.clone();

        // Query handler
        context.handle(&address_query_topic, move |message| {
            let state_mutex = state_mutex.clone();
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
                        match state.get_address_utxos(&address).await {
                            Ok(Some(utxos)) => AddressStateQueryResponse::AddressUTxOs(utxos),
                            Ok(None) => AddressStateQueryResponse::NotFound,
                            Err(e) => AddressStateQueryResponse::Error(e.to_string()),
                        }
                    }
                    AddressStateQuery::GetAddressTransactions { address } => {
                        match state.get_address_transactions(&address).await {
                            Ok(Some(txs)) => AddressStateQueryResponse::AddressTransactions(txs),
                            Ok(None) => AddressStateQueryResponse::NotFound,
                            Err(e) => AddressStateQueryResponse::Error(e.to_string()),
                        }
                    }
                    AddressStateQuery::GetAddressTotals { address } => {
                        match state.get_address_totals(&address).await {
                            Ok(totals) => AddressStateQueryResponse::AddressTotals(totals),
                            Err(e) => AddressStateQueryResponse::Error(e.to_string()),
                        }
                    }
                };
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Addresses(
                    response,
                )))
            }
        });

        if storage_config.any_enabled() {
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

            // Start run task
            context.run(async move {
                Self::run(state_run, address_deltas_sub, params_sub)
                    .await
                    .unwrap_or_else(|e| error!("Failed: {e}"));
            });
        }

        Ok(())
    }
}
