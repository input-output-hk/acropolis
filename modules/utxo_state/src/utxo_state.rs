//! Acropolis UTXO state module for Caryatid
//! Accepts UTXO events and derives the current ledger state in memory

use acropolis_common::{
    caryatid::{RollbackAwarePublisher, RollbackWrapper, ValidationContext},
    configuration::{get_bool_flag, get_string_flag, StartupMode},
    declare_cardano_reader,
    messages::{
        CardanoMessage, GenesisCompleteMessage, Message, PoolRegistrationUpdatesMessage,
        ProtocolParamsMessage, SnapshotMessage, SnapshotStateMessage,
        StakeRegistrationUpdatesMessage, StateQuery, StateQueryResponse, StateTransitionMessage,
        UTXODeltasMessage,
    },
    queries::utxos::{UTxOStateQuery, UTxOStateQueryResponse, DEFAULT_UTXOS_QUERY_TOPIC},
    Pots,
};
use caryatid_sdk::{module, Context, Subscription};

use acropolis_common::queries::errors::QueryError;
use anyhow::{anyhow, bail, Result};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod state;
use state::{ImmutableUTXOStore, State};
mod address_delta_mode;
use address_delta_mode::AddressDeltaPublishMode;
mod reference_scripts_state;

#[cfg(test)]
mod test_utils;

mod address_delta_publisher;
mod volatile_index;
use address_delta_publisher::AddressDeltaPublisher;
mod block_totals_publisher;
use block_totals_publisher::BlockTotalsPublisher;
mod in_memory_immutable_utxo_store;
use in_memory_immutable_utxo_store::InMemoryImmutableUTXOStore;
mod dashmap_immutable_utxo_store;
use dashmap_immutable_utxo_store::DashMapImmutableUTXOStore;
mod sled_immutable_utxo_store;
use sled_immutable_utxo_store::SledImmutableUTXOStore;
mod fjall_immutable_utxo_store;
use fjall_immutable_utxo_store::FjallImmutableUTXOStore;
mod fake_immutable_utxo_store;
use fake_immutable_utxo_store::FakeImmutableUTXOStore;

use crate::reference_scripts_state::ReferenceScriptsState;
mod utils;
pub mod validations;

declare_cardano_reader!(
    GenesisCompleteReader,
    "bootstrapped-subscribe-topic",
    "cardano.sequence.bootstrapped",
    GenesisComplete,
    GenesisCompleteMessage
);
declare_cardano_reader!(
    UTxODeltasReader,
    "utxo-deltas-subscribe-topic",
    "cardano.utxo.deltas",
    UTXODeltas,
    UTXODeltasMessage
);
declare_cardano_reader!(
    ParamsReader,
    "protocol-parameters-subscribe-topic",
    "cardano.protocol.parameters",
    ProtocolParams,
    ProtocolParamsMessage
);
declare_cardano_reader!(
    StakeUpdatesReader,
    "stake-registration-updates-subscribe-topic",
    "cardano.stake.registration.updates",
    StakeRegistrationUpdates,
    StakeRegistrationUpdatesMessage
);
declare_cardano_reader!(
    PoolUpdatesReader,
    "pool-registration-updates-subscribe-topic",
    "cardano.pool.registration.updates",
    PoolRegistrationUpdates,
    PoolRegistrationUpdatesMessage
);
declare_cardano_reader!(
    PotsReader,
    "pots-subscribe-topic",
    "cardano.pots",
    Pots,
    Pots
);

const DEFAULT_STORE: (&str, &str) = ("store", "memory");
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");
const DEFAULT_UTXO_VALIDATION_TOPIC: (&str, &str) =
    ("utxo-validation-publish-topic", "cardano.validation.utxo");
const DEFAULT_ADDRESS_DELTA_PUBLISH_MODE: (&str, &str) = ("address-delta-publish-mode", "compact");
/// Whether to publish per-tx phase-2 evaluation results on the bus. Default `false`
/// — operators opt in by setting this to `true` in the `[module.utxo-state]`
/// section of the process config. When `false`, no work is done beyond the
/// existing validation path.
const DEFAULT_PUBLISH_PHASE2_RESULTS: (&str, bool) = ("publish-phase2-results", false);
/// Topic on which to publish phase-2 evaluation results when the flag is on.
const DEFAULT_PUBLISH_PHASE2_TOPIC: (&str, &str) = ("publish-phase2-topic", "cardano.utxo.phase2");

/// Resolve the optional phase-2 publish topic from a module config.
///
/// `None` means the feature is off — the caller MUST short-circuit before
/// constructing or publishing any [`crate::messages::Phase2EvaluationResultsMessage`].
/// `Some(topic)` is the configured topic to publish on.
///
/// Extracted as a free function so the toggle behaviour is unit-testable.
pub fn resolve_publish_phase2_topic(config: &Config) -> Option<String> {
    if get_bool_flag(config, DEFAULT_PUBLISH_PHASE2_RESULTS) {
        Some(get_string_flag(config, DEFAULT_PUBLISH_PHASE2_TOPIC))
    } else {
        None
    }
}

pub(crate) async fn publish_observer_message(
    publisher: &Option<Mutex<RollbackAwarePublisher<Message>>>,
    message: Arc<Message>,
    error_context: &str,
) {
    if let Some(publisher) = publisher {
        publisher
            .lock()
            .await
            .publish(message)
            .await
            .unwrap_or_else(|e| error!("{error_context}: {e}"));
    }
}

/// UTXO state module
#[module(
    message_type(Message),
    name = "utxo-state",
    description = "In-memory UTXO state from UTXO events"
)]
pub struct UTXOState;

impl UTXOState {
    /// Main run function
    #[allow(clippy::too_many_arguments)]
    async fn run(
        context: Arc<Context<Message>>,
        state: Arc<Mutex<State>>,
        mut bootstrapped_reader: GenesisCompleteReader,
        mut utxo_deltas_reader: UTxODeltasReader,
        mut params_reader: ParamsReader,
        mut pool_updates_reader: Option<PoolUpdatesReader>,
        mut stake_updates_reader: Option<StakeUpdatesReader>,
        mut pots_reader: PotsReader,
        publish_tx_validation_topic: String,
        publish_phase2_topic: Option<String>,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        let genesis_values = match bootstrapped_reader.read_with_rollbacks().await? {
            RollbackWrapper::Normal((block_info, genesis_complete)) => {
                info!(
                    "Received genesis complete message at block {}",
                    block_info.number
                );
                genesis_complete.values.clone()
            }
            _ => {
                bail!("Failed to read genesis complete message");
            }
        };

        if !is_snapshot_mode {
            pots_reader.read_with_rollbacks().await?;
            params_reader.read_with_rollbacks().await?;
        }

        let mut bootstrap_block_processed = false;

        loop {
            let mut ctx =
                ValidationContext::new(&context, &publish_tx_validation_topic, "utxo_state");

            let deltas_msg = match ctx.consume(
                "utxo_deltas_reader",
                utxo_deltas_reader.read_with_rollbacks().await,
            )? {
                RollbackWrapper::Normal((block_info, deltas)) => Some((block_info, deltas)),
                RollbackWrapper::Rollback((block_info, message)) => {
                    // Publish rollbacks downstream
                    let mut state = state.lock().await;
                    state.handle_rollback(&block_info, message).await;

                    None
                }
            };
            let is_new_epoch =
                deltas_msg.as_ref().map(|(b, _)| b.new_epoch && b.epoch > 0).unwrap_or(true);

            let mut current_protocol_params = state.lock().await.get_or_init_protocol_parameters();

            // Read protocol parameters and pots if new epoch
            if is_new_epoch {
                match ctx.consume("params_reader", params_reader.read_with_rollbacks().await)? {
                    RollbackWrapper::Normal((_, params)) => {
                        current_protocol_params = params.params.clone();
                    }
                    RollbackWrapper::Rollback(_) => {}
                }

                match ctx.consume("pots_reader", pots_reader.read_with_rollbacks().await)? {
                    RollbackWrapper::Normal((_, pots)) => {
                        state.lock().await.handle_pots(pots.as_ref().clone());
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
            }

            // Read from pool registration updates subscription if available
            let mut pool_registration_updates = vec![];
            if is_snapshot_mode || bootstrap_block_processed {
                if let Some(reader) = pool_updates_reader.as_mut() {
                    match ctx.consume("pool_updates_reader", reader.read_with_rollbacks().await)? {
                        RollbackWrapper::Normal((_, updates_msg)) => {
                            pool_registration_updates = updates_msg.updates.clone();
                        }
                        RollbackWrapper::Rollback(_) => {}
                    }
                }
            }

            // Read from stake registration updates subscription if available
            let mut stake_registration_updates = vec![];
            if is_snapshot_mode || bootstrap_block_processed {
                if let Some(reader) = stake_updates_reader.as_mut() {
                    match ctx.consume("stake_updates_reader", reader.read_with_rollbacks().await)? {
                        RollbackWrapper::Normal((_, updates_msg)) => {
                            stake_registration_updates = updates_msg.updates.clone();
                        }
                        RollbackWrapper::Rollback(_) => {}
                    }
                }
            }

            // Validate UTxODeltas
            // before applying them
            if let Some((block, deltas_msg)) = deltas_msg.as_ref() {
                let span = info_span!("utxo_state.validate", block = block.number);
                if block.intent.do_validation() {
                    async {
                        let mut state = state.lock().await;

                        let (phase2_messages, result) = state
                            .validate(
                                block,
                                deltas_msg,
                                &pool_registration_updates,
                                &stake_registration_updates,
                                &current_protocol_params,
                                &genesis_values,
                            )
                            .await;

                        // Publish per-tx phase-2 outcomes if and only if the
                        // feature is enabled. The early-out on `None` avoids
                        // any per-tx allocation/clone when off (SC-005).
                        if let Some(topic) = publish_phase2_topic.as_deref() {
                            for msg in phase2_messages {
                                let payload = Arc::new(Message::Cardano((
                                    block.as_ref().clone(),
                                    CardanoMessage::Phase2EvaluationResults(msg),
                                )));
                                if let Err(e) = context.message_bus.publish(topic, payload).await {
                                    error!("phase-2 publish on '{topic}' failed: {e}");
                                }
                            }
                        }

                        ctx.handle("validate", result.map_err(|e| e.into()));

                        ctx.publish().await;
                    }
                    .instrument(span)
                    .await;
                }

                let span = info_span!("utxo_state.handle", block = block.number);
                async {
                    let mut state = state.lock().await;
                    state
                        .handle_utxo_deltas(block, deltas_msg)
                        .await
                        .inspect_err(|e| error!("Messaging handling error: {e}"))
                        .ok();
                }
                .instrument(span)
                .await;

                if !bootstrap_block_processed {
                    bootstrap_block_processed = true;
                }
            }

            // Commit protocol paramemters
            if let Some((block_info, _)) = deltas_msg {
                state
                    .lock()
                    .await
                    .commit_protocol_parameters(&block_info, current_protocol_params.clone());
            }
        }
    }

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // These registration updates subscriptions are only needed for validation
        let pool_registration_updates_subscribe_topic =
            config.get_string("pool-registration-updates-subscribe-topic").ok();
        if let Some(ref topic) = pool_registration_updates_subscribe_topic {
            info!("Creating pool registration updates subscriber on '{topic}'");
        }
        let stake_registration_updates_subscribe_topic =
            config.get_string("stake-registration-updates-subscribe-topic").ok();
        if let Some(ref topic) = stake_registration_updates_subscribe_topic {
            info!("Creating stake registration updates subscriber on '{topic}'");
        }

        // Subscribers
        let bootstrapped_reader = GenesisCompleteReader::new(&context, &config).await?;
        let pool_updates_reader = if pool_registration_updates_subscribe_topic.is_some() {
            Some(PoolUpdatesReader::new(&context, &config).await?)
        } else {
            None
        };
        let stake_updates_reader = if stake_registration_updates_subscribe_topic.is_some() {
            Some(StakeUpdatesReader::new(&context, &config).await?)
        } else {
            None
        };
        let utxo_deltas_reader = UTxODeltasReader::new(&context, &config).await?;
        let params_reader = ParamsReader::new(&context, &config).await?;
        let pots_reader = PotsReader::new(&context, &config).await?;

        let snapshot_topic = get_string_flag(&config, DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC);
        info!("Creating snapshot subscriber on '{snapshot_topic}'");

        let utxos_query_topic = get_string_flag(&config, DEFAULT_UTXOS_QUERY_TOPIC);

        let utxo_validation_publish_topic = get_string_flag(&config, DEFAULT_UTXO_VALIDATION_TOPIC);
        info!("Creating UTxO validation publisher on '{utxo_validation_publish_topic}'");

        // Optional phase-2 results publisher. `None` = feature off (default).
        let publish_phase2_topic = resolve_publish_phase2_topic(&config);
        if let Some(ref topic) = publish_phase2_topic {
            info!("Creating phase-2 evaluation publisher on '{topic}'");
        }

        let address_delta_publish_mode =
            get_string_flag(&config, DEFAULT_ADDRESS_DELTA_PUBLISH_MODE)
                .parse::<AddressDeltaPublishMode>()?;
        info!(
            mode = ?address_delta_publish_mode,
            "Address delta publish mode"
        );

        let is_snapshot_mode = StartupMode::from_config(config.as_ref()).is_snapshot();

        // Create store
        let store_type = get_string_flag(&config, DEFAULT_STORE);
        let store: Arc<dyn ImmutableUTXOStore> = match store_type.as_str() {
            "memory" => Arc::new(InMemoryImmutableUTXOStore::new(config.clone())),
            "dashmap" => Arc::new(DashMapImmutableUTXOStore::new(config.clone())),
            "sled" => Arc::new(SledImmutableUTXOStore::new(config.clone())?),
            "fjall" => Arc::new(FjallImmutableUTXOStore::new(config.clone())?),
            "fake" => Arc::new(FakeImmutableUTXOStore::new(config.clone())),
            _ => return Err(anyhow!("Unknown store type {store_type}")),
        };
        let snapshot_store = store.clone();
        let mut state = State::new(store, address_delta_publish_mode);

        // Create address delta publisher and pass it observations
        let deltas_publisher =
            AddressDeltaPublisher::new(context.clone(), config.clone(), address_delta_publish_mode);
        state.register_address_delta_observer(Arc::new(deltas_publisher));

        // Create block totals publisher and pass it observations
        let totals_publisher = BlockTotalsPublisher::new(context.clone(), config);
        state.register_block_totals_observer(Arc::new(totals_publisher));

        let state = Arc::new(Mutex::new(state));

        let state_run = state.clone();
        let context_run = context.clone();
        context.run(async move {
            Self::run(
                context_run,
                state_run,
                bootstrapped_reader,
                utxo_deltas_reader,
                params_reader,
                pool_updates_reader,
                stake_updates_reader,
                pots_reader,
                utxo_validation_publish_topic,
                publish_phase2_topic,
                is_snapshot_mode,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        // Subscribe for snapshot messages
        {
            let state_snapshot = state.clone();
            let mut reference_scripts = ReferenceScriptsState::default();
            let mut subscription = context.subscribe(&snapshot_topic).await?;
            let context = context.clone();
            let store = snapshot_store.clone();
            enum SnapshotState {
                Preparing,
                Started,
            }
            let mut snapshot_state = SnapshotState::Preparing;
            let mut total_utxos_received = 0u64;
            let mut batch_count = 0u64;
            context.run(async move {
                loop {
                    let Ok((_, message)) = subscription.read().await else {
                        return;
                    };

                    match message.as_ref() {
                        Message::Snapshot(SnapshotMessage::Startup) => {
                            info!("UTXO state received Snapshot Startup message");
                            match snapshot_state {
                                SnapshotState::Preparing => snapshot_state = SnapshotState::Started,
                                _ => error!("Snapshot Startup message received but we have already left preparing state"),
                            }
                        }
                        Message::Snapshot(SnapshotMessage::Bootstrap(
                            SnapshotStateMessage::UTxOPartialState(utxo_state),
                        )) => {
                            let batch_size = utxo_state.utxos.len();
                            batch_count += 1;
                            total_utxos_received += batch_size as u64;

                            if batch_count == 1 {
                                info!("UTXO state received first UTxO batch with {} UTxOs", batch_size);
                            } else if batch_count.is_multiple_of(100) {
                                info!("UTXO state received {} batches, {} total UTxOs so far", batch_count, total_utxos_received);
                            }

                            for (key, value, reference_script) in &utxo_state.utxos {
                                if store.add_utxo(*key, value.clone()).await.is_err() {
                                    error!("Failed to add snapshot utxo to state store");
                                }

                                if let (Some(script_ref), Some(reference_script)) = (value.script_ref.as_ref(), reference_script.as_ref()) {
                                    reference_scripts.apply_reference_scripts(&[], &[(script_ref.script_hash, reference_script.clone())]);
                                }
                            }
                        }
                        Message::Snapshot(SnapshotMessage::Complete) => {
                            info!("UTXO state snapshot complete: {} UTxOs in {} batches", total_utxos_received, batch_count);
                            state_snapshot.lock().await.commit_reference_scripts(0, reference_scripts);

                            return;
                        }
                        _ => {}
                    }
                }
            });
        }

        // Query handler
        let state_query = state.clone();
        context.handle(&utxos_query_topic, move |message| {
            let state_mutex = state_query.clone();
            async move {
                let Message::StateQuery(StateQuery::UTxOs(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::UTxOs(
                        UTxOStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for utxo-state",
                        )),
                    )));
                };

                let mut state = state_mutex.lock().await;
                let response = match query {
                    UTxOStateQuery::GetUTxOsSum { utxo_identifiers } => {
                        match state.get_utxos_sum(utxo_identifiers).await {
                            Ok(balance) => UTxOStateQueryResponse::UTxOsSum(balance),
                            Err(e) => UTxOStateQueryResponse::Error(QueryError::internal_error(
                                e.to_string(),
                            )),
                        }
                    }
                    UTxOStateQuery::GetUTxOs { utxo_identifiers } => {
                        match state.get_utxo_entries(utxo_identifiers).await {
                            Ok(values) => UTxOStateQueryResponse::UTxOs(values),
                            Err(e) => UTxOStateQueryResponse::Error(QueryError::internal_error(
                                e.to_string(),
                            )),
                        }
                    }
                    UTxOStateQuery::GetAllUTxOsSumAtShelleyStart => {
                        let total_lovelace = match state.get_lovelace_at_shelley_start() {
                            Some(cached) => cached,
                            None => match state.get_total_lovelace().await {
                                Ok(v) => v,
                                Err(e) => {
                                    return Arc::new(Message::StateQueryResponse(
                                        StateQueryResponse::UTxOs(UTxOStateQueryResponse::Error(
                                            QueryError::internal_error(e.to_string()),
                                        )),
                                    ));
                                }
                            },
                        };
                        UTxOStateQueryResponse::LovelaceSum(total_lovelace)
                    }
                    UTxOStateQuery::GetAvvmCancelledValue => {
                        if state.get_avvm_cancelled_value().is_none() {
                            if let Err(e) = state.cancel_redeem_utxos().await {
                                error!("Failed to cancel AVVM UTxOs on query: {e}");
                            }
                        }
                        UTxOStateQueryResponse::AvvmCancelledValue(state.get_avvm_cancelled_value())
                    }
                    UTxOStateQuery::GetPointerAddressValues => {
                        if state.get_pointer_address_values().is_none() {
                            if let Err(e) = state.compute_pointer_address_values().await {
                                error!("Failed to compute pointer address values: {e}");
                            }
                        }
                        match state.get_pointer_address_values() {
                            Some(values) => {
                                UTxOStateQueryResponse::PointerAddressValues(values.clone())
                            }
                            None => UTxOStateQueryResponse::PointerAddressValues(
                                std::collections::HashMap::new(),
                            ),
                        }
                    }
                };
                Arc::new(Message::StateQueryResponse(StateQueryResponse::UTxOs(
                    response,
                )))
            }
        });

        // Ticker to log stats and prune state
        let state2 = state.clone();
        let mut subscription = context.subscribe("clock.tick").await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        let span = info_span!("utxo_state.tick", number = message.number);
                        async {
                            state2
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

#[cfg(test)]
mod publish_phase2_toggle_tests {
    use super::{resolve_publish_phase2_topic, DEFAULT_PUBLISH_PHASE2_RESULTS};
    use config::{Config, FileFormat};

    fn config_from(toml: &str) -> Config {
        Config::builder()
            .add_source(config::File::from_str(toml, FileFormat::Toml))
            .build()
            .expect("config")
    }

    #[test]
    fn default_is_off() {
        // The default tuple itself documents the default. (T031.)
        assert!(
            !DEFAULT_PUBLISH_PHASE2_RESULTS.1,
            "publish-phase2-results default must be false"
        );
    }

    #[test]
    fn missing_flag_resolves_to_none() {
        // T029: when the operator does not set publish-phase2-results, the
        // helper returns None — the publish loop in run() takes the
        // single-cmp early-out and constructs no messages.
        let cfg = config_from("");
        assert!(resolve_publish_phase2_topic(&cfg).is_none());
    }

    #[test]
    fn flag_false_resolves_to_none() {
        // T029: explicit false has the same effect as missing.
        let cfg = config_from("publish-phase2-results = false");
        assert!(resolve_publish_phase2_topic(&cfg).is_none());
    }

    #[test]
    fn flag_true_resolves_to_default_topic() {
        // T030 corollary: flipping the flag does not change validation; it
        // only changes whether a publish topic is returned. The default topic
        // matches the contract.
        let cfg = config_from("publish-phase2-results = true");
        let topic = resolve_publish_phase2_topic(&cfg);
        assert_eq!(topic.as_deref(), Some("cardano.utxo.phase2"));
    }

    #[test]
    fn flag_true_with_custom_topic_is_honoured() {
        let cfg = config_from(
            r#"
                publish-phase2-results = true
                publish-phase2-topic = "custom.phase2"
            "#,
        );
        let topic = resolve_publish_phase2_topic(&cfg);
        assert_eq!(topic.as_deref(), Some("custom.phase2"));
    }
}
