//! Acropolis DRDD state module for Caryatid
//! Stores historical DRep delegation distributions
use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper},
    configuration::{get_bool_flag, get_string_flag},
    declare_cardano_reader,
    messages::{CardanoMessage, DRepStakeDistributionMessage, Message, StateTransitionMessage},
    rest_helper::handle_rest_with_query_parameters,
    state_history::{StateHistory, StateHistoryStore},
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{info, info_span, Instrument};
mod state;
use state::State;
mod rest;
use rest::handle_drdd;

const DEFAULT_HANDLE_DRDD_TOPIC: (&str, &str) = ("handle-topic-drdd", "rest.get.drdd");
const DEFAULT_STORE_DRDD: (&str, bool) = ("store-drdd", false);

declare_cardano_reader!(
    DRDDReader,
    "drep-distribution-subscribe-topic",
    "cardano.drep.distribution",
    DRepStakeDistribution,
    DRepStakeDistributionMessage
);

/// DRDD State module
#[module(
    message_type(Message),
    name = "drdd-state",
    description = "DRep Delegation Distribution State Tracker"
)]

pub struct DRDDState;

impl DRDDState {
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut drdd_reader: DRDDReader,
    ) -> anyhow::Result<()> {
        loop {
            let mut state = history.lock().await.get_or_init_with(State::new);

            let primary = PrimaryRead::from_read(drdd_reader.read_with_rollbacks().await?);

            if primary.is_rollback() {
                state = history.lock().await.get_rolled_back_state(primary.block_info().epoch);
            }

            if let Some(msg) = primary.message() {
                state.apply_drdd_snapshot(
                    msg.drdd.dreps.iter().map(|(k, v)| (k.clone(), *v)),
                    msg.drdd.abstain,
                    msg.drdd.no_confidence,
                );
                history.lock().await.commit(primary.block_info().epoch, state);
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let handle_drdd_topic = get_string_flag(&config, DEFAULT_HANDLE_DRDD_TOPIC);
        info!("Creating request handler on '{}'", handle_drdd_topic);

        let store_drdd = get_bool_flag(&config, DEFAULT_STORE_DRDD);

        let history_opt = if store_drdd {
            let history = Arc::new(Mutex::new(StateHistory::<State>::new(
                "drdd_state",
                StateHistoryStore::Unbounded,
            )));

            // Subscribe for drdd messages from accounts_state
            let history_handler = history.clone();
            let drdd_reader = DRDDReader::new(&context, &config).await?;
            context.run(Self::run(history_handler, drdd_reader));

            // Ticker to log stats
            let mut tick_subscription = context.subscribe("clock.tick").await?;
            let history_logger = history.clone();
            context.run(async move {
                loop {
                    let Ok((_, message)) = tick_subscription.read().await else {
                        return;
                    };

                    if let Message::Clock(clock) = message.as_ref() {
                        if clock.number % 60 == 0 {
                            let span = info_span!("drdd_state.tick", number = clock.number);
                            async {
                                let locked = history_logger.lock().await;
                                if let Some(state) = locked.current() {
                                    state.tick(locked.len());
                                }
                            }
                            .instrument(span)
                            .await;
                        }
                    }
                }
            });
            Some(history)
        } else {
            None
        };

        // handle spdd query
        let history_query = history_opt.clone();
        // Register /drdd REST endpoint
        handle_rest_with_query_parameters(context.clone(), &handle_drdd_topic, move |params| {
            let history_rest = history_query.clone();
            handle_drdd(history_rest, params)
        });

        Ok(())
    }
}
