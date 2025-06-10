//! Acropolis DRep State module for Caryatid
//! Accepts certificate events and derives the DRep State in memory

use acropolis_common::messages::RESTResponse;
use acropolis_common::{
    messages::{CardanoMessage, Message},
    DRepCredential,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::{module, Context, MessageBusExt, Module};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

mod drep_distribution_publisher;
mod state;

use crate::drep_distribution_publisher::DRepDistributionPublisher;
use state::State;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.certificates";
const DEFAULT_HANDLE_TOPIC: &str = "rest.get.drep-state.*";
const DEFAULT_DREP_DISTRIBUTION_TOPIC: &str = "cardano.drep.distribution";

/// DRep State module
#[module(
    message_type(Message),
    name = "drep-state",
    description = "In-memory DRep State from certificate events"
)]
pub struct DRepState;

fn decode_rest_drep_credential(id: &str) -> Result<DRepCredential> {
    if let Some(stripped) = id.strip_prefix("script=") {
        Ok(DRepCredential::ScriptHash(hex::decode(stripped)?))
    } else if let Some(stripped) = id.strip_prefix("address=") {
        Ok(DRepCredential::AddrKeyHash(hex::decode(stripped)?))
    } else {
        Err(anyhow!("Poorly formed url, 'script=<hex key hash>' or 'address=<hex key hash>' DRep credential should be provided"))
    }
}

fn perform_rest_request(state: &State, path: &str) -> Result<String> {
    let request = match path.rfind('/') {
        None => return Err(anyhow!("Poorly formed url, '/' expected.")),
        Some(suffix_start) => &path[suffix_start + 1..],
    };

    if request == "list" {
        Ok(format!("DRep list: {:?}", state.list()))
    } else {
        let cred = decode_rest_drep_credential(request)?;
        match state.get_drep(&cred) {
            Some(drep) => Ok(format!(
                "DRep {:?}: deposit={}, anchor={:?}",
                cred, drep.deposit, drep.anchor
            )),
            None => Ok(format!("No DRep {:?}", cred)),
        }
    }
}

impl DRepState {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let subscribe_topic = config
            .get_string("subscribe-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let handle_topic = config
            .get_string("handle-topic")
            .unwrap_or(DEFAULT_HANDLE_TOPIC.to_string());
        info!("Creating request handler on '{handle_topic}'");

        let drep_distribution_topic = config
            .get_string("publish-drep-distribution-topic")
            .unwrap_or(DEFAULT_DREP_DISTRIBUTION_TOPIC.to_string());
        info!("Creating DRep distribution publisher on '{drep_distribution_topic}'");

        let publisher = DRepDistributionPublisher::new(context.clone(), drep_distribution_topic);
        let state = Arc::new(Mutex::new(State::new(Some(publisher))));

        // Subscribe for certificate messages
        let state1 = state.clone();
        let mut subscription = context.message_bus.register(&subscribe_topic).await?;
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::TxCertificates(tx_cert_msg))) => {
                        let mut state = state1.lock().await;
                        state
                            .handle(block_info, tx_cert_msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }
        });

        // Handle requests for single DRep state
        let state2 = state.clone();
        context
            .message_bus
            .handle(&handle_topic, move |message: Arc<Message>| {
                let state = state2.clone();
                async move {
                    let response = match message.as_ref() {
                        Message::RESTRequest(request) => {
                            info!("REST received {} {}", request.method, request.path);
                            let state = state.lock().await;

                            match perform_rest_request(&state, &request.path) {
                                Ok(response) => RESTResponse::with_text(200, &response),
                                Err(error) => {
                                    error!("DRep REST request error: {error:?}");
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
            })?;

        // Ticker to log stats
        let mut subscription = context.message_bus.register(&subscribe_topic).await?;
        let state3 = state.clone();
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        state3
                            .lock()
                            .await
                            .tick()
                            .await
                            .inspect_err(|e| error!("Tick error: {e}"))
                            .ok();
                    }
                }
            }
        });

        Ok(())
    }
}
