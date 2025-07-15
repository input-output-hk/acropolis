//! Acropolis DRep State module for Caryatid
//! Accepts certificate events and derives the DRep State in memory

use acropolis_common::{
    messages::{CardanoMessage, DRepStateMessage, Message},
    rest_helper::{handle_rest, handle_rest_with_parameter},
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

mod rest;
use rest::{handle_drep, handle_list};
mod state;
use state::State;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.certificates";
const LIST_HANDLE_TOPIC: (&str, &str) = ("handle-topic-drep-list", "rest.get.dreps");
const DREP_HANDLE_TOPIC: (&str, &str) = ("handle-topic-drep-single", "rest.get.dreps.*");
const DEFAULT_DREP_STATE_TOPIC: &str = "cardano.drep.state";

/// DRep State module
#[module(
    message_type(Message),
    name = "drep-state",
    description = "In-memory DRep State from certificate events"
)]
pub struct DRepState;

impl DRepState {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let subscribe_topic =
            config.get_string("subscribe-topic").unwrap_or(DEFAULT_SUBSCRIBE_TOPIC.to_string());
        info!("Creating subscriber on '{subscribe_topic}'");

        let handle_list_topic =
            config.get_string(LIST_HANDLE_TOPIC.0).unwrap_or(LIST_HANDLE_TOPIC.1.to_string());
        info!("Creating request handler on '{}'", handle_list_topic);

        let handle_drep_topic =
            config.get_string(DREP_HANDLE_TOPIC.0).unwrap_or(DREP_HANDLE_TOPIC.1.to_string());
        info!("Creating request handler on '{}'", handle_drep_topic);

        let drep_state_topic = config
            .get_string("publish-drep-state-topic")
            .unwrap_or(DEFAULT_DREP_STATE_TOPIC.to_string());
        info!("Creating DRep state publisher on '{drep_state_topic}'");

        let state = Arc::new(Mutex::new(State::new()));

        // Subscribe for certificate messages
        let state1 = state.clone();
        let mut subscription = context.subscribe(&subscribe_topic).await?;
        let context_subscribe = context.clone();
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::TxCertificates(tx_cert_msg))) => {
                        let mut state = state1.lock().await;
                        state
                            .handle(&tx_cert_msg)
                            .await
                            .inspect_err(|e| error!("Messaging handling error: {e}"))
                            .ok();

                        if block_info.new_epoch && block_info.epoch > 0 {
                            // publish DRep state at end of epoch
                            let dreps = state.active_drep_list();
                            let message = Message::Cardano((
                                block_info.clone(),
                                CardanoMessage::DRepState(DRepStateMessage {
                                    epoch: block_info.epoch,
                                    dreps,
                                }),
                            ));
                            context_subscribe
                                .publish(&drep_state_topic, Arc::new(message))
                                .await
                                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                        }
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }
        });

        let state_list = state.clone();
        handle_rest(context.clone(), &handle_list_topic, move || {
            let state = state_list.clone();
            async move { Ok(handle_list(state).await) }
        });

        let state_single = state.clone();
        handle_rest_with_parameter(context.clone(), &handle_drep_topic, move |param| {
            handle_drep(state_single.clone(), param[0].to_string())
        });

        // Ticker to log stats
        let mut subscription = context.subscribe(&subscribe_topic).await?;
        let state2 = state.clone();
        context.run(async move {
            loop {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        state2
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
