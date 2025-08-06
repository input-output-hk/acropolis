//! Acropolis accounts state module for Caryatid
//! Manages stake and reward accounts state

use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    rest_helper::handle_rest,
    state_history::StateHistory,
    BlockInfo, BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{message_bus::Subscription, module, Context, Module};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod drep_distribution_publisher;
use drep_distribution_publisher::DRepDistributionPublisher;
mod spo_distribution_publisher;
use spo_distribution_publisher::SPODistributionPublisher;
mod state;
use state::State;
mod monetary;
mod rest;
mod rewards;
mod snapshot;
use acropolis_common::queries::accounts::{
    AccountInfo, AccountsStateQuery, AccountsStateQueryResponse,
};
use rest::handle_pots;

const DEFAULT_SPO_STATE_TOPIC: &str = "cardano.spo.state";
const DEFAULT_EPOCH_ACTIVITY_TOPIC: &str = "cardano.epoch.activity";
const DEFAULT_TX_CERTIFICATES_TOPIC: &str = "cardano.certificates";
const DEFAULT_WITHDRAWALS_TOPIC: &str = "cardano.withdrawals";
const DEFAULT_POT_DELTAS_TOPIC: &str = "cardano.pot.deltas";
const DEFAULT_STAKE_DELTAS_TOPIC: &str = "cardano.stake.deltas";
const DEFAULT_DREP_STATE_TOPIC: &str = "cardano.drep.state";
const DEFAULT_DREP_DISTRIBUTION_TOPIC: &str = "cardano.drep.distribution";
const DEFAULT_SPO_DISTRIBUTION_TOPIC: &str = "cardano.spo.distribution";
const DEFAULT_PROTOCOL_PARAMETERS_TOPIC: &str = "cardano.protocol.parameters";

const DEFAULT_HANDLE_POTS_TOPIC: (&str, &str) = ("handle-topic-pots", "rest.get.pots");

/// Accounts State module
#[module(
    message_type(Message),
    name = "accounts-state",
    description = "Stake and reward accounts state"
)]
pub struct AccountsState;

impl AccountsState {
    /// Async run loop
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        mut drep_publisher: DRepDistributionPublisher,
        mut spo_publisher: SPODistributionPublisher,
        mut spos_subscription: Box<dyn Subscription<Message>>,
        mut ea_subscription: Box<dyn Subscription<Message>>,
        mut certs_subscription: Box<dyn Subscription<Message>>,
        mut withdrawals_subscription: Box<dyn Subscription<Message>>,
        mut pots_subscription: Box<dyn Subscription<Message>>,
        mut stake_subscription: Box<dyn Subscription<Message>>,
        mut drep_state_subscription: Box<dyn Subscription<Message>>,
        mut parameters_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        // Get the stake address deltas from the genesis bootstrap, which we know
        // don't contain any stake, plus an extra parameter state (!unexplained)
        // !TODO this seems overly specific to our startup process
        let _ = stake_subscription.read().await?;
        let _ = parameters_subscription.read().await?;

        // Initialisation messages
        {
            let mut state = history.lock().await.get_current_state();
            let mut current_block: Option<BlockInfo> = None;

            let pots_message_f = pots_subscription.read();

            // Handle pots
            let (_, message) = pots_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::PotDeltas(pot_deltas_msg))) => {
                    state
                        .handle_pot_deltas(pot_deltas_msg)
                        .inspect_err(|e| error!("Pots handling error: {e:#}"))
                        .ok();

                    current_block = Some(block_info.clone());
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            if let Some(block_info) = current_block {
                history.lock().await.commit(&block_info, state);
            }
        }

        // Main loop of synchronised messages
        loop {
            // Get a mutable state
            let mut state = history.lock().await.get_current_state();

            // Read per-block topics in parallel
            let certs_message_f = certs_subscription.read();
            let stake_message_f = stake_subscription.read();
            let withdrawals_message_f = withdrawals_subscription.read();
            let mut current_block: Option<BlockInfo> = None;

            // Use certs_message as the synchroniser, but we have to handle it after the
            // epoch things, because they apply to the new epoch, not the last
            let (_, certs_message) = certs_message_f.await?;
            let new_epoch = match certs_message.as_ref() {
                Message::Cardano((block_info, _)) => {
                    // Handle rollbacks on this topic only
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(&block_info);
                    }

                    current_block = Some(block_info.clone());
                    block_info.new_epoch && block_info.epoch > 0
                }
                _ => false,
            };

            // Read from epoch-boundary messages only when it's a new epoch
            if new_epoch {
                let dreps_message_f = drep_state_subscription.read();
                let spos_message_f = spos_subscription.read();
                let ea_message_f = ea_subscription.read();
                let params_message_f = parameters_subscription.read();

                // Handle DRep
                let (_, message) = dreps_message_f.await?;
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::DRepState(dreps_msg))) => {
                        let span = info_span!(
                            "account_state.handle_drep_state",
                            block = block_info.number
                        );
                        async {
                            Self::check_sync(&current_block, &block_info);
                            state.handle_drep_state(&dreps_msg);

                            let drdd = state.generate_drdd();
                            if let Err(e) = drep_publisher.publish_drdd(block_info, drdd).await {
                                error!("Error publishing drep voting stake distribution: {e:#}")
                            }
                        }
                        .instrument(span)
                        .await;
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }

                // Handle SPOs
                let (_, message) = spos_message_f.await?;
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::SPOState(spo_msg))) => {
                        let span =
                            info_span!("account_state.handle_spo_state", block = block_info.number);
                        async {
                            Self::check_sync(&current_block, &block_info);
                            state
                                .handle_spo_state(spo_msg)
                                .inspect_err(|e| error!("SPOState handling error: {e:#}"))
                                .ok();

                            let spdd = state.generate_spdd();
                            if let Err(e) = spo_publisher.publish_spdd(block_info, spdd).await {
                                error!("Error publishing SPO stake distribution: {e:#}")
                            }
                        }
                        .instrument(span)
                        .await;
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }

                // Handle epoch activity
                let (_, message) = ea_message_f.await?;
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::EpochActivity(ea_msg))) => {
                        let span = info_span!(
                            "account_state.handle_epoch_activity",
                            block = block_info.number
                        );
                        async {
                            Self::check_sync(&current_block, &block_info);
                            state
                                .handle_epoch_activity(ea_msg)
                                .await
                                .inspect_err(|e| error!("EpochActivity handling error: {e:#}"))
                                .ok();
                        }
                        .instrument(span)
                        .await;
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }

                // Update parameters - *after* reward calculation in epoch-activity above
                // ready for the *next* epoch boundary
                let (_, message) = params_message_f.await?;
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::ProtocolParams(params_msg))) => {
                        let span = info_span!(
                            "account_state.handle_parameters",
                            block = block_info.number
                        );
                        async {
                            Self::check_sync(&current_block, &block_info);
                            if let Some(ref block) = current_block {
                                if block.number != block_info.number {
                                    error!(
                                        expected = block.number,
                                        received = block_info.number,
                                        "Certificate and parameters messages re-ordered!"
                                    );
                                }
                            }

                            state
                                .handle_parameters(params_msg)
                                .inspect_err(|e| error!("Messaging handling error: {e}"))
                                .ok();
                        }
                        .instrument(span)
                        .await;
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }

            // Now handle the certs_message properly
            match certs_message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::TxCertificates(tx_certs_msg))) => {
                    let span = info_span!("account_state.handle_certs", block = block_info.number);
                    async {
                        Self::check_sync(&current_block, &block_info);
                        state
                            .handle_tx_certificates(tx_certs_msg)
                            .inspect_err(|e| error!("TxCertificates handling error: {e:#}"))
                            .ok();
                    }
                    .instrument(span)
                    .await;
                }

                _ => error!("Unexpected message type: {certs_message:?}"),
            }

            // Handle withdrawals
            let (_, message) = withdrawals_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::Withdrawals(withdrawals_msg))) => {
                    let span = info_span!(
                        "account_state.handle_withdrawals",
                        block = block_info.number
                    );
                    async {
                        Self::check_sync(&current_block, &block_info);
                        state
                            .handle_withdrawals(withdrawals_msg)
                            .inspect_err(|e| error!("Withdrawals handling error: {e:#}"))
                            .ok();
                    }
                    .instrument(span)
                    .await;
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            // Handle stake address deltas
            let (_, message) = stake_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::StakeAddressDeltas(deltas_msg))) => {
                    let span = info_span!(
                        "account_state.handle_stake_deltas",
                        block = block_info.number
                    );
                    async {
                        Self::check_sync(&current_block, &block_info);
                        state
                            .handle_stake_deltas(deltas_msg)
                            .inspect_err(|e| error!("StakeAddressDeltas handling error: {e:#}"))
                            .ok();
                    }
                    .instrument(span)
                    .await;
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            // Commit the new state
            if let Some(block_info) = current_block {
                history.lock().await.commit(&block_info, state);
            }
        }
    }

    /// Check for synchronisation
    fn check_sync(expected: &Option<BlockInfo>, actual: &BlockInfo) {
        if let Some(ref block) = expected {
            if block.number != actual.number {
                error!(
                    expected = block.number,
                    actual = actual.number,
                    "Messages out of sync"
                );
            }
        }
    }

    /// Async initialisation
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration

        // Subscription topics
        let spo_state_topic =
            config.get_string("spo-state-topic").unwrap_or(DEFAULT_SPO_STATE_TOPIC.to_string());
        info!("Creating SPO state subscriber on '{spo_state_topic}'");

        let epoch_activity_topic = config
            .get_string("epoch-activity-topic")
            .unwrap_or(DEFAULT_EPOCH_ACTIVITY_TOPIC.to_string());
        info!("Creating epoch activity subscriber on '{epoch_activity_topic}'");

        let tx_certificates_topic = config
            .get_string("tx-certificates-topic")
            .unwrap_or(DEFAULT_TX_CERTIFICATES_TOPIC.to_string());
        info!("Creating Tx certificates subscriber on '{tx_certificates_topic}'");

        let withdrawals_topic =
            config.get_string("withdrawals-topic").unwrap_or(DEFAULT_WITHDRAWALS_TOPIC.to_string());
        info!("Creating withdrawals subscriber on '{withdrawals_topic}'");

        let pot_deltas_topic =
            config.get_string("pot-deltas-topic").unwrap_or(DEFAULT_POT_DELTAS_TOPIC.to_string());
        info!("Creating pots subscriber on '{pot_deltas_topic}'");

        let stake_deltas_topic = config
            .get_string("stake-deltas-topic")
            .unwrap_or(DEFAULT_STAKE_DELTAS_TOPIC.to_string());
        info!("Creating stake deltas subscriber on '{stake_deltas_topic}'");

        let drep_state_topic =
            config.get_string("drep-state-topic").unwrap_or(DEFAULT_DREP_STATE_TOPIC.to_string());
        info!("Creating DRep state subscriber on '{drep_state_topic}'");

        let parameters_topic = config
            .get_string("protocol-parameters-topic")
            .unwrap_or(DEFAULT_PROTOCOL_PARAMETERS_TOPIC.to_string());

        // Publishing topics
        let drep_distribution_topic = config
            .get_string("publish-drep-distribution-topic")
            .unwrap_or(DEFAULT_DREP_DISTRIBUTION_TOPIC.to_string());

        let spo_distribution_topic = config
            .get_string("publish-spo-distribution-topic")
            .unwrap_or(DEFAULT_SPO_DISTRIBUTION_TOPIC.to_string());

        // REST handler topics
        let handle_pots_topic = config
            .get_string(DEFAULT_HANDLE_POTS_TOPIC.0)
            .unwrap_or(DEFAULT_HANDLE_POTS_TOPIC.1.to_string());
        info!("Creating request handler on '{}'", handle_pots_topic);

        // Create history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new("AccountsState")));
        let history_account_single = history.clone();
        let history_pots = history.clone();
        let history_tick = history.clone();

        context.handle("accounts-state", move |message| {
            let history = history_account_single.clone();
            async move {
                let Message::StateQuery(StateQuery::Accounts(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Accounts(
                        AccountsStateQueryResponse::Error(
                            "Invalid message for accounts-state".into(),
                        ),
                    )));
                };

                let guard = history.lock().await;
                let state = match guard.current() {
                    Some(s) => s,
                    None => {
                        return Arc::new(Message::StateQueryResponse(
                            StateQueryResponse::Accounts(AccountsStateQueryResponse::NotFound),
                        ));
                    }
                };

                let response = match query {
                    AccountsStateQuery::GetAccountInfo { stake_key } => {
                        if let Some(account) = state.get_stake_state(stake_key) {
                            AccountsStateQueryResponse::AccountInfo(AccountInfo {
                                utxo_value: account.utxo_value,
                                rewards: account.rewards,
                                delegated_spo: account.delegated_spo.clone(),
                                delegated_drep: account.delegated_drep.clone(),
                            })
                        } else {
                            AccountsStateQueryResponse::NotFound
                        }
                    }

                    _ => AccountsStateQueryResponse::Error(format!(
                        "Unimplemented query variant: {:?}",
                        query
                    )),
                };

                Arc::new(Message::StateQueryResponse(StateQueryResponse::Accounts(
                    response,
                )))
            }
        });

        handle_rest(context.clone(), &handle_pots_topic, move || {
            handle_pots(history_pots.clone())
        });

        // Ticker to log stats
        let mut tick_subscription = context.subscribe("clock.tick").await?;
        context.clone().run(async move {
            loop {
                let Ok((_, message)) = tick_subscription.read().await else {
                    return;
                };
                if let Message::Clock(message) = message.as_ref() {
                    if (message.number % 60) == 0 {
                        let span = info_span!("accounts_state.tick", number = message.number);
                        async {
                            if let Some(state) = history_tick.lock().await.current() {
                                state.tick().await.inspect_err(|e| error!("Tick error: {e}")).ok();
                            }
                        }
                        .instrument(span)
                        .await;
                    }
                }
            }
        });

        // Publishers
        let drep_publisher =
            DRepDistributionPublisher::new(context.clone(), drep_distribution_topic);
        let spo_publisher = SPODistributionPublisher::new(context.clone(), spo_distribution_topic);

        // Subscribe
        let spos_subscription = context.subscribe(&spo_state_topic).await?;
        let ea_subscription = context.subscribe(&epoch_activity_topic).await?;
        let certs_subscription = context.subscribe(&tx_certificates_topic).await?;
        let withdrawals_subscription = context.subscribe(&withdrawals_topic).await?;
        let pot_deltas_subscription = context.subscribe(&pot_deltas_topic).await?;
        let stake_subscription = context.subscribe(&stake_deltas_topic).await?;
        let drep_state_subscription = context.subscribe(&drep_state_topic).await?;
        let parameters_subscription = context.subscribe(&parameters_topic).await?;

        // Start run task
        context.run(async move {
            Self::run(
                history,
                drep_publisher,
                spo_publisher,
                spos_subscription,
                ea_subscription,
                certs_subscription,
                withdrawals_subscription,
                pot_deltas_subscription,
                stake_subscription,
                drep_state_subscription,
                parameters_subscription,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
