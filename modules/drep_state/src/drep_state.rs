//! Acropolis SPO state module for Caryatid
//! Accepts certificate events and derives the SPO state in memory

use caryatid_sdk::{Context, Module, module, MessageBusExt};
use acropolis_common::{messages::Message, DRepCredential, Serialiser};
use std::sync::Arc;
use anyhow::{anyhow, Result};
use config::Config;
use tracing::{error, info};
use tokio::sync::Mutex;
use acropolis_common::messages::RESTResponse;

mod state;
mod drep_voting_stake_publisher;

use state::State;
use crate::drep_voting_stake_publisher::DrepVotingStakePublisher;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.certificates";
const DEFAULT_HANDLE_TOPIC: &str = "rest.get.drep-state.*";
const DEFAULT_VOTING_STAKE_TOPIC: &str = "stake.voting.drep";

/// SPO State module
#[module(
    message_type(Message),
    name = "drep-state",
    description = "In-memory DRep State from certificate events"
)]
pub struct DRepState;

fn decode_rest_drep_credential(id: &str) -> Result<DRepCredential> {
    if let Some(stripped) = id.strip_prefix("script=") {
        Ok(DRepCredential::ScriptHash(hex::decode(stripped)?))
    }
    else if let Some(stripped) = id.strip_prefix("address=") {
        Ok(DRepCredential::AddrKeyHash(hex::decode(stripped)?))
    }
    else {
        Err(anyhow!("Poorly formed url, 'script=<hex key hash>' or 'address=<hex key hash>' DRep credential should be provided"))
    }
}

fn perform_rest_request(state: &State, path: &str) -> Result<String> {
    let request = match path.rfind('/') {
        None => return Err(anyhow!("Poorly formed url, '/' expected.")),
        Some(suffix_start) => &path[suffix_start+1..]
    };

    if request == "list" {
        Ok(format!("DRep list: {:?}", state.list()))
    }
    else {
        let cred = decode_rest_drep_credential(request)?;
        match state.get_drep(&cred) {
            Some(drep) => Ok(format!("DRep {:?}: deposit={}, anchor={:?}", cred, drep.deposit, drep.anchor)),
            None => Ok(format!("No DRep {:?}", cred))
        }
    }
}

impl DRepState
{
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {

        // Get configuration
        let subscribe_topic = config.get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let handle_topic = config.get_string("handle-topic")
            .unwrap_or(DEFAULT_HANDLE_TOPIC.to_string());
        info!("Creating request handler on '{handle_topic}'");

        let drep_voting_stake_topic = config.get_string("publish-stake-voting-drep-topic")
            .unwrap_or(DEFAULT_VOTING_STAKE_TOPIC.to_string());
        info!("Creating request handler on '{drep_voting_stake_topic}'");

        let publisher = DrepVotingStakePublisher::new(context.clone(), drep_voting_stake_topic);

        let state = Arc::new(Mutex::new(State::new(Some(publisher))));
        let state_handle = state.clone();
        let state_tick = state.clone();

        let serialiser = Arc::new(Mutex::new(Serialiser::new(state, module_path!(), 1)));
        let serialiser_tick = serialiser.clone();

        // Subscribe for certificate messages
        context.clone().message_bus.subscribe(&subscribe_topic, move |message: Arc<Message>| {
            let serialiser = serialiser.clone();
            async move {
                match message.as_ref() {
                    Message::TxCertificates(tx_cert_msg) => {
                        let mut serialiser = serialiser.lock().await;
                        serialiser.handle_message(tx_cert_msg.sequence, tx_cert_msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}")
                }
            }
        })?;

        // Handle requests for single DRep state
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
                                error!("DRep REST request error: {error:?}");
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
            let serialiser = serialiser_tick.clone();
            let state = state_tick.clone();

            async move {
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        state.lock().await.tick()
                            .await
                            .inspect_err(|e| error!("Tick error: {e}"))
                            .ok();
                        serialiser.lock().await.tick();
                    }
                }
            }
        })?;

        Ok(())
    }
}
