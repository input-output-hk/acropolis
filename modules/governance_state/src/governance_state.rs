//! Acropolis Governance State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt, message_bus::Subscription};
use acropolis_common::messages::{Message, RESTResponse, CardanoMessage};
use std::sync::Arc;
use anyhow::{anyhow, Result};
use config::Config;
use hex::ToHex;
use tokio::sync::Mutex;
use tracing::{error, info};

mod state;
use state::State;

const DEFAULT_SUBSCRIBE_TOPIC: (&str, &str) = ("subscribe-topic", "cardano.governance");
const DEFAULT_HANDLE_TOPIC: (&str, &str) = ("handle-topic", "rest.get.governance-state.*");
const DEFAULT_DREP_DISTRIBUTION_TOPIC: (&str, &str) = ("stake-drep-distribution-topic", "cardano.drep.distribution");
const DEFAULT_PROTOCOL_PARAMETERS_TOPIC: (&str, &str) = ("protocol-parameters-topic", "cardano.parameters.state");
const DEFAULT_ENACT_STATE_TOPIC: (&str, &str) = ("enact-state-topic", "cardano.enact.state");

/// SPO State module
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
    protocol_parameters_topic: String,
    enact_state_topic: String
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
            protocol_parameters_topic: Self::conf(config, DEFAULT_PROTOCOL_PARAMETERS_TOPIC),
            enact_state_topic: Self::conf(config, DEFAULT_ENACT_STATE_TOPIC)
        })
    }
}

fn perform_rest_request(state: &State, path: &str) -> Result<String> {
    let request = match path.rfind('/') {
        None => return Err(anyhow!("Poorly formed url, '/' expected.")),
        Some(suffix_start) => &path[suffix_start+1..]
    };

    if request == "list" {
        let mut list_votes = Vec::new();
        let mut list_props = Vec::new();

        for (a,p) in state.list_proposals()?.into_iter() {
            list_props.push(format!("{}: {:?}", a, p));
        }

        for (a,v,tx,vp) in state.list_votes()?.into_iter() {
            list_votes.push(format!("{}: {} at {} voted as {:?}", a, v, tx.encode_hex::<String>(), vp));
        }

        Ok(format!("Governance proposals list: {:?}\nGovernance votes list: {:?}",
            list_props, list_votes
        ))
    }
    else {
        Err(anyhow!("Invalid action specified."))
    }
}

impl GovernanceState {
    async fn async_init(context: Arc<Context<Message>>, config: Arc<GovernanceStateConfig>) -> Result<()> {
        let gt = context.clone().message_bus.register(&config.subscribe_topic).await?;
        let dt = context.clone().message_bus.register(&config.drep_distribution_topic).await?;
        let pt = context.clone().message_bus.register(&config.protocol_parameters_topic).await?;

        tokio::spawn(async move {
            Self::run(context, config, gt, dt, pt).await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }

    async fn run(context: Arc<Context<Message>>, config: Arc<GovernanceStateConfig>,
                 mut governance_s: Box<dyn Subscription<Message>>,
                 mut drep_s: Box<dyn Subscription<Message>>,
                 mut protocol_s: Box<dyn Subscription<Message>>) -> Result<()>
    {
        let state = Arc::new(Mutex::new(State::new(context.clone(), config.enact_state_topic.clone())));
        let state_handle = state.clone();
        //let state_tick = state.clone();

        // REST requests handling
        context.message_bus.handle(&config.clone().handle_topic, move |message: Arc<Message>| {
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
                    },
                    _ => {
                        error!("Unexpected message type: {message:?}");
                        RESTResponse::with_text(500, &format!("Unexpected message type"))
                    }
                };

                Arc::new(Message::RESTResponse(response))
            }
        })?;

        loop {
            info!("tick: reading {}", config.subscribe_topic);
            let (blk_g, gov_procs) = match governance_s.read().await?.1.as_ref() {
                Message::Cardano((block, CardanoMessage::GovernanceProcedures(procs))) => (block.clone(), procs.clone()),
                msg => {
                    error!("Unexpected message {msg:?} for governance procedures topic");
                    continue
                }
            };
            info!("read {}: {:?}, {:?}", config.subscribe_topic, blk_g, gov_procs);

            if blk_g.new_epoch {
                match protocol_s.read().await?.1.as_ref() {
                    Message::Cardano((blk_p, CardanoMessage::ProtocolParams(params))) => {
                        if blk_p.number != blk_g.number {
                            error!("Misaligned governance and protocol blocks: {blk_g:?} and {blk_p:?}");
                        }
                        state.lock().await.handle_protocol_parameters(&blk_p, &params).await?;
                    }
                    msg => error!("Unexpected message {msg:?} for protocol parameters topic")
                }
            };

            state.lock().await.handle_governance(&blk_g, &gov_procs).await?;

            match drep_s.read().await?.1.as_ref() {
                Message::Cardano((block, CardanoMessage::DRepStakeDistribution(procs))) => 
                    state.lock().await.handle_drep_stake(/*&block, */&procs).await?,
                msg => error!("Unexpected message {msg:?} for DRep distribution topic")
            }
        }
    }

    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async move {
                Self::async_init(context, GovernanceStateConfig::new(&config))
                    .await.unwrap_or_else(|e| error!("Failed: {e}"));
            })
        });

        Ok(())
    }
}
