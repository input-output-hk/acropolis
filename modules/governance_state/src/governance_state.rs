//! Acropolis Governance State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use acropolis_common::validation::ValidationOutcomes;
use acropolis_common::{
    caryatid::SubscriptionExt,
    declare_cardano_reader,
    messages::{
        CardanoMessage, DRepStakeDistributionMessage, GovernanceProceduresMessage, Message,
        ProtocolParamsMessage, SPOStakeDistributionMessage, StateQuery, StateQueryResponse,
        StateTransitionMessage,
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

const CONFIG_GOVERNANCE_TOPIC: (&str, &str) = ("subscribe-topic", "cardano.governance");
const CONFIG_DREP_DISTRIBUTION_TOPIC: &str = "stake-drep-distribution-topic";
const CONFIG_SPO_DISTRIBUTION_TOPIC: &str = "stake-spo-distribution-topic";
const CONFIG_PROTOCOL_PARAMETERS_TOPIC: (&str, &str) =
    ("protocol-parameters-topic", "cardano.protocol.parameters");
const CONFIG_ENACT_STATE_TOPIC: (&str, &str) = ("enact-state-topic", "cardano.enact.state");
const CONFIG_VALIDATION_OUTCOME_TOPIC: (&str, &str) =
    ("validation-outcome-topic", "cardano.validation.governance");

const VERIFICATION_OUTPUT_FILE: &str = "verification-output-file";

/// Governance State module
#[module(
    message_type(Message),
    name = "governance-state",
    description = "In-memory Governance State from events"
)]
pub struct GovernanceState;

pub struct GovernanceStateConfig {
    governance_topic: String,
    drep_distribution_topic: Option<String>,
    spo_distribution_topic: Option<String>,
    protocol_parameters_topic: String,
    enact_state_topic: String,
    governance_query_topic: String,
    validation_outcome_topic: String,
    verification_output_file: Option<String>,
}

impl GovernanceStateConfig {
    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        let actual = config.get_string(keydef.0).unwrap_or(keydef.1.to_string());
        info!("Creating subscriber on '{}' for {}", actual, keydef.0);
        actual
    }

    fn conf_option(config: &Arc<Config>, key: &str) -> Option<String> {
        let actual = config.get_string(key).ok();
        if let Some(ref value) = actual {
            info!("Creating subscriber on '{}' for {}", value, key);
        }
        actual
    }

    pub fn new(config: &Arc<Config>) -> Arc<Self> {
        Arc::new(Self {
            governance_topic: Self::conf(config, CONFIG_GOVERNANCE_TOPIC),
            drep_distribution_topic: Self::conf_option(config, CONFIG_DREP_DISTRIBUTION_TOPIC),
            spo_distribution_topic: Self::conf_option(config, CONFIG_SPO_DISTRIBUTION_TOPIC),
            protocol_parameters_topic: Self::conf(config, CONFIG_PROTOCOL_PARAMETERS_TOPIC),
            enact_state_topic: Self::conf(config, CONFIG_ENACT_STATE_TOPIC),
            governance_query_topic: Self::conf(config, DEFAULT_GOVERNANCE_QUERY_TOPIC),
            validation_outcome_topic: Self::conf(config, CONFIG_VALIDATION_OUTCOME_TOPIC),
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

    async fn run(
        context: Arc<Context<Message>>,
        config: Arc<GovernanceStateConfig>,
        mut governance_s: Box<dyn Subscription<Message>>,
        mut drep_s: Option<Box<dyn Subscription<Message>>>,
        mut spo_s: Option<Box<dyn Subscription<Message>>>,
        mut protocol_s: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let state = Arc::new(Mutex::new(State::new(
            context.clone(),
            config.enact_state_topic.clone(),
            config.verification_output_file.clone(),
        )));

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
                        if let Some(ref mut drep_s) = drep_s {
                            if let Some(ref mut spo_s) = spo_s {
                                let (blk_drep, d_drep) = Self::read_drep(drep_s).await?;
                                if blk_g != blk_drep {
                                    outcomes.push_anyhow(anyhow!(
                                        "Governance {blk_g:?} and DRep distribution {blk_drep:?} are out of sync"
                                    ));
                                }

                                let (blk_spo, d_spo) = Self::read_spo(spo_s).await?;
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
                        }
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
        let gt = context.clone().subscribe(&cfg.governance_topic).await?;
        let dt = match cfg.drep_distribution_topic {
            Some(ref topic) => Some(context.clone().subscribe(topic).await?),
            None => None,
        };
        let st = match cfg.spo_distribution_topic {
            Some(ref topic) => Some(context.clone().subscribe(topic).await?),
            None => None,
        };
        let pt = context.clone().subscribe(&cfg.protocol_parameters_topic).await?;

        tokio::spawn(async move {
            Self::run(context, cfg, gt, dt, st, pt).await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
