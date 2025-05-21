//! Acropolis Governance State module for Caryatid
//! Accepts certificate events and derives the Governance State in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::messages::{Message, RESTResponse};
use std::sync::Arc;
use anyhow::{anyhow, Result};
use config::Config;
use hex::ToHex;
use tokio::sync::Mutex;
use tracing::{error, info};

mod state;
use state::State;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.governance";
const DEFAULT_HANDLE_TOPIC: &str = "rest.get.governance-state.*";
const DEFAULT_DREP_DISTRIBUTION_TOPIC: &str = "cardano.drep.distribution";
const DEFAULT_GENESIS_COMPLETE_TOPIC: &str = "cardano.sequence.bootstrapped";

/// SPO State module
#[module(
    message_type(Message),
    name = "governance-state",
    description = "In-memory Governance State from events"
)]
pub struct GovernanceState;

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

impl GovernanceState
{
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let handle_topic = config.get_string("handle-topic")
            .unwrap_or(DEFAULT_HANDLE_TOPIC.to_string());
        info!("Creating request handler on '{handle_topic}'");

        let drep_distribution_topic = config.get_string("stake-drep-distribution-topic")
            .unwrap_or(DEFAULT_DREP_DISTRIBUTION_TOPIC.to_string());
        info!("Creating request handler on '{drep_distribution_topic}'");

        let genesis_complete_topic = config.get_string("genesis-complete-topic")
            .unwrap_or(DEFAULT_GENESIS_COMPLETE_TOPIC.to_string());
        info!("Creating request handler on '{genesis_complete_topic}'");

        let state = Arc::new(Mutex::new(State::new()));
        let state_gov = state.clone();
        let state_drep = state.clone();
        let state_genesis = state.clone();
        let state_handle = state.clone();
        let state_tick = state.clone();

        // Subscribe to governance procedures serializer
        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {
            let state = state_gov.clone();

            async move {
                match message.as_ref() {
                    Message::GovernanceProcedures(msg) => {
                        let mut state = state.lock().await;
                        state.handle_governance(msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        // Subscribe to drep stake distribution serializer
        context.clone().message_bus.subscribe(&drep_distribution_topic, move |message: Arc<Message>| {
            let state = state_drep.clone();

            async move {
                match message.as_ref() {
                    Message::DRepStakeDistribution(msg) => {
                        let mut state = state.lock().await;
                        state.handle_drep_stake(msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        // Subscribe to bootstrap completion serializer
        context.clone().message_bus.subscribe(&genesis_complete_topic, move |message: Arc<Message>| {
            let state = state_genesis.clone();

            async move {
                match message.as_ref() {
                    Message::GenesisComplete(msg) => {
                        let mut state = state.lock().await;
                        state.handle_genesis(msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        // REST requests handling
        context.message_bus.handle(&handle_topic, move |message: Arc<Message>| {
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

        // Ticker to log stats
        context.clone().message_bus.subscribe("clock.tick", move |message: Arc<Message>| {
            let state = state_tick.clone();

            async move {
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        state.lock().await.tick()
                            .await
                            .inspect_err(|e| error!("Tick error: {e}"))
                            .ok();
                    }
                }
            }
        })?;

        Ok(())
    }
}
