//! Acropolis DRep State module for Caryatid
//! Accepts certificate events and derives the DRep State in memory

use acropolis_common::queries::errors::QueryError;
use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::governance::{
        DRepDelegatorAddresses, DRepInfo, DRepInfoWithDelegators, DRepUpdates, DRepVotes,
        DRepsList, GovernanceStateQuery, GovernanceStateQueryResponse,
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

mod state;
use state::State;
mod drep_state_publisher;
use drep_state_publisher::DRepStatePublisher;

use crate::state::DRepStorageConfig;

// Subscription topics
const DEFAULT_CERTIFICATES_SUBSCRIBE_TOPIC: (&str, &str) =
    ("certificates-subscribe-topic", "cardano.certificates");
const DEFAULT_GOVERNANCE_SUBSCRIBE_TOPIC: (&str, &str) =
    ("governance-subscribe-topic", "cardano.governance");
const DEFAULT_PARAMETERS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("parameters-subscribe-topic", "cardano.protocol.parameters");

// Publisher topic
const DEFAULT_DREP_STATE_TOPIC: &str = "cardano.drep.state";

// Query topic
const DEFAULT_DREPS_QUERY_TOPIC: (&str, &str) = ("dreps-state-query-topic", "cardano.query.dreps");

// Configuration defaults
const DEFAULT_STORE_INFO: (&str, bool) = ("store-info", false);
const DEFAULT_STORE_DELEGATORS: (&str, bool) = ("store-delegators", false);
const DEFAULT_STORE_METADATA: (&str, bool) = ("store-metadata", false);
const DEFAULT_STORE_UPDATES: (&str, bool) = ("store-updates", false);
const DEFAULT_STORE_VOTES: (&str, bool) = ("store-votes", false);

/// DRep State module
#[module(
    message_type(Message),
    name = "drep-state",
    description = "In-memory DRep State from certificate events"
)]
pub struct DRepState;

impl DRepState {
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut certs_subscription: Box<dyn Subscription<Message>>,
        mut gov_subscription: Option<Box<dyn Subscription<Message>>>,
        mut params_subscription: Option<Box<dyn Subscription<Message>>>,
        mut drep_state_publisher: DRepStatePublisher,
        context: Arc<Context<Message>>,
        storage_config: DRepStorageConfig,
    ) -> Result<()> {
        if storage_config.store_info {
            if let Some(sub) = params_subscription.as_mut() {
                let _ = sub.read().await?;
                info!("Consumed initial genesis params from params_subscription");
            }
        }
        // Main loop of synchronised messages
        loop {
            // Get the current state snapshot
            let mut state = {
                let mut h = history.lock().await;
                h.get_or_init_with(|| State::new(storage_config))
            };
            let mut current_block: Option<BlockInfo> = None;

            // Read per-block messages in parallel
            let certs_message_f = certs_subscription.read();

            // Certificates are the synchroniser
            let (_, certs_message) = certs_message_f.await?;
            let new_epoch = match certs_message.as_ref() {
                Message::Cardano((ref block_info, _)) => {
                    // rollback only on certs
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(block_info.number);
                    }
                    current_block = Some(block_info.clone());
                    block_info.new_epoch && block_info.epoch > 0
                }
                _ => false,
            };

            // Read from epoch-boundary messages only when it's a new epoch
            if new_epoch {
                // Read params subscription if store-info is enabled to obtain DRep expiration param. Update expirations on epoch transition
                if let Some(sub) = params_subscription.as_mut() {
                    let (_, message) = sub.read().await?;
                    match message.as_ref() {
                        Message::Cardano((
                            ref block_info,
                            CardanoMessage::ProtocolParams(params),
                        )) => {
                            Self::check_sync(&current_block, block_info, "params");
                            if let Some(conway) = &params.params.conway {
                                state
                                    .update_drep_expirations(
                                        block_info.epoch,
                                        conway.d_rep_activity,
                                    )
                                    .inspect_err(|e| error!("Param update error: {e:#}"))
                                    .ok();
                            }
                        }
                        _ => error!("Unexpected params message: {message:?}"),
                    }
                }

                // Publish DRep state at the end of the epoch
                if let Some(ref block) = current_block {
                    let dreps = state.active_drep_list();
                    drep_state_publisher.publish_drep_state(block, dreps).await?;
                }
            }

            // Handle cert message
            match certs_message.as_ref() {
                Message::Cardano((
                    ref block_info,
                    CardanoMessage::TxCertificates(tx_certs_msg),
                )) => {
                    let span = info_span!("drep_state.handle_certs", block = block_info.number);
                    async {
                        Self::check_sync(&current_block, block_info, "certs");
                        state
                            .process_certificates(
                                context.clone(),
                                &tx_certs_msg.certificates,
                                block_info.epoch,
                            )
                            .await
                            .inspect_err(|e| error!("Certificates handling error: {e:#}"))
                            .ok();
                    }
                    .instrument(span)
                    .await;
                }

                _ => error!("Unexpected message type: {certs_message:?}"),
            }

            // Handle governance message
            if let Some(sub) = gov_subscription.as_mut() {
                let (_, message) = sub.read().await?;
                match message.as_ref() {
                    Message::Cardano((
                        block_info,
                        CardanoMessage::GovernanceProcedures(gov_msg),
                    )) => {
                        let span = info_span!("drep_state.handle_votes", block = block_info.number);
                        async {
                            Self::check_sync(&current_block, block_info, "gov");
                            state
                                .process_votes(&gov_msg.voting_procedures)
                                .inspect_err(|e| error!("Votes handling error: {e:#}"))
                                .ok();
                        }
                        .instrument(span)
                        .await;
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }

            // Commit the new state
            if let Some(block_info) = current_block {
                history.lock().await.commit(block_info.number, state);
            }
        }
    }

    /// Check for synchronisation
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
        let storage_config = DRepStorageConfig {
            store_info: get_bool_flag(&config, DEFAULT_STORE_INFO),
            store_delegators: get_bool_flag(&config, DEFAULT_STORE_DELEGATORS),
            store_metadata: get_bool_flag(&config, DEFAULT_STORE_METADATA),
            store_updates: get_bool_flag(&config, DEFAULT_STORE_UPDATES),
            store_votes: get_bool_flag(&config, DEFAULT_STORE_VOTES),
        };

        let certificates_subscribe_topic =
            get_string_flag(&config, DEFAULT_CERTIFICATES_SUBSCRIBE_TOPIC);
        info!("Creating subscriber on '{certificates_subscribe_topic}'");

        let mut governance_subscribe_topic = String::new();
        if storage_config.store_votes {
            governance_subscribe_topic =
                get_string_flag(&config, DEFAULT_GOVERNANCE_SUBSCRIBE_TOPIC);
            info!("Creating subscriber on '{governance_subscribe_topic}'");
        }

        let mut parameters_subscribe_topic = String::new();
        if storage_config.store_info {
            parameters_subscribe_topic =
                get_string_flag(&config, DEFAULT_PARAMETERS_SUBSCRIBE_TOPIC);
            info!("Creating subscriber on '{parameters_subscribe_topic}'");
        }

        let drep_state_topic = config
            .get_string("publish-drep-state-topic")
            .unwrap_or(DEFAULT_DREP_STATE_TOPIC.to_string());
        info!("Creating DRep state publisher on '{drep_state_topic}'");

        let drep_query_topic = get_string_flag(&config, DEFAULT_DREPS_QUERY_TOPIC);
        info!("Creating DRep query handler on '{drep_query_topic}'");

        // Initalize state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "DRepState",
            StateHistoryStore::default_block_store(),
        )));
        let history_run = history.clone();
        let query_history = history.clone();
        let ticker_history = history.clone();
        let ctx_run = context.clone();

        // Query handler
        context.handle(&drep_query_topic, move |message| {
            let history = query_history.clone();
            async move {
                let Message::StateQuery(StateQuery::Governance(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Governance(
                        GovernanceStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for governance-state",
                        )),
                    )));
                };

                let locked = history.lock().await;

                let response = match query {
                    GovernanceStateQuery::GetDRepsList => match locked.current() {
                        Some(state) => {
                            let dreps = state.list();
                            GovernanceStateQueryResponse::DRepsList(DRepsList { dreps })
                        }
                        None => GovernanceStateQueryResponse::Error(QueryError::internal_error(
                            "No current DRep state",
                        )),
                    },
                    GovernanceStateQuery::GetDRepInfoWithDelegators { drep_credential } => {
                        match locked.current() {
                            Some(state) => match state.get_drep_info(drep_credential) {
                                Ok(Some(info)) => {
                                    match state.get_drep_delegators(drep_credential) {
                                        Ok(Some(delegators)) => {
                                            let response = DRepInfoWithDelegators {
                                                info: DRepInfo {
                                                    deposit: info.deposit,
                                                    retired: info.retired,
                                                    expired: info.expired,
                                                    active_epoch: info.active_epoch,
                                                    last_active_epoch: info.last_active_epoch,
                                                },
                                                delegators: delegators.to_vec(),
                                            };

                                            GovernanceStateQueryResponse::DRepInfoWithDelegators(
                                                response,
                                            )
                                        }

                                        Ok(None) => GovernanceStateQueryResponse::Error(
                                            QueryError::not_found(format!(
                                                "DRep delegators for {:?}",
                                                drep_credential
                                            )),
                                        ),
                                        Err(msg) => GovernanceStateQueryResponse::Error(
                                            QueryError::internal_error(msg),
                                        ),
                                    }
                                }

                                Ok(None) => {
                                    GovernanceStateQueryResponse::Error(QueryError::not_found(
                                        format!("DRep {:?} not found", drep_credential),
                                    ))
                                }
                                Err(msg) => GovernanceStateQueryResponse::Error(
                                    QueryError::internal_error(msg),
                                ),
                            },
                            None => GovernanceStateQueryResponse::Error(
                                QueryError::internal_error("No current state"),
                            ),
                        }
                    }
                    GovernanceStateQuery::GetDRepDelegators { drep_credential } => match locked
                        .current()
                    {
                        Some(state) => match state.get_drep_delegators(drep_credential) {
                            Ok(Some(delegators)) => GovernanceStateQueryResponse::DRepDelegators(
                                DRepDelegatorAddresses {
                                    addresses: delegators.clone(),
                                },
                            ),
                            Ok(None) => GovernanceStateQueryResponse::Error(QueryError::not_found(
                                format!("DRep delegators for {:?} not found", drep_credential),
                            )),
                            Err(msg) => {
                                GovernanceStateQueryResponse::Error(QueryError::internal_error(msg))
                            }
                        },
                        None => GovernanceStateQueryResponse::Error(QueryError::internal_error(
                            "No current state",
                        )),
                    },
                    GovernanceStateQuery::GetDRepMetadata { drep_credential } => match locked
                        .current()
                    {
                        Some(state) => match state.get_drep_anchor(drep_credential) {
                            Ok(Some(anchor)) => GovernanceStateQueryResponse::DRepMetadata(Some(
                                Some(anchor.clone()),
                            )),
                            Ok(None) => GovernanceStateQueryResponse::Error(QueryError::not_found(
                                format!("DRep metadata for {:?} not found", drep_credential),
                            )),
                            Err(msg) => {
                                GovernanceStateQueryResponse::Error(QueryError::internal_error(msg))
                            }
                        },
                        None => GovernanceStateQueryResponse::Error(QueryError::internal_error(
                            "No current state",
                        )),
                    },

                    GovernanceStateQuery::GetDRepUpdates { drep_credential } => match locked
                        .current()
                    {
                        Some(state) => match state.get_drep_updates(drep_credential) {
                            Ok(Some(updates)) => {
                                GovernanceStateQueryResponse::DRepUpdates(DRepUpdates {
                                    updates: updates.to_vec(),
                                })
                            }
                            Ok(None) => GovernanceStateQueryResponse::Error(QueryError::not_found(
                                format!("DRep updates for {:?} not found", drep_credential),
                            )),
                            Err(msg) => {
                                GovernanceStateQueryResponse::Error(QueryError::internal_error(msg))
                            }
                        },
                        None => GovernanceStateQueryResponse::Error(QueryError::internal_error(
                            "No current state",
                        )),
                    },
                    GovernanceStateQuery::GetDRepVotes { drep_credential } => {
                        match locked.current() {
                            Some(state) => match state.get_drep_votes(drep_credential) {
                                Ok(Some(votes)) => {
                                    GovernanceStateQueryResponse::DRepVotes(DRepVotes {
                                        votes: votes.to_vec(),
                                    })
                                }
                                Ok(None) => {
                                    GovernanceStateQueryResponse::Error(QueryError::not_found(
                                        format!("DRep votes for {:?}", drep_credential),
                                    ))
                                }
                                Err(msg) => GovernanceStateQueryResponse::Error(
                                    QueryError::internal_error(msg),
                                ),
                            },
                            None => GovernanceStateQueryResponse::Error(
                                QueryError::internal_error("No current state"),
                            ),
                        }
                    }
                    _ => GovernanceStateQueryResponse::Error(QueryError::internal_error(format!(
                        "Unimplemented governance query: {query:?}"
                    ))),
                };
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Governance(
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
                    if (message.number % 60) == 0 {
                        let span = info_span!("drep_state.tick", number = message.number);
                        async {
                            ticker_history
                                .lock()
                                .await
                                .get_current_state()
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

        // Publisher for DRep State
        let drep_state_publisher = DRepStatePublisher::new(context.clone(), drep_state_topic);

        // Subscribe to enabled topics
        let certs_sub = context.subscribe(&certificates_subscribe_topic).await?;

        let gov_sub = if storage_config.store_votes {
            Some(context.subscribe(&governance_subscribe_topic).await?)
        } else {
            None
        };

        let params_sub = if storage_config.store_info {
            Some(context.subscribe(&parameters_subscribe_topic).await?)
        } else {
            None
        };

        // Start run task
        context.run(async move {
            Self::run(
                history_run,
                certs_sub,
                gov_sub,
                params_sub,
                drep_state_publisher,
                ctx_run,
                storage_config,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
