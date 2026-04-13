//! Acropolis Address State module for Caryatid.
//! Consumes address delta messages and indexes per-address
//! utxos, transactions, and total sent/received amounts.

use std::sync::Arc;

use crate::{
    immutable_address_store::ImmutableAddressStore,
    state::{AddressStorageConfig, State},
};
use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper},
    configuration::{get_bool_flag, get_string_flag, StartupMode},
    declare_cardano_reader,
    messages::{AddressDeltasMessage, ProtocolParamsMessage, StateTransitionMessage},
    queries::errors::QueryError,
};
use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::addresses::{
        AddressStateQuery, AddressStateQueryResponse, DEFAULT_ADDRESS_QUERY_TOPIC,
    },
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info};
mod immutable_address_store;
mod state;
mod volatile_addresses;

declare_cardano_reader!(
    AddressDeltasReader,
    "address-deltas-subscribe-topic",
    "cardano.address.deltas",
    AddressDeltas,
    AddressDeltasMessage
);
declare_cardano_reader!(
    ParamsReader,
    "parameters-subscribe-topic",
    "cardano.protocol.parameters",
    ProtocolParams,
    ProtocolParamsMessage
);

// Configuration defaults
const DEFAULT_ADDRESS_DB_PATH: (&str, &str) = ("db-path", "./fjall-addresses");
const DEFAULT_CLEAR_ON_START: (&str, bool) = ("clear-on-start", true);
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
        mut address_deltas_reader: AddressDeltasReader,
        mut params_reader: ParamsReader,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        if !is_snapshot_mode {
            match params_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal(_) => {}
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial params");
                }
            }
            debug!("Consumed initial genesis params from params_subscription");
        }

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
            let primary =
                PrimaryRead::from_read(address_deltas_reader.read_with_rollbacks().await?);

            if primary.is_rollback() {
                let mut state = state_mutex.lock().await;
                state.volatile.rollback_before(primary.block_info().number);
                state.volatile.next_block();
            }

            // Epoch-0 params are consumed during init, so the main loop only needs
            // real epoch transitions plus rollback markers to keep the reader aligned.
            if primary.should_read_epoch_transition_messages() {
                match params_reader.read_with_rollbacks().await? {
                    RollbackWrapper::Normal((block_info, params)) => {
                        let mut state = state_mutex.lock().await;
                        state.volatile.start_new_epoch(block_info.number);
                        if let Some(shelley) = &params.params.shelley {
                            state.volatile.update_k(shelley.security_param);
                        }
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
            }

            // Process address deltas into volatile and persist to disk if a full epoch is out of rollback window
            if let Some(address_deltas_msg) = primary.message() {
                let block_info = primary.block_info().clone();
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
                    let compact_deltas = address_deltas_msg.as_compact_or_convert();
                    state.apply_address_deltas(compact_deltas.as_ref());

                    store = state.immutable.clone();
                    config = state.config.clone();
                    epoch = block_info.epoch;

                    // Move volatile deltas for an epoch to ImmutableAddressStore if out of rollback window
                    should_prune = state.ready_to_prune(&block_info);
                    if should_prune {
                        state.prune_volatile().await;
                    }
                }

                if should_prune {
                    if let Err(e) = persist_tx.send((epoch, store.clone(), config.clone())).await {
                        panic!("persistence worker crashed: {e}");
                    }
                }

                {
                    let mut state = state_mutex.lock().await;
                    state.volatile.next_block();
                }
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration flags and query topic
        let storage_config = AddressStorageConfig {
            db_path: get_string_flag(&config, DEFAULT_ADDRESS_DB_PATH),
            clear_on_start: get_bool_flag(&config, DEFAULT_CLEAR_ON_START),
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

        context.handle(&address_query_topic, move |message| {
            let state_mutex = state_mutex.clone();
            async move {
                let Message::StateQuery(StateQuery::Addresses(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Addresses(
                        AddressStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for address-state",
                        )),
                    )));
                };

                let state = state_mutex.lock().await;
                let response = match query {
                    AddressStateQuery::GetAddressUTxOs { address } => {
                        match state.get_address_utxos(address).await {
                            Ok(Some(utxos)) => AddressStateQueryResponse::AddressUTxOs(utxos),
                            Ok(None) => match address.to_string() {
                                Ok(addr_str) => {
                                    AddressStateQueryResponse::Error(QueryError::not_found(
                                        format!("Address {} not found", addr_str),
                                    ))
                                }
                                Err(e) => {
                                    AddressStateQueryResponse::Error(QueryError::internal_error(
                                        format!("Could not convert address to string: {}", e),
                                    ))
                                }
                            },
                            Err(e) => AddressStateQueryResponse::Error(QueryError::internal_error(
                                e.to_string(),
                            )),
                        }
                    }
                    AddressStateQuery::GetAddressTransactions { address } => {
                        match state.get_address_transactions(address).await {
                            Ok(Some(txs)) => AddressStateQueryResponse::AddressTransactions(txs),
                            Ok(None) => match address.to_string() {
                                Ok(addr_str) => {
                                    AddressStateQueryResponse::Error(QueryError::not_found(
                                        format!("Address {} not found", addr_str),
                                    ))
                                }
                                Err(e) => {
                                    AddressStateQueryResponse::Error(QueryError::internal_error(
                                        format!("Could not convert address to string: {}", e),
                                    ))
                                }
                            },
                            Err(e) => AddressStateQueryResponse::Error(QueryError::internal_error(
                                e.to_string(),
                            )),
                        }
                    }
                    AddressStateQuery::GetAddressTotals { address } => {
                        match state.get_address_totals(address).await {
                            Ok(totals) => AddressStateQueryResponse::AddressTotals(totals),
                            Err(e) => AddressStateQueryResponse::Error(QueryError::internal_error(
                                e.to_string(),
                            )),
                        }
                    }
                    AddressStateQuery::GetAddressesTotals { addresses } => {
                        match state.get_addresses_totals(addresses).await {
                            Ok(totals) => AddressStateQueryResponse::AddressesTotals(totals),
                            Err(e) => AddressStateQueryResponse::Error(QueryError::internal_error(
                                e.to_string(),
                            )),
                        }
                    }
                    AddressStateQuery::GetAddressesUTxOs { addresses } => {
                        match state.get_addresses_utxos(addresses).await {
                            Ok(utxos) => AddressStateQueryResponse::AddressesUTxOs(utxos),
                            Err(e) => AddressStateQueryResponse::Error(QueryError::internal_error(
                                e.to_string(),
                            )),
                        }
                    }
                };
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Addresses(
                    response,
                )))
            }
        });

        if storage_config.any_enabled() {
            // Subscribe to enabled topics
            let address_deltas_reader = AddressDeltasReader::new(&context, &config).await?;
            let params_reader = ParamsReader::new(&context, &config).await?;

            let is_snapshot_mode = StartupMode::from_config(config.as_ref()).is_snapshot();

            // Start run task
            context.run(async move {
                Self::run(
                    state_run,
                    address_deltas_reader,
                    params_reader,
                    is_snapshot_mode,
                )
                .await
                .unwrap_or_else(|e| error!("Failed: {e}"));
            });
        }

        Ok(())
    }
}
