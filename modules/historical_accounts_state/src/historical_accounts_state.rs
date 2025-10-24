//! Acropolis historical accounts state module for Caryatid
//! Manages optional state data needed for Blockfrost alignment

use acropolis_common::queries::accounts::{
    AccountsStateQuery, AccountsStateQueryResponse, DEFAULT_HISTORICAL_ACCOUNTS_QUERY_TOPIC,
};
use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    BlockInfo, BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{message_bus::Subscription, module, Context, Module};
use config::Config;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{error, info, info_span};

mod state;
use state::State;

use crate::immutable_historical_account_store::ImmutableHistoricalAccountStore;
use crate::state::HistoricalAccountsConfig;
mod immutable_historical_account_store;
mod volatile_historical_accounts;

const DEFAULT_REWARDS_SUBSCRIBE_TOPIC: &str = "cardano.stake.reward.deltas";
const DEFAULT_TX_CERTIFICATES_SUBSCRIBE_TOPIC: &str = "cardano.certificates";
const DEFAULT_WITHDRAWALS_SUBSCRIBE_TOPIC: &str = "cardano.withdrawals";
const DEFAULT_ADDRESS_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("address-deltas-subscribe-topic", "cardano.address.delta");
const DEFAULT_PARAMETERS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("parameters-subscribe-topic", "cardano.protocol.parameters");

// Configuration defaults
const DEFAULT_HISTORICAL_ACCOUNTS_DB_PATH: (&str, &str) = ("db-path", "./db");
const DEFAULT_STORE_REWARDS_HISTORY: (&str, bool) = ("store-rewards-history", false);
const DEFAULT_STORE_ACTIVE_STAKE_HISTORY: (&str, bool) = ("store-active-stake-history", false);
const DEFAULT_STORE_REGISTRATION_HISTORY: (&str, bool) = ("store-registration-history", false);
const DEFAULT_STORE_DELEGATION_HISTORY: (&str, bool) = ("store-delegation-history", false);
const DEFAULT_STORE_MIR_HISTORY: (&str, bool) = ("store-mir-history", false);
const DEFAULT_STORE_WITHDRAWAL_HISTORY: (&str, bool) = ("store-withdrawal-history", false);
const DEFAULT_STORE_ADDRESSES: (&str, bool) = ("store-addresses", false);

/// Historical Accounts State module
#[module(
    message_type(Message),
    name = "historical-accounts-state",
    description = "Historical accounts state for Blockfrost compatibility"
)]
pub struct HistoricalAccountsState;

impl HistoricalAccountsState {
    /// Async run loop
    async fn run(
        state_mutex: Arc<Mutex<State>>,
        mut rewards_subscription: Box<dyn Subscription<Message>>,
        mut certs_subscription: Box<dyn Subscription<Message>>,
        mut withdrawals_subscription: Box<dyn Subscription<Message>>,
        mut address_deltas_subscription: Box<dyn Subscription<Message>>,
        mut params_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let _ = params_subscription.read().await?;
        info!("Consumed initial genesis params from params_subscription");
        let _ = address_deltas_subscription.read().await?;
        info!("Consumed initial address deltas from address_deltas_subscription");

        // Background task to persist epochs sequentially
        const MAX_PENDING_PERSISTS: usize = 1;
        let (persist_tx, mut persist_rx) = mpsc::channel::<(
            u32,
            Arc<ImmutableHistoricalAccountStore>,
            HistoricalAccountsConfig,
        )>(MAX_PENDING_PERSISTS);
        tokio::spawn(async move {
            while let Some((epoch, store, config)) = persist_rx.recv().await {
                if let Err(e) = store.persist_epoch(epoch, &config).await {
                    error!("failed to persist epoch {epoch}: {e}");
                }
            }
        });
        // Main loop of synchronised messages
        loop {
            // Create all per-block message futures upfront before processing messages sequentially
            let certs_message_f = certs_subscription.read();
            let address_deltas_message_f = address_deltas_subscription.read();
            let withdrawals_message_f = withdrawals_subscription.read();

            let mut current_block: Option<BlockInfo> = None;

            // Use certs_message as the synchroniser
            let (_, certs_message) = certs_message_f.await?;
            let new_epoch = match certs_message.as_ref() {
                Message::Cardano((block_info, _)) => {
                    // Handle rollbacks on this topic only
                    let mut state = state_mutex.lock().await;
                    if block_info.status == BlockStatus::RolledBack {
                        state.volatile.rollback_before(block_info.number);
                        state.volatile.next_block();
                    }

                    current_block = Some(block_info.clone());
                    block_info.new_epoch && block_info.epoch > 0
                }
                _ => false,
            };

            // Read from epoch-boundary messages only when it's a new epoch
            if new_epoch {
                let (_, params_msg) = params_subscription.read().await?;
                if let Message::Cardano((ref block_info, CardanoMessage::ProtocolParams(params))) =
                    params_msg.as_ref()
                {
                    Self::check_sync(&current_block, &block_info);
                    let mut state = state_mutex.lock().await;
                    state.volatile.start_new_epoch(block_info.number);
                    if let Some(shelley) = &params.params.shelley {
                        state.volatile.update_k(shelley.security_param);
                    }
                }

                let (_, rewards_msg) = rewards_subscription.read().await?;
                if let Message::Cardano((
                    block_info,
                    CardanoMessage::StakeRewardDeltas(rewards_msg),
                )) = rewards_msg.as_ref()
                {
                    Self::check_sync(&current_block, &block_info);
                    let mut state = state_mutex.lock().await;
                    state
                        .handle_rewards(rewards_msg)
                        .inspect_err(|e| error!("Reward deltas handling error: {e:#}"))
                        .ok();
                }
            }

            // Now handle the certs_message properly
            match certs_message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::TxCertificates(tx_certs_msg))) => {
                    let span = info_span!(
                        "historical_account_state.handle_certs",
                        block = block_info.number
                    );
                    let _entered = span.enter();

                    Self::check_sync(&current_block, &block_info);
                    let mut state = state_mutex.lock().await;
                    state.handle_tx_certificates(tx_certs_msg, block_info.epoch as u32);
                }

                _ => error!("Unexpected message type: {certs_message:?}"),
            }

            // Handle withdrawals
            let (_, message) = withdrawals_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::Withdrawals(withdrawals_msg))) => {
                    let span = info_span!(
                        "historical_account_state.handle_withdrawals",
                        block = block_info.number
                    );
                    let _entered = span.enter();

                    Self::check_sync(&current_block, &block_info);
                    let mut state = state_mutex.lock().await;
                    state.handle_withdrawals(withdrawals_msg);
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            // Handle address deltas
            let (_, message) = address_deltas_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::AddressDeltas(deltas_msg))) => {
                    let span = info_span!(
                        "historical_account_state.handle_address_deltas",
                        block = block_info.number
                    );
                    let _entered = span.enter();

                    Self::check_sync(&current_block, &block_info);
                    {
                        let mut state = state_mutex.lock().await;
                        state
                            .handle_address_deltas(deltas_msg)
                            .inspect_err(|e| error!("AddressDeltas handling error: {e:#}"))
                            .ok();
                    }
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            // Prune volatile and persist if needed
            if let Some(current_block) = current_block {
                let should_prune = {
                    let state = state_mutex.lock().await;
                    state.ready_to_prune(&current_block)
                };

                if should_prune {
                    let (store, cfg) = {
                        let mut state: tokio::sync::MutexGuard<'_, State> =
                            state_mutex.lock().await;
                        state.prune_volatile().await;
                        (state.immutable.clone(), state.config.clone())
                    };

                    info!("sending persist for epoch {}", current_block.epoch);
                    if let Err(e) = persist_tx.send((current_block.epoch as u32, store, cfg)).await
                    {
                        panic!("persistence worker crashed: {e}");
                    }

                    info!("persist send completed");
                }
            }

            {
                let mut state = state_mutex.lock().await;
                state.volatile.next_block();
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
        let tx_certificates_topic = config
            .get_string("tx-certificates-topic")
            .unwrap_or(DEFAULT_TX_CERTIFICATES_SUBSCRIBE_TOPIC.to_string());
        info!("Creating Tx certificates subscriber on '{tx_certificates_topic}'");

        let withdrawals_topic = config
            .get_string("withdrawals-topic")
            .unwrap_or(DEFAULT_WITHDRAWALS_SUBSCRIBE_TOPIC.to_string());
        info!("Creating withdrawals subscriber on '{withdrawals_topic}'");

        let rewards_topic = config
            .get_string("rewards-topic")
            .unwrap_or(DEFAULT_REWARDS_SUBSCRIBE_TOPIC.to_string());

        let address_deltas_topic = config
            .get_string(DEFAULT_ADDRESS_DELTAS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_ADDRESS_DELTAS_SUBSCRIBE_TOPIC.1.to_string());

        let params_topic = config
            .get_string(DEFAULT_PARAMETERS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_PARAMETERS_SUBSCRIBE_TOPIC.1.to_string());

        // Query topics
        let historical_accounts_query_topic = config
            .get_string(DEFAULT_HISTORICAL_ACCOUNTS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_HISTORICAL_ACCOUNTS_QUERY_TOPIC.1.to_string());
        info!(
            "Creating query handler on '{}'",
            historical_accounts_query_topic
        );

        let storage_config = HistoricalAccountsConfig {
            db_path: config
                .get_string(DEFAULT_HISTORICAL_ACCOUNTS_DB_PATH.0)
                .unwrap_or(DEFAULT_HISTORICAL_ACCOUNTS_DB_PATH.1.to_string()),
            store_rewards_history: config
                .get_bool(DEFAULT_STORE_REWARDS_HISTORY.0)
                .unwrap_or(DEFAULT_STORE_REWARDS_HISTORY.1),
            store_active_stake_history: config
                .get_bool(DEFAULT_STORE_ACTIVE_STAKE_HISTORY.0)
                .unwrap_or(DEFAULT_STORE_ACTIVE_STAKE_HISTORY.1),
            store_delegation_history: config
                .get_bool(DEFAULT_STORE_DELEGATION_HISTORY.0)
                .unwrap_or(DEFAULT_STORE_DELEGATION_HISTORY.1),
            store_registration_history: config
                .get_bool(DEFAULT_STORE_REGISTRATION_HISTORY.0)
                .unwrap_or(DEFAULT_STORE_REGISTRATION_HISTORY.1),
            store_withdrawal_history: config
                .get_bool(DEFAULT_STORE_WITHDRAWAL_HISTORY.0)
                .unwrap_or(DEFAULT_STORE_WITHDRAWAL_HISTORY.1),
            store_mir_history: config
                .get_bool(DEFAULT_STORE_MIR_HISTORY.0)
                .unwrap_or(DEFAULT_STORE_MIR_HISTORY.1),
            store_addresses: config
                .get_bool(DEFAULT_STORE_ADDRESSES.0)
                .unwrap_or(DEFAULT_STORE_ADDRESSES.1),
        };

        // Initalize state
        let state = State::new(storage_config).await?;
        let state_mutex = Arc::new(Mutex::new(state));
        let state_query = state_mutex.clone();

        context.handle(&historical_accounts_query_topic, move |message| {
            let state = state_query.clone();
            async move {
                let Message::StateQuery(StateQuery::Accounts(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Accounts(
                        AccountsStateQueryResponse::Error(
                            "Invalid message for accounts-state".into(),
                        ),
                    )));
                };

                let response = match query {
                    AccountsStateQuery::GetAccountRegistrationHistory { account } => {
                        match state.lock().await.get_registration_history(&account).await {
                            Ok(registrations) => {
                                AccountsStateQueryResponse::AccountRegistrationHistory(
                                    registrations,
                                )
                            }
                            Err(e) => AccountsStateQueryResponse::Error(e.to_string()),
                        }
                    }
                    AccountsStateQuery::GetAccountDelegationHistory { account } => {
                        match state.lock().await.get_delegation_history(&account).await {
                            Ok(delegations) => {
                                AccountsStateQueryResponse::AccountDelegationHistory(delegations)
                            }
                            Err(e) => AccountsStateQueryResponse::Error(e.to_string()),
                        }
                    }
                    AccountsStateQuery::GetAccountMIRHistory { account } => {
                        match state.lock().await.get_mir_history(&account).await {
                            Ok(mirs) => AccountsStateQueryResponse::AccountMIRHistory(mirs),
                            Err(e) => AccountsStateQueryResponse::Error(e.to_string()),
                        }
                    }
                    AccountsStateQuery::GetAccountWithdrawalHistory { account } => {
                        match state.lock().await.get_withdrawal_history(&account).await {
                            Ok(withdrawals) => {
                                AccountsStateQueryResponse::AccountWithdrawalHistory(withdrawals)
                            }
                            Err(e) => AccountsStateQueryResponse::Error(e.to_string()),
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

        // Subscribe
        let rewards_subscription = context.subscribe(&rewards_topic).await?;
        let certs_subscription = context.subscribe(&tx_certificates_topic).await?;
        let withdrawals_subscription = context.subscribe(&withdrawals_topic).await?;
        let address_deltas_subscription = context.subscribe(&address_deltas_topic).await?;
        let params_subscription = context.subscribe(&params_topic).await?;

        // Start run task
        context.run(async move {
            Self::run(
                state_mutex,
                rewards_subscription,
                certs_subscription,
                withdrawals_subscription,
                address_deltas_subscription,
                params_subscription,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
