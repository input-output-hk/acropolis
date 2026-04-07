//! Acropolis Governance State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper, ValidationContext},
    configuration::StartupMode,
    declare_cardano_reader,
    messages::{
        CardanoMessage, DRepStakeDistributionMessage, DRepStateMessage,
        GovernanceProceduresMessage, Message, ProtocolParamsMessage, SPOStakeDistributionMessage,
        SnapshotMessage, SnapshotStateMessage, StateQuery, StateQueryResponse,
        StateTransitionMessage,
    },
    queries::errors::QueryError,
    queries::governance::{
        GovernanceStateQuery, GovernanceStateQueryResponse, ProposalInfo, ProposalVotes,
        ProposalsList, DEFAULT_GOVERNANCE_QUERY_TOPIC,
    },
};
use anyhow::{anyhow, bail, Result};
use caryatid_sdk::{message_bus::Subscription, module, Context};
use config::Config;
use std::sync::Arc;
use tokio::sync::{Mutex, MutexGuard};
use tracing::{error, info, info_span, Instrument};
use acropolis_common::calculations::epoch_to_first_slot_with_shelley_params;
use acropolis_common::state_history::{StateHistory, StateHistoryStore};

mod alonzo_babbage_voting;
mod conway_voting;
mod conway_voting_test;
mod state;
mod voting_state;

use state::State;
use voting_state::VotingRegistrationState;
use crate::conway_voting::VerificationConfig;

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
}

struct Readers {
    pub gov_reader: GovReader,
    pub drep_reader: DRepReader,
    pub drep_state_reader: DRepStateReader,
    pub spo_reader: SPOReader,
    pub param_reader: ParamReader,
}

impl GovernanceStateConfig {
    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        let actual = config.get_string(keydef.0).unwrap_or(keydef.1.to_string());
        info!(
            "Creating subscriber/publisher on '{}' for {}",
            actual, keydef.0
        );
        actual
    }

    pub fn new(config: &Arc<Config>) -> Arc<Self> {
        Arc::new(Self {
            enact_publish_topic: Self::conf(config, CONFIG_ENACT_STATE_TOPIC),
            governance_query_topic: Self::conf(config, DEFAULT_GOVERNANCE_QUERY_TOPIC),
            validation_outcome_topic: Self::conf(config, CONFIG_VALIDATION_OUTCOME_TOPIC),
            snapshot_subscribe_topic: Self::conf(config, CONFIG_SNAPSHOT_SUBSCRIBE_TOPIC),
            verification_output_file: config
                .get_string(VERIFICATION_OUTPUT_FILE)
                .map(Some)
                .unwrap_or(None),
            verify_votes_files: config.get_string(VERIFY_VOTES_FILES).map(Some).unwrap_or_default(),
        })
    }
}

impl GovernanceState {
    /// Wait for and process snapshot bootstrap messages
    async fn wait_for_bootstrap(
        history: Arc<Mutex<StateHistory<State>>>,
        mut snapshot_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        info!("Waiting for governance state snapshot bootstrap messages...");
        let mut state = State::default();

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
                    // Use a default voting length if conway params not yet available
                    // The actual voting length will be set when protocol params arrive
                    let voting_length = state
                        .get_conway_voting()
                        .get_conway_params()
                        .map(|p| p.gov_action_lifetime as u64)
                        .unwrap_or(6); // Default to 6 epochs if not set

                    state.get_conway_voting_mut().bootstrap_from_snapshot(gov_msg, voting_length);
                    info!(
                        "Snapshot Bootstrap message received, {} proposals loaded",
                        gov_msg.proposals.len()
                    );
                }
                Message::Snapshot(SnapshotMessage::Complete) => {
                    history.lock().await.bootstrap_init_with(state, 0);
                    info!("Snapshot complete, exiting bootstrap loop");
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    async fn process_drep_spo(
        vld: &mut ValidationContext,
        state: &mut State,
        readers: &mut Box<Readers>,
    ) -> Result<()> {
        let d_drep = vld.consume_rollback_optoin(
            "drep_reader",
            readers.drep_reader.read_with_rollbacks().await,
        )?;

        let spo_msg =
            vld.consume_rollback_optoin("spo_reader", readers.spo_reader.read_with_rollbacks().await)?;

        let drep_state = vld.consume_rollback_optoin(
            "drep_state_reader",
            readers.drep_state_reader.read_with_rollbacks().await,
        )?;

        if let (Some(d_spo), Some(drep_state), Some(d_drep)) = (spo_msg, drep_state, d_drep) {
            let epoch = vld.get_block_info()?.epoch;

            if epoch != d_spo.epoch + 1 {
                vld.handle_error(
                    "spo",
                    &anyhow!(
                        "SPO block epoch {epoch} != SPO epoch + 1 ({})",
                        d_spo.epoch
                    ),
                );
            }

            if epoch != d_drep.epoch {
                vld.handle_error(
                    "drep_state",
                    &anyhow!(
                        "DRep state epoch {epoch} != DRep epoch ({})",
                        d_drep.epoch
                    ),
                );
            }

            vld.handle(
                "handle_drep_stake",
                state.handle_drep_stake(&d_drep, &drep_state, &d_spo).await,
            );
        }

        Ok(())
    }

    async fn run(
        context: Arc<Context<Message>>,
        config: Arc<GovernanceStateConfig>,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        mut readers: Box<Readers>,
    ) -> Result<()> {
        let history: Arc<Mutex<StateHistory<State>>> =
            Arc::new(Mutex::new(StateHistory::<State>::new(
                "governance_state",
                StateHistoryStore::Unbounded,
            )));

        // Wait for snapshot bootstrap if subscription is provided
        if let Some(subscription) = snapshot_subscription {
            Self::wait_for_bootstrap(history.clone(), subscription).await?;
        }

        // Ticker to log stats
        let tick = history.clone();
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
                            let history: MutexGuard<StateHistory<State>> = tick.lock().await;
                            let state: Option<&State> = history.current();
                            if let Some(state) = state {
                                state.tick().await.inspect_err(|e| error!("Tick error: {e}")).ok();
                            }
                            else {
                                error!("No state available for tick");
                            }
                        }
                        .instrument(span)
                        .await;
                    }
                }
            }
        });

        let query = history.clone();
        context.handle(&config.governance_query_topic, move |message| {
            let query_handle = query.clone();
            async move {
                let Message::StateQuery(StateQuery::Governance(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Governance(
                        GovernanceStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for governance-state",
                        )),
                    )));
                };

                let history_lock = query_handle.lock().await;
                let locked = history_lock.current();

                let response = match (locked, query) {
                    (Some(st), GovernanceStateQuery::GetProposalsList) => {
                        let proposals = st.list_proposals();
                        GovernanceStateQueryResponse::ProposalsList(ProposalsList { proposals })
                    }

                    (Some(st), GovernanceStateQuery::GetProposalInfo { proposal }) => {
                        match st.get_proposal(proposal) {
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
                    (Some(st), GovernanceStateQuery::GetProposalVotes { proposal }) => {
                        match st.get_proposal_votes(proposal) {
                            Ok(votes) => {
                                GovernanceStateQueryResponse::ProposalVotes(ProposalVotes { votes })
                            }
                            Err(_) => GovernanceStateQueryResponse::Error(QueryError::not_found(
                                format!("Proposal {} not found", proposal),
                            )),
                        }
                    }
                    (Some(_),_) => GovernanceStateQueryResponse::Error(QueryError::not_implemented(format!(
                        "Unimplemented governance query: {query:?}"
                    ))),
                    (None,_) => GovernanceStateQueryResponse::Error(QueryError::internal_error(
                        "Governance state is not yet initialized",
                    )),
                };

                Arc::new(Message::StateQueryResponse(StateQueryResponse::Governance(
                    response,
                )))
            }
        });

        let vconf = VerificationConfig {
            verification_output_file: config.verification_output_file.clone(),
            verify_votes_files: config.verify_votes_files.clone(),
        };
        
        loop {
            let mut state = history.lock().await.get_or_init_with(State::default);

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
                state = history.lock().await.get_rolled_back_state(primary.block_info().epoch);
                context.publish(&config.enact_publish_topic, message.clone()).await?;
            }

            async {
                if let Some(gov_procs) = primary.message() {
                    let blk_g = primary.block_info();
                    if blk_g.new_epoch {
                        // New governance from new epoch means that we must prepare all governance
                        // outcome for the previous epoch.
                        let gov_outcomes = state.process_new_epoch(blk_g, &vconf);
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
                        match vld.consume_sync(
                            "param_reader",
                            readers.param_reader.read_with_rollbacks().await,
                        )? {
                            RollbackWrapper::Normal((blk_g, params)) => {
                                vld.handle(
                                    "handle_protocol_parameters",
                                    state.handle_protocol_parameters(&params).await,
                                );

                                if blk_g.epoch > 0 {
                                    Self::process_drep_spo(&mut vld, &mut state, &mut readers)
                                        .await?;
                                }

                                vld.handle("advance_epoch", state.advance_epoch(&blk_g));
                            }
                            RollbackWrapper::Rollback(_) => {

                            }
                        }
                    }
                } else {
                    vld.consume_sync(
                        "param_reader",
                        readers.param_reader.read_with_rollbacks().await,
                    )?;
                    Self::process_drep_spo(&mut vld, &mut state, &mut readers).await?;
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
