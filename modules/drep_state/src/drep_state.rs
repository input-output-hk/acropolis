//! Acropolis DRep State module for Caryatid
//! Accepts certificate events and derives the DRep State in memory

use acropolis_common::{
    messages::{CardanoMessage, DRepStateMessage, Message, StateQuery, StateQueryResponse},
    queries::governance::{
        DRepInfo, DRepsList, GovernanceStateQuery, GovernanceStateQueryResponse,
    },
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info};

mod state;
use state::State;

const DEFAULT_SUBSCRIBE_TOPIC: &str = "cardano.certificates";
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

        let query_state = state.clone();
        context.handle("drep-state", move |message| {
            let state_handle = query_state.clone(); // your shared Arc<Mutex<State>>
            async move {
                let Message::StateQuery(StateQuery::Governance(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Governance(
                        GovernanceStateQueryResponse::Error(
                            "Invalid message for governance-state".into(),
                        ),
                    )));
                };

                let locked = state_handle.lock().await;

                let response = match query {
                    GovernanceStateQuery::GetDRepsList => {
                        let dreps = locked.list();
                        GovernanceStateQueryResponse::DRepsList(DRepsList { dreps })
                    }
                    GovernanceStateQuery::GetDRepInfo { drep_credential } => {
                        match locked.get_drep(&drep_credential) {
                            Some(record) => GovernanceStateQueryResponse::DRepInfo(DRepInfo {
                                deposit: record.deposit,
                                anchor: record.anchor.clone(),
                            }),
                            None => GovernanceStateQueryResponse::NotFound,
                        }
                    }
                    _ => GovernanceStateQueryResponse::Error(format!(
                        "Unimplemented governance query: {query:?}"
                    )),
                };

                Arc::new(Message::StateQueryResponse(StateQueryResponse::Governance(
                    response,
                )))
            }
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
