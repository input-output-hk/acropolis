//! Acropolis Governance State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper, ValidationContext},
    configuration::{get_string_flag, StartupMode},
    declare_cardano_reader,
    messages::{
        CardanoMessage, DRepStakeDistributionMessage, DRepStateMessage,
        GovernanceProceduresMessage, Message, ProtocolParamsMessage, SPODefaultVoteMessage,
        SPOStakeDistributionMessage, SnapshotMessage, SnapshotStateMessage, StateQuery,
        StateQueryResponse, StateTransitionMessage,
    },
    queries::{
        errors::QueryError,
        governance::{
            GovernanceStateQuery, GovernanceStateQueryResponse, ProposalInfo, ProposalVotes,
            ProposalsList, DEFAULT_GOVERNANCE_QUERY_TOPIC,
        },
    },
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo,
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

declare_cardano_reader!(
    SPODefaultVoteReader,
    "spo-default-vote-topic",
    "cardano.spo.default-vote",
    SPODefaultVote,
    SPODefaultVoteMessage
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
    pub spo_default_vote_reader: SPODefaultVoteReader,
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
        history: Arc<Mutex<StateHistory<State>>>,
        config: &GovernanceStateConfig,
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
                    let mut state = history.lock().await.get_or_init_with(|| {
                        State::new(
                            config.verification_output_file.clone(),
                            config.verify_votes_files.clone(),
                            config.verify_aggregated_votes_file.clone(),
                        )
                        .expect("failed to initialize State")
                    });
                    // Use a default voting length if conway params not yet available
                    // The actual voting length will be set when protocol params arrive
                    let voting_length = state
                        .get_conway_voting()
                        .get_conway_params()
                        .map(|p| p.gov_action_lifetime as u64)
                        .unwrap_or(6); // Default to 6 epochs if not set

                    state
                        .get_conway_voting_mut()
                        .bootstrap_from_snapshot(gov_msg, voting_length)?;

                    history.lock().await.bootstrap_init_with(state, gov_msg.block_number);
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
        block_info: &BlockInfo,
        vld: &mut ValidationContext,
        state: &mut State,
        readers: &mut Box<Readers>,
    ) -> Result<()> {
        let d_drep = vld.consume_opt(
            "drep_reader",
            readers.drep_reader.read_with_rollbacks().await,
        )?;

        let spo_msg =
            vld.consume_opt("spo_reader", readers.spo_reader.read_with_rollbacks().await)?;

        let drep_state = vld.consume_opt(
            "drep_state_reader",
            readers.drep_state_reader.read_with_rollbacks().await,
        )?;

        let spo_default_vote = vld.consume_opt(
            "spo_default_vote_reader",
            readers.spo_default_vote_reader.read_with_rollbacks().await,
        )?;

        if let Some(d_spo) = spo_msg {
            if let Some(drep_state) = drep_state {
                if let Some(d_drep) = d_drep {
                    if let Some(spo_default_vote) = spo_default_vote {
                        if block_info.epoch != d_spo.epoch + 1 {
                            vld.handle_error(
                                "spo",
                                &anyhow!(
                                    "SPO distibution {block_info:?} != SPO epoch + 1 ({})",
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
                            state
                                .handle_drep_stake(&d_drep, &drep_state, &d_spo, &spo_default_vote)
                                .await,
                        );
                    }
                }
            }
        }

        Ok(())
    }

    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        context: Arc<Context<Message>>,
        config: Arc<GovernanceStateConfig>,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        mut readers: Box<Readers>,
    ) -> Result<()> {
        // Wait for snapshot bootstrap if subscription is provided
        if let Some(subscription) = snapshot_subscription {
            Self::wait_for_bootstrap(history.clone(), config.as_ref(), subscription).await?;
        }

        // Ticker to log stats
        let history_tick = history.clone();
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
                            if let Some(state) = history_tick.lock().await.current() {
                                state.log_stats();
                            }
                        }
                        .instrument(span)
                        .await;
                    }
                }
            }
        });

        let query_history = history.clone();
        context.handle(&config.governance_query_topic, move |message| {
            let state_handle = query_history.clone();
            async move {
                let Message::StateQuery(StateQuery::Governance(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Governance(
                        GovernanceStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for governance-state",
                        )),
                    )));
                };

                let locked = state_handle.lock().await.get_current_state();

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

            let mut state = history.lock().await.get_or_init_with(|| {
                State::new(
                    config.verification_output_file.clone(),
                    config.verify_votes_files.clone(),
                    config.verify_aggregated_votes_file.clone(),
                )
                .expect("Failed to initialize state")
            });

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
                        state.handle_governance(blk_g, gov_procs).await,
                    );

                    if blk_g.new_epoch {
                        if let Some(params) = vld.consume_opt(
                            "param_reader",
                            readers.param_reader.read_with_rollbacks().await,
                        )? {
                            vld.handle(
                                "handle_protocol_parameters",
                                state.handle_protocol_parameters(&params).await,
                            );

                            if blk_g.epoch > 0 {
                                Self::process_drep_spo(
                                    blk_g.as_ref(),
                                    &mut vld,
                                    &mut state,
                                    &mut readers,
                                )
                                .await?;
                            }

                            vld.handle("advance_epoch", state.advance_epoch(blk_g));
                        }
                    }
                } else {
                    // If the primary message was a rollback still read the other readers to keep synchronization aligned
                    vld.consume(
                        "param_reader",
                        readers.param_reader.read_with_rollbacks().await,
                    )?;
                    Self::process_drep_spo(
                        primary.block_info().as_ref(),
                        &mut vld,
                        &mut state,
                        &mut readers,
                    )
                    .await?;
                }

                Ok::<(), anyhow::Error>(())
            }
            .await?;

            // Commit the new state
            if primary.message().is_some() {
                history.lock().await.commit(primary.block_info().number, state);

                if primary.do_validation() {
                    vld.publish().await;
                }
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

        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "governance_state",
            StateHistoryStore::default_block_store(),
        )));

        let readers = Box::new(Readers {
            gov_reader: GovReader::new(&context, &config).await?,
            drep_reader: DRepReader::new(&context, &config).await?,
            drep_state_reader: DRepStateReader::new(&context, &config).await?,
            spo_reader: SPOReader::new(&context, &config).await?,
            param_reader: ParamReader::new(&context, &config).await?,
            spo_default_vote_reader: SPODefaultVoteReader::new(&context, &config).await?,
        });

        tokio::spawn(async move {
            Self::run(history, context, cfg, snapshot_subscription, readers)
                .await
                .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
