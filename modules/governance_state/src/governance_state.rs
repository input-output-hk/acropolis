//! Acropolis Governance State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper, ValidationContext},
    configuration::{get_string_flag, StartupMode},
    declare_cardano_reader,
    messages::{
        CardanoMessage, DRepStakeDistributionMessage, DRepStateMessage,
        GovernanceProceduresMessage, Message, ProtocolParamsMessage, SPOStakeDistributionMessage,
        SnapshotMessage, SnapshotStateMessage, StateQuery, StateQueryResponse,
        StateTransitionMessage,
    },
    queries::{
        errors::QueryError,
        governance::{
            GovernanceStateQuery, GovernanceStateQueryResponse, ProposalInfo, ProposalVotes,
            ProposalsList, DEFAULT_GOVERNANCE_QUERY_TOPIC,
        },
    },
};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::{message_bus::Subscription, module, Context};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod alonzo_babbage_voting;
mod conway_voting;
mod conway_voting_test;
mod state;
mod voting_state;

use state::State;
use voting_state::VotingRegistrationState;

declare_cardano_reader!(
    GovReader,
    "subscribe-topic",
    "cardano.governance",
    GovernanceProcedures,
    GovernanceProceduresMessage
);
declare_cardano_reader!(
    ParamReader,
    "protocol-parameters-topic",
    "cardano.protocol.parameters",
    ProtocolParams,
    ProtocolParamsMessage
);

declare_cardano_reader!(
    DRepReader,
    "stake-drep-distribution-topic",
    "cardano.drep.distribution",
    DRepStakeDistribution,
    DRepStakeDistributionMessage
);

declare_cardano_reader!(
    SPOReader,
    "stake-spo-distribution-topic",
    "cardano.spo.distribution",
    SPOStakeDistribution,
    SPOStakeDistributionMessage
);

declare_cardano_reader!(
    DRepStateReader,
    "drep-state-topic",
    "cardano.drep.state",
    DRepState,
    DRepStateMessage
);

const CONFIG_ENACT_STATE_TOPIC: (&str, &str) = ("enact-state-topic", "cardano.enact.state");
const CONFIG_VALIDATION_OUTCOME_TOPIC: (&str, &str) =
    ("validation-outcome-topic", "cardano.validation.governance");
const CONFIG_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

const VERIFICATION_OUTPUT_FILE: &str = "verification-output-file";
const VERIFY_VOTES_FILES: &str = "verify-votes-files";
const VERIFY_AGGREGATE_VOTES_FILE: &str = "verify-aggregate-votes-file";

/// Governance State module
#[module(
    message_type(Message),
    name = "governance-state",
    description = "In-memory Governance State from events"
)]
pub struct GovernanceState;

pub struct GovernanceStateConfig {
    enact_publish_topic: String,
    governance_query_topic: String,
    validation_outcome_topic: String,
    snapshot_subscribe_topic: String,
    verification_output_file: Option<String>,
    verify_votes_files: Option<String>,
    verify_aggregated_votes_file: Option<String>,
}

struct Readers {
    pub gov_reader: GovReader,
    pub drep_reader: DRepReader,
    pub drep_state_reader: DRepStateReader,
    pub spo_reader: SPOReader,
    pub param_reader: ParamReader,
}

impl GovernanceStateConfig {
    pub fn new(config: &Arc<Config>) -> Arc<Self> {
        Arc::new(Self {
            enact_publish_topic: get_string_flag(config, CONFIG_ENACT_STATE_TOPIC),
            governance_query_topic: get_string_flag(config, DEFAULT_GOVERNANCE_QUERY_TOPIC),
            validation_outcome_topic: get_string_flag(config, CONFIG_VALIDATION_OUTCOME_TOPIC),
            snapshot_subscribe_topic: get_string_flag(config, CONFIG_SNAPSHOT_SUBSCRIBE_TOPIC),
            verification_output_file: config
                .get_string(VERIFICATION_OUTPUT_FILE)
                .map(Some)
                .unwrap_or(None),
            verify_votes_files: config.get_string(VERIFY_VOTES_FILES).map(Some).unwrap_or_default(),
            verify_aggregated_votes_file: config
                .get_string(VERIFY_AGGREGATE_VOTES_FILE)
                .map(Some)
                .unwrap_or_default(),
        })
    }
}

impl GovernanceState {
    /// Wait for and process snapshot bootstrap messages
    async fn wait_for_bootstrap(
        state: Arc<Mutex<State>>,
        mut snapshot_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        info!("Waiting for governance state snapshot bootstrap messages...");

        loop {
            let Ok((_, message)) = snapshot_subscription.read().await else {
                info!("Snapshot subscription closed");
                return Ok(());
            };

            match message.as_ref() {
                Message::Snapshot(SnapshotMessage::Startup) => {
                    info!("Snapshot Startup message received");
                }
                Message::Snapshot(SnapshotMessage::Bootstrap(
                    SnapshotStateMessage::GovernanceState(gov_msg),
                )) => {
                    let mut locked = state.lock().await;
                    // Use a default voting length if conway params not yet available
                    // The actual voting length will be set when protocol params arrive
                    let voting_length = locked
                        .get_conway_voting()
                        .get_conway_params()
                        .map(|p| p.gov_action_lifetime as u64)
                        .unwrap_or(6); // Default to 6 epochs if not set

                    locked
                        .get_conway_voting_mut()
                        .bootstrap_from_snapshot(gov_msg, voting_length)?;
                    info!(
                        "Snapshot Bootstrap message received, {} proposals loaded",
                        gov_msg.proposals.len()
                    );
                }
                Message::Snapshot(SnapshotMessage::Complete) => {
                    info!("Snapshot complete, exiting bootstrap loop");
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    async fn process_drep_spo(
        vld: &mut ValidationContext,
        state: Arc<Mutex<State>>,
        readers: &mut Box<Readers>,
    ) -> Result<()> {
        let d_drep = match vld.consume_sync(
            "drep_reader",
            readers.drep_reader.read_with_rollbacks().await,
        )? {
            RollbackWrapper::Normal((_, d_drep)) => Some(d_drep),
            RollbackWrapper::Rollback(_) => None,
        };

        let spo_msg =
            match vld.consume_sync("spo_reader", readers.spo_reader.read_with_rollbacks().await)? {
                RollbackWrapper::Normal((blk_spo, d_spo)) => Some((blk_spo, d_spo)),
                RollbackWrapper::Rollback(_) => None,
            };

        let drep_state = match vld.consume_sync(
            "drep_state_reader",
            readers.drep_state_reader.read_with_rollbacks().await,
        )? {
            RollbackWrapper::Normal((_, drep_state)) => Some(drep_state),
            RollbackWrapper::Rollback(_) => None,
        };

        if let Some((blk_spo, d_spo)) = spo_msg {
            if let Some(drep_state) = drep_state {
                if let Some(d_drep) = d_drep {
                    if blk_spo.epoch != d_spo.epoch + 1 {
                        vld.handle_error(
                            "spo",
                            &anyhow!(
                                "SPO distibution {blk_spo:?} != SPO epoch + 1 ({})",
                                d_spo.epoch
                            ),
                        );
                    }

                    if drep_state.epoch != d_drep.epoch {
                        vld.handle_error(
                            "drep_state",
                            &anyhow!(
                                "DRep state {} epoch != DRep epoch ({})",
                                drep_state.epoch,
                                d_drep.epoch
                            ),
                        );
                    }

                    vld.handle(
                        "handle_drep_stake",
                        state.lock().await.handle_drep_stake(&d_drep, &drep_state, &d_spo).await,
                    );
                }
            }
        }

        Ok(())
    }

    async fn run(
        context: Arc<Context<Message>>,
        config: Arc<GovernanceStateConfig>,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        mut readers: Box<Readers>,
    ) -> Result<()> {
        let state = Arc::new(Mutex::new(State::new(
            config.verification_output_file.clone(),
            config.verify_votes_files.clone(),
            config.verify_aggregated_votes_file.clone(),
        )?));

        // Wait for snapshot bootstrap if subscription is provided
        if let Some(subscription) = snapshot_subscription {
            Self::wait_for_bootstrap(state.clone(), subscription).await?;
        }

        // Ticker to log stats
        let state_tick = state.clone();
        let mut subscription = context.subscribe("clock.tick").await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        let span = info_span!("governance_state.tick", number = message.number);
                        async {
                            state_tick
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

        let query_state = state.clone();
        context.handle(&config.governance_query_topic, move |message| {
            let state_handle = query_state.clone();
            async move {
                let Message::StateQuery(StateQuery::Governance(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Governance(
                        GovernanceStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for governance-state",
                        )),
                    )));
                };

                let locked = state_handle.lock().await;

                let response = match query {
                    GovernanceStateQuery::GetProposalsList => {
                        let proposals = locked.list_proposals();
                        GovernanceStateQueryResponse::ProposalsList(ProposalsList { proposals })
                    }

                    GovernanceStateQuery::GetProposalInfo { proposal } => {
                        match locked.get_proposal(proposal) {
                            Some(proc) => {
                                GovernanceStateQueryResponse::ProposalInfo(ProposalInfo {
                                    procedure: proc.clone(),
                                })
                            }
                            None => GovernanceStateQueryResponse::Error(QueryError::not_found(
                                format!("Proposal {} not found", proposal),
                            )),
                        }
                    }
                    GovernanceStateQuery::GetProposalVotes { proposal } => {
                        match locked.get_proposal_votes(proposal) {
                            Ok(votes) => {
                                GovernanceStateQueryResponse::ProposalVotes(ProposalVotes { votes })
                            }
                            Err(_) => GovernanceStateQueryResponse::Error(QueryError::not_found(
                                format!("Proposal {} not found", proposal),
                            )),
                        }
                    }
                    _ => GovernanceStateQueryResponse::Error(QueryError::not_implemented(format!(
                        "Unimplemented governance query: {query:?}"
                    ))),
                };

                Arc::new(Message::StateQueryResponse(StateQueryResponse::Governance(
                    response,
                )))
            }
        });

        loop {
            let mut vld = ValidationContext::new(
                &context,
                &config.validation_outcome_topic,
                "governance_state",
            );

            let primary = PrimaryRead::from_sync(
                &mut vld,
                "gov_reader",
                readers.gov_reader.read_with_rollbacks().await,
            )?;

            if let Some(message) = primary.rollback_message() {
                context.publish(&config.enact_publish_topic, message.clone()).await?;
            }

            async {
                if let Some(gov_procs) = primary.message() {
                    let blk_g = primary.block_info();
                    if blk_g.new_epoch {
                        // New governance from new epoch means that we must prepare all governance
                        // outcome for the previous epoch.
                        let mut state = state.lock().await;
                        let gov_outcomes = state.process_new_epoch(blk_g);
                        if let Some(gov_outcomes) =
                            vld.handle("process outcome", gov_outcomes.map(Some))
                        {
                            let message = Arc::new(Message::Cardano((
                                blk_g.as_ref().clone(),
                                CardanoMessage::GovernanceOutcomes(gov_outcomes),
                            )));
                            vld.handle(
                                "publish",
                                context.publish(&config.enact_publish_topic, message).await,
                            );
                        }
                    }

                    // Governance may present in any block -- not only in 'new epoch' blocks.
                    vld.handle(
                        "handle_governance",
                        state.lock().await.handle_governance(blk_g, gov_procs).await,
                    );

                    if blk_g.new_epoch {
                        match vld.consume_sync(
                            "param_reader",
                            readers.param_reader.read_with_rollbacks().await,
                        )? {
                            RollbackWrapper::Normal((blk_g, params)) => {
                                vld.handle(
                                    "handle_protocol_parameters",
                                    state.lock().await.handle_protocol_parameters(&params).await,
                                );

                                if blk_g.epoch > 0 {
                                    Self::process_drep_spo(&mut vld, state.clone(), &mut readers)
                                        .await?;
                                }

                                vld.handle(
                                    "advance_epoch",
                                    state.lock().await.advance_epoch(&blk_g),
                                );
                            }
                            RollbackWrapper::Rollback(_) => {}
                        }
                    }
                } else {
                    vld.consume_sync(
                        "param_reader",
                        readers.param_reader.read_with_rollbacks().await,
                    )?;
                    Self::process_drep_spo(&mut vld, state.clone(), &mut readers).await?;
                }

                Ok::<(), anyhow::Error>(())
            }
            .await?;

            if primary.do_validation() {
                vld.publish().await;
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = GovernanceStateConfig::new(&config);

        // Subscribe for snapshot bootstrap if starting from snapshot
        let snapshot_subscription = if StartupMode::from_config(config.as_ref()).is_snapshot() {
            Some(context.subscribe(&cfg.snapshot_subscribe_topic).await?)
        } else {
            None
        };

        let readers = Box::new(Readers {
            gov_reader: GovReader::new(&context, &config).await?,
            drep_reader: DRepReader::new(&context, &config).await?,
            drep_state_reader: DRepStateReader::new(&context, &config).await?,
            spo_reader: SPOReader::new(&context, &config).await?,
            param_reader: ParamReader::new(&context, &config).await?,
        });

        tokio::spawn(async move {
            Self::run(context, cfg, snapshot_subscription, readers)
                .await
                .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
