use std::sync::Arc;

use acropolis_common::{
    configuration::get_string_flag,
    messages::{Message, StateQuery, StateQueryResponse},
    queries::{
        errors::QueryError,
        stake_deltas::{
            StakeDeltaQuery, StakeDeltaQueryResponse, DEFAULT_STAKE_DELTAS_QUERY_TOPIC,
        },
    },
    state_history::StateHistory,
};
use caryatid_sdk::Context;
use config::Config;
use tokio::sync::Mutex;
use tracing::info;

use crate::{state::State, utils::PointerCache};

/// Register a query handler that resolves pointer addresses using the given cache.
pub fn register_query_handler(
    context: &Arc<Context<Message>>,
    config: &Arc<Config>,
    cache: Arc<PointerCache>,
) {
    let query_topic = config
        .get_string(DEFAULT_STAKE_DELTAS_QUERY_TOPIC.0)
        .unwrap_or(DEFAULT_STAKE_DELTAS_QUERY_TOPIC.1.to_string());
    info!("Registering query handler on '{query_topic}'");

    context.handle(&query_topic, move |message| {
        let cache = cache.clone();
        async move {
            let Message::StateQuery(StateQuery::StakeDeltas(query)) = message.as_ref() else {
                return Arc::new(Message::StateQueryResponse(
                    StateQueryResponse::StakeDeltas(StakeDeltaQueryResponse::Error(
                        QueryError::internal_error("Invalid message for stake-delta-filter"),
                    )),
                ));
            };

            let response = match query {
                StakeDeltaQuery::ResolvePointers { pointers } => {
                    let mut resolved = std::collections::HashMap::new();
                    for ptr in pointers {
                        if let Some(Some(stake_addr)) = cache.decode_pointer(ptr) {
                            resolved.insert(ptr.clone(), stake_addr.clone());
                        }
                    }
                    StakeDeltaQueryResponse::ResolvedPointers(resolved)
                }
            };

            Arc::new(Message::StateQueryResponse(
                StateQueryResponse::StakeDeltas(response),
            ))
        }
    });
}

/// Register a query handler for stateful mode, where the cache is behind a Mutex.
pub fn register_query_handler_stateful(
    context: &Arc<Context<Message>>,
    config: &Arc<Config>,
    history: Arc<Mutex<StateHistory<State>>>,
) {
    let query_topic = get_string_flag(config, DEFAULT_STAKE_DELTAS_QUERY_TOPIC);
    info!("Registering stateful query handler on '{query_topic}'");

    context.handle(&query_topic, move |message| {
        let history = history.clone();
        async move {
            let Message::StateQuery(StateQuery::StakeDeltas(query)) = message.as_ref() else {
                return Arc::new(Message::StateQueryResponse(
                    StateQueryResponse::StakeDeltas(StakeDeltaQueryResponse::Error(
                        QueryError::internal_error("Invalid message for stake-delta-filter"),
                    )),
                ));
            };

            let locked = history.lock().await;
            let state = match locked.current() {
                Some(state) => state,
                None => {
                    return Arc::new(Message::StateQueryResponse(
                        StateQueryResponse::StakeDeltas(StakeDeltaQueryResponse::Error(
                            QueryError::internal_error("Invalid message for stake-delta-filter"),
                        )),
                    ))
                }
            };

            let response = match query {
                StakeDeltaQuery::ResolvePointers { pointers } => {
                    let mut resolved = std::collections::HashMap::new();
                    for ptr in pointers {
                        if let Some(Some(stake_addr)) = state.pointer_cache.decode_pointer(ptr) {
                            resolved.insert(ptr.clone(), stake_addr.clone());
                        }
                    }
                    StakeDeltaQueryResponse::ResolvedPointers(resolved)
                }
            };

            Arc::new(Message::StateQueryResponse(
                StateQueryResponse::StakeDeltas(response),
            ))
        }
    });
}
