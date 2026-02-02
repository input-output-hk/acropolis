//! Acropolis DRep State module for Caryatid
//! Accepts certificate events and derives the DRep State in memory

use acropolis_common::{
    caryatid::{RollbackWrapper, ValidationContext},
    configuration::StartupMode,
    declare_cardano_reader,
    messages::{
        CardanoMessage, GovernanceProceduresMessage, Message, ProtocolParamsMessage,
        SnapshotMessage, SnapshotStateMessage, StateQuery, StateQueryResponse,
        StateTransitionMessage, TxCertificatesMessage,
    },
    queries::{
        errors::QueryError,
        governance::{
            DRepDelegatorAddresses, DRepInfo, DRepInfoWithDelegators, DRepUpdates, DRepVotes,
            DRepsList, GovernanceStateQuery, GovernanceStateQueryResponse,
        },
    },
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus,
};
use anyhow::{bail, Result};
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
declare_cardano_reader!(
    CertReader,
    "certificates-subscribe-topic",
    "cardano.certificates",
    TxCertificates,
    TxCertificatesMessage
);

declare_cardano_reader!(
    ParamReader,
    "parameters-subscribe-topic",
    "cardano.protocol.parameters",
    ProtocolParams,
    ProtocolParamsMessage
);

declare_cardano_reader!(
    GovReader,
    "governance-subscribe-topic",
    "cardano.governance",
    GovernanceProcedures,
    GovernanceProceduresMessage
);

const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

// Publisher topic
const DEFAULT_DREP_STATE_TOPIC: &str = "cardano.drep.state";

const DEFAULT_VALIDATION_OUTPUT_TOPIC: (&str, &str) =
    ("validation-output-topic", "cardano.validation.drep");

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

struct DRepSubscriptions {
    snapshot: Option<Box<dyn Subscription<Message>>>,
    certs: CertReader,
    gov: Option<GovReader>,
    params: Option<ParamReader>,
}

impl DRepState {
    /// Wait for and process snapshot bootstrap message if available
    async fn wait_for_bootstrap(
        history: Arc<Mutex<StateHistory<State>>>,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        storage_config: DRepStorageConfig,
    ) -> Result<()> {
        let mut subscription = match snapshot_subscription {
            Some(sub) => sub,
            None => {
                info!("No snapshot subscription, skipping bootstrap");
                return Ok(());
            }
        };

        info!("Waiting for snapshot bootstrap messages...");
        loop {
            let Ok((_, message)) = subscription.read().await else {
                info!("Snapshot subscription closed without receiving bootstrap");
                break;
            };

            match message.as_ref() {
                Message::Snapshot(SnapshotMessage::Startup) => {
                    info!("Received Startup signal, awaiting bootstrap data...");
                }
                Message::Snapshot(SnapshotMessage::Bootstrap(SnapshotStateMessage::DRepState(
                    drep_msg,
                ))) => {
                    info!(
                        "Received bootstrap message with {} DReps for epoch {}",
                        drep_msg.dreps.len(),
                        drep_msg.epoch
                    );
                    let block_number = drep_msg.block_number;
                    // Snapshot bootstrap: protocol parameters not known yet.
                    let mut state = State::new(storage_config, None, None, None);
                    state.bootstrap(drep_msg);
                    let drep_count = state.dreps.len();
                    history.lock().await.bootstrap_init_with(state, block_number);
                    info!(
                        "Bootstrap complete - {} DReps committed to state",
                        drep_count
                    );
                }
                Message::Snapshot(SnapshotMessage::Complete) => {
                    info!("Snapshot complete, exiting DRep state bootstrap loop");
                    return Ok(());
                }
                _ => (),
            }
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut subs: Box<DRepSubscriptions>,
        mut drep_state_publisher: DRepStatePublisher,
        validation_topic: String,
        context: Arc<Context<Message>>,
        storage_config: DRepStorageConfig,
    ) -> Result<()> {
        // Wait for snapshot bootstrap first (if available)
        Self::wait_for_bootstrap(history.clone(), subs.snapshot, storage_config).await?;

        // Initial Conway params (needed for DRep expiries calculations)
        // and Shelly params (protocol version).
        let mut initial_d_rep_activity: Option<u32> = None;
        let mut initial_gov_action_lifetime: Option<u32> = None;
        let mut is_bootstrap: Option<bool> = None;

        if let Some(params) = &mut subs.params {
            let (_, message) = params.read_skip_rollbacks().await?;

            // Snapshot may start mid-epoch, so read protocol params from genesis.
            if let (Some(shelley), Some(conway)) = (&message.params.shelley, &message.params.conway)
            {
                is_bootstrap = Option::from(shelley.protocol_params.protocol_version.is_chang()?);
                initial_d_rep_activity = Some(conway.d_rep_activity);
                initial_gov_action_lifetime = Some(conway.gov_action_lifetime);
            } else if message.params.conway.is_some() {
                bail!("Invalid protocol parameters: Conway parameters require Shelley parameters.");
            }
            info!("Consumed initial genesis params from params_subscription");
        }

        // Main loop of synchronised messages
        loop {
            // Get the current state snapshot
            let mut state = {
                let mut h = history.lock().await;
                h.get_or_init_with(|| {
                    State::new(
                        storage_config,
                        initial_d_rep_activity,
                        initial_gov_action_lifetime,
                        is_bootstrap,
                    )
                })
            };

            let mut ctx = ValidationContext::new(&context, &validation_topic);

            let (certs_message, new_epoch) =
                match &ctx.consume_sync(subs.certs.read_with_rollbacks().await)? {
                    RollbackWrapper::Normal(msg @ (blk_inf, _)) => {
                        if blk_inf.status == BlockStatus::RolledBack {
                            state = history.lock().await.get_rolled_back_state(blk_inf.number);
                        }
                        let new_epoch =
                            (blk_inf.new_epoch && blk_inf.epoch > 0).then_some(blk_inf.epoch);
                        (Some(msg.clone()), new_epoch)
                    }
                    RollbackWrapper::Rollback(msg) => {
                        ctx.handle(
                            "rollback",
                            drep_state_publisher.publish_rollback(msg.clone()).await,
                        );
                        (None, None)
                    }
                };

            // Read from epoch-boundary messages only when it's a new epoch
            if let Some(new_epoch) = new_epoch {
                state.update_num_dormant_epochs(new_epoch);

                // Read params subscription if store-info is enabled to obtain DRep expiration param.
                // Update expirations on epoch transition
                if let Some(params) = &mut subs.params {
                    if let Some((_, msg)) =
                        ctx.consume("params", params.read_skip_rollbacks().await)
                    {
                        ctx.handle("params", state.update_protocol_params(&msg.params));
                        ctx.handle("params", state.update_drep_expirations(new_epoch));
                    }
                }

                // Publish DRep state at the end of the epoch
                let dreps = state.active_drep_list();
                let block_info = ctx.get_block_info()?;
                let inactive_dreps = state.inactive_drep_list(block_info.epoch);
                drep_state_publisher.publish_drep_state(&block_info, dreps, inactive_dreps).await?;
            }

            if let Some((block_info, tx_certs)) = certs_message {
                let span = info_span!("drep_state.handle_certs", block = block_info.number);
                async {
                    ctx.merge(
                        "certs",
                        state
                            .process_certificates(
                                context.clone(),
                                &tx_certs.certificates,
                                block_info.epoch,
                                state.conway_d_rep_activity,
                            )
                            .await,
                    )
                }
                .instrument(span)
                .await;
            }

            if let Some(gov_sub) = subs.gov.as_mut() {
                if let Some((blk_inf, gov)) =
                    ctx.consume("gov", gov_sub.read_skip_rollbacks().await)
                {
                    let span = info_span!("drep_state.handle_votes", block = blk_inf.number);
                    async {
                        // Track proposals for dormant-epoch counting, so that
                        // they can be checked if they are active at the N+1 epoch boundary.
                        state.record_proposals(&gov.proposal_procedures, blk_inf.epoch);

                        if !gov.proposal_procedures.is_empty() {
                            state.apply_dormant_expiry(blk_inf.epoch);
                        }

                        ctx.merge(
                            "gov",
                            state.process_votes(
                                &gov.voting_procedures,
                                blk_inf.epoch,
                                state.conway_d_rep_activity,
                            ),
                        );
                    }
                    .instrument(span)
                    .await;
                }
            }

            // Commit the new state
            if let Some(block_info) = &ctx.get_current_block_opt() {
                history.lock().await.commit(block_info.number, state);
            }

            ctx.publish().await;
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

        // Subscribe for snapshot messages if bootstrapping from snapshot
        let snapshot_subscribe_topic = config
            .get_string(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.1.to_string());

        let subscriptions = DRepSubscriptions {
            snapshot: if StartupMode::from_config(config.as_ref()).is_snapshot() {
                info!("Creating subscriber on '{snapshot_subscribe_topic}' for DRep bootstrap");
                Some(context.subscribe(&snapshot_subscribe_topic).await?)
            } else {
                None
            },
            certs: CertReader::new(&context, &config).await?,
            gov: GovReader::new_opt(storage_config.store_votes, &context, &config).await?,
            params: ParamReader::new_opt(storage_config.store_info, &context, &config).await?,
        };

        let drep_state_topic = config
            .get_string("publish-drep-state-topic")
            .unwrap_or(DEFAULT_DREP_STATE_TOPIC.to_string());
        info!("Creating DRep state publisher on '{drep_state_topic}'");

        let drep_query_topic = get_string_flag(&config, DEFAULT_DREPS_QUERY_TOPIC);
        info!("Creating DRep query handler on '{drep_query_topic}'");

        let validation_topic = get_string_flag(&config, DEFAULT_VALIDATION_OUTPUT_TOPIC);
        info!("Creating DRep state publisher on '{validation_topic}'");

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
                    GovernanceStateQuery::GetDRepDelegators { drep_credential } => {
                        match locked.current() {
                            Some(state) => match state.get_drep_delegators(drep_credential) {
                                Ok(Some(delegators)) => {
                                    GovernanceStateQueryResponse::DRepDelegators(
                                        DRepDelegatorAddresses {
                                            addresses: delegators.clone(),
                                        },
                                    )
                                }
                                Ok(None) => GovernanceStateQueryResponse::Error(
                                    QueryError::not_found(format!(
                                        "DRep delegators for {:?} not found",
                                        drep_credential
                                    )),
                                ),
                                Err(msg) => GovernanceStateQueryResponse::Error(
                                    QueryError::internal_error(msg),
                                ),
                            },
                            None => GovernanceStateQueryResponse::Error(
                                QueryError::internal_error("No current state"),
                            ),
                        }
                    }
                    GovernanceStateQuery::GetDRepMetadata { drep_credential } => {
                        match locked.current() {
                            Some(state) => match state.get_drep_anchor(drep_credential) {
                                Ok(Some(anchor)) => GovernanceStateQueryResponse::DRepMetadata(
                                    Some(Some(anchor.clone())),
                                ),
                                Ok(None) => GovernanceStateQueryResponse::Error(
                                    QueryError::not_found(format!(
                                        "DRep metadata for {:?} not found",
                                        drep_credential
                                    )),
                                ),
                                Err(msg) => GovernanceStateQueryResponse::Error(
                                    QueryError::internal_error(msg),
                                ),
                            },
                            None => GovernanceStateQueryResponse::Error(
                                QueryError::internal_error("No current state"),
                            ),
                        }
                    }

                    GovernanceStateQuery::GetDRepUpdates { drep_credential } => {
                        match locked.current() {
                            Some(state) => match state.get_drep_updates(drep_credential) {
                                Ok(Some(updates)) => {
                                    GovernanceStateQueryResponse::DRepUpdates(DRepUpdates {
                                        updates: updates.to_vec(),
                                    })
                                }
                                Ok(None) => {
                                    GovernanceStateQueryResponse::Error(QueryError::not_found(
                                        format!("DRep updates for {:?} not found", drep_credential),
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

        // Start run task
        context.run(async move {
            Self::run(
                history_run,
                Box::new(subscriptions),
                drep_state_publisher,
                validation_topic,
                ctx_run,
                storage_config,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
