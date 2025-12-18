//! Acropolis Governance State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use acropolis_common::configuration::StartupMethod;
use acropolis_common::validation::ValidationOutcomes;
use acropolis_common::{
    caryatid::SubscriptionExt,
    declare_cardano_reader,
    messages::{
        CardanoMessage, DRepStakeDistributionMessage, GovernanceProceduresMessage, Message,
        ProtocolParamsMessage, SPOStakeDistributionMessage, SnapshotMessage, SnapshotStateMessage,
        StateQuery, StateQueryResponse, StateTransitionMessage,
    },
    queries::errors::QueryError,
    queries::governance::{
        GovernanceStateQuery, GovernanceStateQueryResponse, ProposalInfo, ProposalVotes,
        ProposalsList, DEFAULT_GOVERNANCE_QUERY_TOPIC,
    },
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

const DEFAULT_SUBSCRIBE_TOPIC: (&str, &str) = ("subscribe-topic", "cardano.governance");
const DEFAULT_DREP_DISTRIBUTION_TOPIC: (&str, &str) =
    ("stake-drep-distribution-topic", "cardano.drep.distribution");
const DEFAULT_SPO_DISTRIBUTION_TOPIC: (&str, &str) =
    ("stake-spo-distribution-topic", "cardano.spo.distribution");
const DEFAULT_PROTOCOL_PARAMETERS_TOPIC: (&str, &str) =
    ("protocol-parameters-topic", "cardano.protocol.parameters");
const DEFAULT_ENACT_STATE_TOPIC: (&str, &str) = ("enact-state-topic", "cardano.enact.state");
const DEFAULT_VALIDATION_OUTCOME_TOPIC: (&str, &str) =
    ("validation-outcome-topic", "cardano.validation.governance");
/// Topic for receiving bootstrap data when starting from a CBOR dump snapshot
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

const VERIFICATION_OUTPUT_FILE: &str = "verification-output-file";

/// Governance State module
#[module(
    message_type(Message),
    name = "governance-state",
    description = "In-memory Governance State from events"
)]
pub struct GovernanceState;

pub struct GovernanceStateConfig {
    subscribe_topic: String,
    drep_distribution_topic: String,
    spo_distribution_topic: String,
    protocol_parameters_topic: String,
    enact_state_topic: String,
    governance_query_topic: String,
    validation_outcome_topic: String,
    snapshot_subscribe_topic: String,
    verification_output_file: Option<String>,
}

impl GovernanceStateConfig {
    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        let actual = config.get_string(keydef.0).unwrap_or(keydef.1.to_string());
        info!("Creating subscriber on '{}' for {}", actual, keydef.0);
        actual
    }

    pub fn new(config: &Arc<Config>) -> Arc<Self> {
        Arc::new(Self {
            subscribe_topic: Self::conf(config, DEFAULT_SUBSCRIBE_TOPIC),
            drep_distribution_topic: Self::conf(config, DEFAULT_DREP_DISTRIBUTION_TOPIC),
            spo_distribution_topic: Self::conf(config, DEFAULT_SPO_DISTRIBUTION_TOPIC),
            protocol_parameters_topic: Self::conf(config, DEFAULT_PROTOCOL_PARAMETERS_TOPIC),
            enact_state_topic: Self::conf(config, DEFAULT_ENACT_STATE_TOPIC),
            governance_query_topic: Self::conf(config, DEFAULT_GOVERNANCE_QUERY_TOPIC),
            validation_outcome_topic: Self::conf(config, DEFAULT_VALIDATION_OUTCOME_TOPIC),
            snapshot_subscribe_topic: Self::conf(config, DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC),
            verification_output_file: config
                .get_string(VERIFICATION_OUTPUT_FILE)
                .map(Some)
                .unwrap_or(None),
        })
    }
}

impl GovernanceState {
    declare_cardano_reader!(
        read_governance,
        GovernanceProcedures,
        GovernanceProceduresMessage
    );
    declare_cardano_reader!(read_parameters, ProtocolParams, ProtocolParamsMessage);
    declare_cardano_reader!(
        read_drep,
        DRepStakeDistribution,
        DRepStakeDistributionMessage
    );
    declare_cardano_reader!(read_spo, SPOStakeDistribution, SPOStakeDistributionMessage);

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

                    locked.get_conway_voting_mut().bootstrap_from_snapshot(gov_msg, voting_length);
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

    async fn run(
        context: Arc<Context<Message>>,
        config: Arc<GovernanceStateConfig>,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        mut governance_s: Box<dyn Subscription<Message>>,
        mut drep_s: Box<dyn Subscription<Message>>,
        mut spo_s: Box<dyn Subscription<Message>>,
        mut protocol_s: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let state = Arc::new(Mutex::new(State::new(
            context.clone(),
            config.enact_state_topic.clone(),
            config.verification_output_file.clone(),
        )));

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
            let (_, message) = governance_s.read().await?;
            let (blk_g, gov_procs) = match message.as_ref() {
                Message::Cardano((blk, CardanoMessage::GovernanceProcedures(msg))) => {
                    (blk.clone(), msg.clone())
                }
                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                )) => {
                    let mut state = state.lock().await;
                    state.publish_rollback(message).await?;
                    continue;
                }
                _ => bail!("Unexpected message {message:?} for governance procedures topic"),
            };

            let mut outcomes = ValidationOutcomes::default();
            let span = info_span!("governance_state.handle", block = blk_g.number);
            async {
                if blk_g.new_epoch {
                    // New governance from new epoch means that we must prepare all governance
                    // outcome for the previous epoch.
                    let mut state = state.lock().await;
                    let governance_outcomes = state.process_new_epoch(&blk_g)?;
                    state.send(&blk_g, governance_outcomes).await?;
                }

                // Governance may present in any block -- not only in 'new epoch' blocks.
                {
                    outcomes.merge(&mut state.lock().await.handle_governance(&blk_g, &gov_procs).await?);
                }

                if blk_g.new_epoch {
                    let (blk_p, params) = Self::read_parameters(&mut protocol_s).await?;
                    if blk_g != blk_p {
                        outcomes.push_anyhow(anyhow!(
                            "Governance {blk_g:?} and protocol parameters {blk_p:?} are out of sync"
                        ));
                    }

                    {
                        state.lock().await.handle_protocol_parameters(&params).await?;
                    }

                    if blk_g.epoch > 0 {
                        // TODO: make sync more stable
                        let (blk_drep, d_drep) = Self::read_drep(&mut drep_s).await?;
                        if blk_g != blk_drep {
                            outcomes.push_anyhow(anyhow!(
                                "Governance {blk_g:?} and DRep distribution {blk_drep:?} are out of sync"
                            ));
                        }

                        let (blk_spo, d_spo) = Self::read_spo(&mut spo_s).await?;
                        if blk_g != blk_spo {
                            outcomes.push_anyhow(anyhow!(
                                "Governance {blk_g:?} and SPO distribution {blk_spo:?} are out of sync"
                            ));
                        }

                        if blk_spo.epoch != d_spo.epoch + 1 {
                            outcomes.push_anyhow(anyhow!(
                                "SPO distibution {blk_spo:?} != SPO epoch + 1 ({})",
                                d_spo.epoch
                            ));
                        }

                        state.lock().await.handle_drep_stake(&d_drep, &d_spo).await?
                    }

                    {
                        state.lock().await.advance_epoch(&blk_g)?;
                    }
                }
                Ok::<(), anyhow::Error>(())
            }.instrument(span).await?;

            outcomes.publish(&context, &config.validation_outcome_topic, &blk_g).await?;
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = GovernanceStateConfig::new(&config);

        // Subscribe for snapshot bootstrap if starting from snapshot
        let snapshot_subscription = if StartupMethod::from_config(config.as_ref()).is_snapshot() {
            Some(context.subscribe(&cfg.snapshot_subscribe_topic).await?)
        } else {
            None
        };

        let gt = context.clone().subscribe(&cfg.subscribe_topic).await?;
        let dt = context.clone().subscribe(&cfg.drep_distribution_topic).await?;
        let st = context.clone().subscribe(&cfg.spo_distribution_topic).await?;
        let pt = context.clone().subscribe(&cfg.protocol_parameters_topic).await?;

        tokio::spawn(async move {
            Self::run(context, cfg, snapshot_subscription, gt, dt, st, pt)
                .await
                .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
