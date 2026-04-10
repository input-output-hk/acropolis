mod stores;

use crate::queries::{handle_blocks_query, handle_txs_query};
use crate::state::State;
use crate::stores::{fjall::FjallStore, Store};

use acropolis_common::configuration::get_string_flag;
use acropolis_common::queries::errors::QueryError;
use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper, ValidationContext},
    declare_cardano_reader,
    messages::{
        CardanoMessage, Message, ProtocolParamsMessage, RawBlockMessage, StateQuery,
        StateQueryResponse, StateTransitionMessage,
    },
    queries::blocks::{BlocksStateQueryResponse, DEFAULT_BLOCKS_QUERY_TOPIC},
    queries::transactions::{TransactionsStateQueryResponse, DEFAULT_TRANSACTIONS_QUERY_TOPIC},
    state_history::{StateHistory, StateHistoryStore},
    NetworkId,
};
use anyhow::{bail, Result};
use caryatid_sdk::message_bus::Subscription;
use caryatid_sdk::{module, Context};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::info;

mod helpers;
mod queries;
mod state;

const DEFAULT_STORE: (&str, &str) = ("store", "fjall");
const DEFAULT_VALIDATION_OUTCOME_PUBLISH_TOPIC: (&str, &str) =
    ("validation-publish-topic", "cardano.validation.chainstore");

declare_cardano_reader!(
    BlocksReader,
    "blocks-subscribe-topic",
    "cardano.block.available",
    BlockAvailable,
    RawBlockMessage
);

declare_cardano_reader!(
    ParamsReader,
    "protocol-parameters-subscribe-topic",
    "cardano.protocol.parameters",
    ProtocolParams,
    ProtocolParamsMessage
);

#[module(
    message_type(Message),
    name = "chain-store",
    description = "Block and TX state"
)]
pub struct ChainStore;

impl ChainStore {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let block_queries_topic = get_string_flag(&config, DEFAULT_BLOCKS_QUERY_TOPIC);
        let txs_queries_topic = get_string_flag(&config, DEFAULT_TRANSACTIONS_QUERY_TOPIC);
        let validation_topic = get_string_flag(&config, DEFAULT_VALIDATION_OUTCOME_PUBLISH_TOPIC);
        info!("Publishing validation outcomes on '{validation_topic}'");

        let network_id: NetworkId =
            config.get_string("network-id").unwrap_or("mainnet".to_string()).into();

        let store_type = get_string_flag(&config, DEFAULT_STORE);
        let store: Arc<dyn Store> = match store_type.as_str() {
            "fjall" => Arc::new(FjallStore::new(config.clone())?),
            _ => bail!("Unknown store type {store_type}"),
        };

        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "chain_store",
            StateHistoryStore::default_epoch_store(),
        )));
        history.lock().await.commit_forced(State::new());

        let query_store = store.clone();
        let query_history = history.clone();
        context.handle(&block_queries_topic, move |req| {
            let query_store = query_store.clone();
            let query_history = query_history.clone();
            async move {
                let Message::StateQuery(StateQuery::Blocks(query)) = req.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Blocks(
                        BlocksStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for blocks-state",
                        )),
                    )));
                };
                let Some(state) = query_history.lock().await.current().cloned() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Blocks(
                        BlocksStateQueryResponse::Error(QueryError::internal_error(
                            "uninitialized state",
                        )),
                    )));
                };
                let res = handle_blocks_query(&query_store, &state, query).unwrap_or_else(|err| {
                    BlocksStateQueryResponse::Error(QueryError::internal_error(err.to_string()))
                });
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Blocks(res)))
            }
        });

        let query_store = store.clone();
        context.handle(&txs_queries_topic, move |req| {
            let query_store = query_store.clone();
            let network_id = network_id.clone();
            async move {
                let Message::StateQuery(StateQuery::Transactions(query)) = req.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(
                        StateQueryResponse::Transactions(TransactionsStateQueryResponse::Error(
                            QueryError::internal_error("Invalid message for txs-state"),
                        )),
                    ));
                };
                let res = handle_txs_query(&query_store, query, network_id).unwrap_or_else(|err| {
                    TransactionsStateQueryResponse::Error(QueryError::internal_error(
                        err.to_string(),
                    ))
                });
                Arc::new(Message::StateQueryResponse(
                    StateQueryResponse::Transactions(res),
                ))
            }
        });

        let mut params_reader = ParamsReader::new(&context, &config).await?;
        let mut blocks_reader = BlocksReader::new(&context, &config).await?;
        let run_ctx = context.clone();

        context.run::<Result<(), anyhow::Error>, _>(async move {
            match blocks_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal((block_info, block)) => {
                    if let Err(err) =
                        State::handle_first_block(&store, block_info.as_ref(), block.as_ref())
                    {
                        panic!("Corrupted DB: {err}")
                    }
                }
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading blocks");
                }
            }
            match params_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal(_) => {}
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial params");
                }
            }

            loop {
                let mut ctx = ValidationContext::new(&run_ctx, &validation_topic, "chain_store");

                let mut state = history.lock().await.get_or_init_with(State::new);
                let primary = PrimaryRead::from_sync(
                    &mut ctx,
                    "blocks_reader",
                    blocks_reader.read_with_rollbacks().await,
                )?;

                if primary.is_rollback() {
                    let mut history = history.lock().await;
                    state = history.get_rolled_back_state(primary.block_info().epoch);

                    // Keep the persisted store on the same rewind boundary as StateHistory.
                    store.rollback(primary.block_info())?;
                }

                if let Some(block) = primary.message() {
                    ctx.handle(
                        "handle_new_block",
                        State::handle_new_block(
                            &store,
                            primary.block_info().as_ref(),
                            block.as_ref(),
                        ),
                    );
                }

                // Epoch-0 params are consumed during init, so the loop only syncs
                // the params reader on rollbacks and real epoch transitions.
                if primary.should_read_epoch_transition_messages() {
                    match ctx
                        .consume_sync("params_reader", params_reader.read_with_rollbacks().await)?
                    {
                        RollbackWrapper::Normal((_, params)) => {
                            state.handle_new_params(params.as_ref());
                        }
                        RollbackWrapper::Rollback(_) => {}
                    }
                }

                // Commit state on epoch transition
                if let Some(epoch) = primary.epoch() {
                    let mut history = history.lock().await;
                    history.commit(epoch, state);
                }
            }
        });

        Ok(())
    }
}
