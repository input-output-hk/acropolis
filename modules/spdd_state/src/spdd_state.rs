//! Acropolis SPDD state module for Caryatid
//! Stores historical stake pool delegation distributions
use acropolis_common::caryatid::{PrimaryRead, RollbackWrapper};
use acropolis_common::declare_cardano_reader;
use acropolis_common::messages::SPOStakeDistributionMessage;
use acropolis_common::queries::errors::QueryError;
use acropolis_common::state_history::{StateHistory, StateHistoryStore, StoreType};
use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse, StateTransitionMessage},
    queries::spdd::{SPDDStateQuery, SPDDStateQueryResponse, DEFAULT_SPDD_QUERY_TOPIC},
    rest_helper::handle_rest_with_query_parameters,
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, info_span, Instrument};

mod state;
use state::State;
mod rest;
use rest::handle_spdd;

const DEFAULT_HANDLE_SPDD_TOPIC: (&str, &str) = ("handle-topic-spdd", "rest.get.spdd");
const DEFAULT_STORE_SPDD: (&str, bool) = ("store-spdd", false);

declare_cardano_reader!(
    SPDDReader,
    "stake-pool-distribution-subscribe-topic",
    "cardano.spo.distribution",
    SPOStakeDistribution,
    SPOStakeDistributionMessage
);

/// SPDD State module
#[module(
    message_type(Message),
    name = "spdd-state",
    description = "Stake Pool Delegation Distribution State Tracker"
)]

pub struct SPDDState;

impl SPDDState {
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut spdd_reader: SPDDReader,
    ) -> anyhow::Result<()> {
        loop {
            let mut state = history.lock().await.get_or_init_with(State::new);

            let primary = PrimaryRead::from_read(spdd_reader.read_with_rollbacks().await?);

            if primary.is_rollback() {
                state = history.lock().await.get_rolled_back_state(primary.block_info().epoch);
            }

            if let Some(msg) = primary.message() {
                let span = info_span!("spdd_state.handle", epoch = msg.epoch);
                async {
                    state.apply_spdd_snapshot(msg.spos.iter().map(|(k, v)| (*k, *v)));
                }
                .instrument(span)
                .await;

                history.lock().await.commit(primary.block_info().epoch, state);
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration

        // REST topic (not included in BF)
        let handle_spdd_topic = config
            .get_string(DEFAULT_HANDLE_SPDD_TOPIC.0)
            .unwrap_or(DEFAULT_HANDLE_SPDD_TOPIC.1.to_string());
        info!("Creating request handler on '{}'", handle_spdd_topic);

        // Query topic
        let spdd_query_topic = config
            .get_string(DEFAULT_SPDD_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_SPDD_QUERY_TOPIC.1.to_string());
        info!("Creating query handler on '{}'", spdd_query_topic);

        let store_spdd = config.get_bool(DEFAULT_STORE_SPDD.0).unwrap_or(DEFAULT_STORE_SPDD.1);

        let history_opt = if store_spdd {
            let history = Arc::new(Mutex::new(StateHistory::<State>::new(
                "spdd_state",
                StateHistoryStore::Unbounded,
                &config,
                StoreType::Epoch,
            )));

            // Register /spdd REST endpoint
            let history_rest = history.clone();
            handle_rest_with_query_parameters(context.clone(), &handle_spdd_topic, move |params| {
                handle_spdd(history_rest.clone(), params)
            });

            // Subscribe for spdd messages from accounts_state
            let history_handler = history.clone();
            let spdd_reader = SPDDReader::new(&context, &config).await?;

            context.run(Self::run(history_handler, spdd_reader));

            // Ticker to log stats
            let mut tick_subscription = context.subscribe("clock.tick").await?;
            let history_logger = history.clone();
            context.run(async move {
                loop {
                    let Ok((_, message)) = tick_subscription.read().await else {
                        return;
                    };

                    if let Message::Clock(clock) = message.as_ref() {
                        if clock.number % 60 == 0 {
                            let span = info_span!("spdd_state.tick", number = clock.number);
                            async {
                                let history = history_logger.lock().await;
                                let current_opt = history.current();

                                if let Some(current) = current_opt {
                                    current.tick(history.len());
                                }
                            }
                            .instrument(span)
                            .await;
                        }
                    }
                }
            });
            Some(history)
        } else {
            None
        };

        // handle spdd query
        let history_query = history_opt.clone();
        context.handle(&spdd_query_topic, move |message| {
            let history_query = history_query.clone();
            async move {
                let Message::StateQuery(StateQuery::SPDD(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::SPDD(
                        SPDDStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for spdd-state",
                        )),
                    )));
                };

                let history = match history_query {
                    Some(history) => history,
                    None => {
                        return Arc::new(Message::StateQueryResponse(StateQueryResponse::SPDD(
                            SPDDStateQueryResponse::Error(QueryError::storage_disabled("SPDD")),
                        )))
                    }
                };

                let locked = history.lock().await;

                let response = match query {
                    SPDDStateQuery::GetEpochTotalActiveStakes { epoch } => {
                        // Since this is active stakes we plus 2 to epoch number
                        let active_stake = match locked.get_by_index(*epoch - 2) {
                            Some(state) => state.get_total_active_stakes(),
                            None => 0,
                        };
                        SPDDStateQueryResponse::EpochTotalActiveStakes(active_stake)
                    }
                    SPDDStateQuery::GetEpochSPDD { epoch } => SPDDStateQueryResponse::EpochSPDD(
                        locked
                            .get_by_index(*epoch + 1)
                            .map(|map| {
                                map.get_latest()
                                    .iter()
                                    .map(|(pool_id, stake)| (*pool_id, stake.active))
                                    .collect()
                            })
                            .unwrap_or_default(),
                    ),
                };

                Arc::new(Message::StateQueryResponse(StateQueryResponse::SPDD(
                    response,
                )))
            }
        });

        Ok(())
    }
}
