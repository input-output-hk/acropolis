//! Acropolis Governance State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use acropolis_common::{
    messages::{
        CardanoMessage, DRepStakeDistributionMessage, SPOStakeDistributionMessage,
        GovernanceOutcomesMessage, GovernanceProceduresMessage, Message,
        ProtocolParamsMessage, RESTResponse,
    },
    BlockInfo,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::{message_bus::Subscription, module, Context, Module};
use config::Config;
use hex::ToHex;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

mod state;
mod voting_state;
use state::State;
use voting_state::VotingRegistrationState;

const DEFAULT_SUBSCRIBE_TOPIC: (&str, &str) = ("subscribe-topic", "cardano.governance");
const DEFAULT_HANDLE_TOPIC: (&str, &str) = ("handle-topic", "rest.get.governance-state.*");
const DEFAULT_DREP_DISTRIBUTION_TOPIC: (&str, &str) =
    ("stake-drep-distribution-topic", "cardano.drep.distribution");
const DEFAULT_SPO_DISTRIBUTION_TOPIC: (&str, &str) =
    ("stake-spo-distribution-topic", "cardano.spo.distribution");
const DEFAULT_PROTOCOL_PARAMETERS_TOPIC: (&str, &str) =
    ("protocol-parameters-topic", "cardano.protocol.parameters");
const DEFAULT_ENACT_STATE_TOPIC: (&str, &str) = ("enact-state-topic", "cardano.enact.state");

/// Governance State module
#[module(
    message_type(Message),
    name = "governance-state",
    description = "In-memory Governance State from events"
)]
pub struct GovernanceState;

pub struct GovernanceStateConfig {
    subscribe_topic: String,
    handle_topic: String,
    drep_distribution_topic: String,
    spo_distribution_topic: String,
    protocol_parameters_topic: String,
    enact_state_topic: String,
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
            handle_topic: Self::conf(config, DEFAULT_HANDLE_TOPIC),
            drep_distribution_topic: Self::conf(config, DEFAULT_DREP_DISTRIBUTION_TOPIC),
            spo_distribution_topic: Self::conf(config, DEFAULT_SPO_DISTRIBUTION_TOPIC),
            protocol_parameters_topic: Self::conf(config, DEFAULT_PROTOCOL_PARAMETERS_TOPIC),
            enact_state_topic: Self::conf(config, DEFAULT_ENACT_STATE_TOPIC),
        })
    }
}

fn perform_rest_request(state: &State, path: &str) -> Result<String> {
    let request = match path.rfind('/') {
        None => return Err(anyhow!("Poorly formed url, '/' expected.")),
        Some(suffix_start) => &path[suffix_start + 1..],
    };

    if request == "list" {
        let mut list_votes = Vec::new();
        let mut list_props = Vec::new();

        for (a, p) in state.list_proposals()?.into_iter() {
            list_props.push(format!("{}: {:?}", a, p));
        }

        for (a, v, tx, vp) in state.list_votes()?.into_iter() {
            list_votes.push(format!(
                "{}: {} at {} voted as {:?}",
                a,
                v,
                tx.encode_hex::<String>(),
                vp
            ));
        }

        Ok(format!(
            "Governance proposals list: {:?}\nGovernance votes list: {:?}",
            list_props, list_votes
        ))
    } else {
        Err(anyhow!("Invalid action specified."))
    }
}

impl GovernanceState {
    async fn read_governance(
        governance_s: &mut Box<dyn Subscription<Message>>,
    ) -> Result<(BlockInfo, GovernanceProceduresMessage)> {
        match governance_s.read().await?.1.as_ref() {
            Message::Cardano((blk, CardanoMessage::GovernanceProcedures(msg))) => {
                Ok((blk.clone(), msg.clone()))
            }
            msg => Err(anyhow!(
                "Unexpected message {msg:?} for governance procedures topic"
            )),
        }
    }

    async fn read_parameters<'a>(
        parameters_s: &mut Box<dyn Subscription<Message>>,
    ) -> Result<(BlockInfo, ProtocolParamsMessage)> {
        match parameters_s.read().await?.1.as_ref() {
            Message::Cardano((blk, CardanoMessage::ProtocolParams(params))) => {
                Ok((blk.clone(), params.clone()))
            }
            msg => Err(anyhow!(
                "Unexpected message {msg:?} for protocol parameters topic"
            )),
        }
    }

    async fn read_drep(
        drep_s: &mut Box<dyn Subscription<Message>>,
    ) -> Result<(BlockInfo, DRepStakeDistributionMessage)> {
        match drep_s.read().await?.1.as_ref() {
            Message::Cardano((blk, CardanoMessage::DRepStakeDistribution(distr))) => {
                Ok((blk.clone(), distr.clone()))
            }
            msg => Err(anyhow!(
                "Unexpected message {msg:?} for DRep distribution topic"
            )),
        }
    }

    async fn read_spo(
        spo_s: &mut Box<dyn Subscription<Message>>,
    ) -> Result<(BlockInfo, SPOStakeDistributionMessage)> {
        match spo_s.read().await?.1.as_ref() {
            Message::Cardano((blk, CardanoMessage::SPOStakeDistribution(distr))) => {
                Ok((blk.clone(), distr.clone()))
            }
            msg => Err(anyhow!(
                "Unexpected message {msg:?} for SPO distribution topic"
            )),
        }
    }

    async fn run(
        context: Arc<Context<Message>>,
        config: Arc<GovernanceStateConfig>,
        mut governance_s: Box<dyn Subscription<Message>>,
        mut drep_s: Box<dyn Subscription<Message>>,
        mut spo_s: Box<dyn Subscription<Message>>,
        mut protocol_s: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let state = Arc::new(Mutex::new(State::new(
            context.clone(),
            config.enact_state_topic.clone(),
        )));
        let state_handle = state.clone();
        let state_tick = state.clone();

        // REST requests handling
        context.handle(
            &config.clone().handle_topic,
            move |message: Arc<Message>| {
                let state = state_handle.clone();
                async move {
                    let response = match message.as_ref() {
                        Message::RESTRequest(request) => {
                            info!("REST received {} {}", request.method, request.path);
                            let lock = state.lock().await;

                            match perform_rest_request(&lock, &request.path) {
                                Ok(response) => RESTResponse::with_text(200, &response),
                                Err(error) => {
                                    error!("Governance State REST request error: {error:?}");
                                    RESTResponse::with_text(400, &format!("{error:?}"))
                                }
                            }
                        }
                        _ => {
                            error!("Unexpected message type: {message:?}");
                            RESTResponse::with_text(500, &format!("Unexpected message type"))
                        }
                    };

                    Arc::new(Message::RESTResponse(response))
                }
            },
        );

        // Ticker to log stats
        let mut subscription = context.subscribe("clock.tick").await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        state_tick.lock().await.tick().await
                            .inspect_err(|e| error!("Tick error: {e}")).ok();
                    }
                }
            }
        });

        loop {
            let (blk_g, gov_procs) = Self::read_governance(&mut governance_s).await?;

            if blk_g.new_epoch {
                // New governance from new epoch means that we must prepare all governance
                // outcome for the previous epoch.
                let mut state = state.lock().await;
                let governance_outcomes = match state.process_new_epoch(&blk_g) {
                    Ok(out) => out,
                    Err(e) => {
                        error!("Cannot process new epoch: {e}");
                        GovernanceOutcomesMessage::default()
                    }
                };
                state.send(&blk_g, governance_outcomes).await?;
            }

            {
                state.lock().await.handle_governance(&blk_g, &gov_procs).await?;
            }

            if blk_g.new_epoch {
                let (blk_p, params) = Self::read_parameters(&mut protocol_s).await?;
                if blk_g != blk_p {
                    error!(
                        "Governance {blk_g:?} and protocol parameters {blk_p:?} are out of sync"
                    );
                }
                state.lock().await.handle_protocol_parameters(&params).await?;

                if blk_g.epoch > 0 {
                    // TODO: make sync more stable
                    let (blk_drep, d_drep) = Self::read_drep(&mut drep_s).await?;
                    if blk_g != blk_drep {
                        error!("Governance {blk_g:?} and DRep distribution {blk_drep:?} are out of sync");
                    }

                    let (blk_spo, d_spo) = Self::read_spo(&mut spo_s).await?;
                    if blk_g != blk_spo {
                        error!("Governance {blk_g:?} and SPO distribution {blk_spo:?} are out of sync");
                    }

                    if blk_spo.epoch != d_spo.epoch+1 {
                        error!("SPO distibution {blk_spo:?} != SPO epoch + 1 ({})", d_spo.epoch);
                    }

                    state.lock().await.handle_drep_stake(&d_drep, &d_spo).await?
                }
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = GovernanceStateConfig::new(&config);
        let gt = context.clone().subscribe(&cfg.subscribe_topic).await?;
        let dt = context.clone().subscribe(&cfg.drep_distribution_topic).await?;
        let st = context.clone().subscribe(&cfg.spo_distribution_topic).await?;
        let pt = context.clone().subscribe(&cfg.protocol_parameters_topic).await?;

        tokio::spawn(async move {
            Self::run(context, cfg, gt, dt, st, pt).await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
