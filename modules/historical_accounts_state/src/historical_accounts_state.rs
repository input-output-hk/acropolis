//! Acropolis historical accounts state module for Caryatid
//! Manages optional state data needed for Blockfrost alignment

use acropolis_common::caryatid::{PrimaryRead, RollbackWrapper};
use acropolis_common::configuration::{get_bool_flag, get_string_flag, StartupMode};
use acropolis_common::declare_cardano_reader;
use acropolis_common::messages::{CardanoMessage, Message, StateQuery, StateQueryResponse};
use acropolis_common::messages::{
    ProtocolParamsMessage, StakeAddressDeltasMessage, StakeRewardDeltasMessage,
    StateTransitionMessage, TxCertificatesMessage, WithdrawalsMessage,
};
use acropolis_common::queries::accounts::{
    AccountsStateQuery, AccountsStateQueryResponse, DEFAULT_HISTORICAL_ACCOUNTS_QUERY_TOPIC,
};
use acropolis_common::queries::errors::QueryError;
use anyhow::{bail, Result};
use caryatid_sdk::{message_bus::Subscription, module, Context};
use config::Config;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info};

mod state;
use state::State;

use crate::immutable_historical_account_store::ImmutableHistoricalAccountStore;
use crate::state::HistoricalAccountsConfig;
mod immutable_historical_account_store;
mod volatile_historical_accounts;

declare_cardano_reader!(
    RewardsReader,
    "rewards-subscribe-topic",
    "cardano.stake.reward.deltas",
    StakeRewardDeltas,
    StakeRewardDeltasMessage
);
declare_cardano_reader!(
    CertsReader,
    "certificates-subscribe-topic",
    "cardano.certificates",
    TxCertificates,
    TxCertificatesMessage
);
declare_cardano_reader!(
    WithdrawalsReader,
    "withdrawals-subscribe-topic",
    "cardano.withdrawals",
    Withdrawals,
    WithdrawalsMessage
);
declare_cardano_reader!(
    StakeDeltasReader,
    "stake-address-deltas-subscribe-topic",
    "cardano.stake.deltas",
    StakeAddressDeltas,
    StakeAddressDeltasMessage
);
declare_cardano_reader!(
    ParamsReader,
    "parameters-subscribe-topic",
    "cardano.protocol.parameters",
    ProtocolParams,
    ProtocolParamsMessage
);

// Configuration defaults
const DEFAULT_HISTORICAL_ACCOUNTS_DB_PATH: (&str, &str) = ("db-path", "./fjall-accounts");
const DEFAULT_CLEAR_ON_START: (&str, bool) = ("clear-on-start", true);
const DEFAULT_STORE_REWARDS_HISTORY: (&str, bool) = ("store-rewards-history", false);
const DEFAULT_STORE_ACTIVE_STAKE_HISTORY: (&str, bool) = ("store-active-stake-history", false);
const DEFAULT_STORE_REGISTRATION_HISTORY: (&str, bool) = ("store-registration-history", false);
const DEFAULT_STORE_DELEGATION_HISTORY: (&str, bool) = ("store-delegation-history", false);
const DEFAULT_STORE_MIR_HISTORY: (&str, bool) = ("store-mir-history", false);
const DEFAULT_STORE_WITHDRAWAL_HISTORY: (&str, bool) = ("store-withdrawal-history", false);
const DEFAULT_STORE_ADDRESSES: (&str, bool) = ("store-addresses", false);
const DEFAULT_STORE_TX_COUNT: (&str, bool) = ("store-tx-count", false);

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
        mut rewards_reader: RewardsReader,
        mut certs_reader: CertsReader,
        mut withdrawals_reader: WithdrawalsReader,
        mut stake_deltas_reader: StakeDeltasReader,
        mut params_reader: ParamsReader,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        if !is_snapshot_mode {
            match params_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal(_) => {}
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial params");
                }
            }
            debug!("Consumed initial genesis params from params_subscription");
            match stake_deltas_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal(_) => {}
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial stake deltas");
                }
            }
            debug!("Consumed initial stake deltas from stake_address_deltas_subscription");
        }

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
            // Use certs_message as the synchroniser
            let primary = PrimaryRead::from_read(certs_reader.read_with_rollbacks().await?);

            if primary.is_rollback() {
                let mut state = state_mutex.lock().await;
                state.volatile.rollback_before(primary.block_info().number);
                state.volatile.next_block();
            }

            // Init drains the epoch-0 bootstrap messages, so the main loop only
            // synchronizes these readers on rollbacks and real transitions.
            if primary.should_read_epoch_transition_messages() {
                match params_reader.read_with_rollbacks().await? {
                    RollbackWrapper::Normal((block_info, params)) => {
                        let mut state = state_mutex.lock().await;
                        state.volatile.start_new_epoch(block_info.number);
                        if let Some(shelley) = &params.params.shelley {
                            state.volatile.update_k(shelley.security_param);
                        }
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
            }

            // Rewards publish on real epoch transitions (>0) and rollbacks.
            if primary.should_read_epoch_transition_messages() {
                match rewards_reader.read_with_rollbacks().await? {
                    RollbackWrapper::Normal((block_info, rewards_msg)) => {
                        let mut state = state_mutex.lock().await;
                        state.handle_rewards(&rewards_msg, block_info.epoch as u32);
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
            }

            // Now handle the certs_message properly
            if let Some(tx_certs_msg) = primary.message() {
                let block_info = primary.block_info().clone();
                let mut state = state_mutex.lock().await;
                state.handle_tx_certificates(tx_certs_msg, block_info.epoch as u32);
            }

            // Handle withdrawals
            match withdrawals_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal((_, withdrawals_msg)) => {
                    let mut state = state_mutex.lock().await;
                    state.handle_withdrawals(&withdrawals_msg);
                }
                RollbackWrapper::Rollback(_) => {}
            }

            // Handle address deltas
            match stake_deltas_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal((_, deltas_msg)) => {
                    let mut state = state_mutex.lock().await;
                    state.handle_address_deltas(&deltas_msg);
                }
                RollbackWrapper::Rollback(_) => {}
            }

            // Prune volatile and persist if needed
            if primary.message().is_some() {
                let current_block = primary.block_info().clone();
                let should_prune = {
                    let state = state_mutex.lock().await;
                    state.ready_to_prune(&current_block)
                };

                if should_prune {
                    let (store, cfg) = {
                        let mut state = state_mutex.lock().await;
                        state.prune_volatile().await;
                        (state.immutable.clone(), state.config.clone())
                    };

                    if let Err(e) = persist_tx.send((current_block.epoch as u32, store, cfg)).await
                    {
                        panic!("persistence worker crashed: {e}");
                    }
                }
            }

            {
                let mut state = state_mutex.lock().await;
                state.volatile.next_block();
            }
        }
    }

    /// Async initialisation
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration
        let is_snapshot_mode = StartupMode::from_config(config.as_ref()).is_snapshot();

        // Query topics
        let historical_accounts_query_topic =
            get_string_flag(&config, DEFAULT_HISTORICAL_ACCOUNTS_QUERY_TOPIC);
        info!(
            "Creating query handler on '{}'",
            historical_accounts_query_topic
        );

        let storage_config = HistoricalAccountsConfig {
            db_path: get_string_flag(&config, DEFAULT_HISTORICAL_ACCOUNTS_DB_PATH),
            clear_on_start: get_bool_flag(&config, DEFAULT_CLEAR_ON_START),
            store_rewards_history: get_bool_flag(&config, DEFAULT_STORE_REWARDS_HISTORY),
            store_active_stake_history: get_bool_flag(&config, DEFAULT_STORE_ACTIVE_STAKE_HISTORY),
            store_delegation_history: get_bool_flag(&config, DEFAULT_STORE_DELEGATION_HISTORY),
            store_registration_history: get_bool_flag(&config, DEFAULT_STORE_REGISTRATION_HISTORY),
            store_withdrawal_history: get_bool_flag(&config, DEFAULT_STORE_WITHDRAWAL_HISTORY),
            store_mir_history: get_bool_flag(&config, DEFAULT_STORE_MIR_HISTORY),
            store_addresses: get_bool_flag(&config, DEFAULT_STORE_ADDRESSES),
            store_tx_count: get_bool_flag(&config, DEFAULT_STORE_TX_COUNT),
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
                        AccountsStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for accounts-state",
                        )),
                    )));
                };

                let response = match query {
                    AccountsStateQuery::GetAccountRegistrationHistory { account } => {
                        match state.lock().await.get_registration_history(account).await {
                            Ok(Some(registrations)) => {
                                AccountsStateQueryResponse::AccountRegistrationHistory(
                                    registrations,
                                )
                            }
                            Ok(None) => AccountsStateQueryResponse::Error(QueryError::not_found(
                                format!("Account {} not found", account),
                            )),
                            Err(e) => AccountsStateQueryResponse::Error(
                                QueryError::internal_error(e.to_string()),
                            ),
                        }
                    }
                    AccountsStateQuery::GetAccountDelegationHistory { account } => {
                        match state.lock().await.get_delegation_history(account).await {
                            Ok(Some(delegations)) => {
                                AccountsStateQueryResponse::AccountDelegationHistory(delegations)
                            }
                            Ok(None) => AccountsStateQueryResponse::Error(QueryError::not_found(
                                format!("Account {}", account),
                            )),
                            Err(e) => AccountsStateQueryResponse::Error(
                                QueryError::internal_error(e.to_string()),
                            ),
                        }
                    }
                    AccountsStateQuery::GetAccountMIRHistory { account } => {
                        match state.lock().await.get_mir_history(account).await {
                            Ok(Some(mirs)) => AccountsStateQueryResponse::AccountMIRHistory(mirs),
                            Ok(None) => AccountsStateQueryResponse::Error(QueryError::not_found(
                                format!("Account {}", account),
                            )),
                            Err(e) => AccountsStateQueryResponse::Error(
                                QueryError::internal_error(e.to_string()),
                            ),
                        }
                    }
                    AccountsStateQuery::GetAccountWithdrawalHistory { account } => {
                        match state.lock().await.get_withdrawal_history(account).await {
                            Ok(Some(withdrawals)) => {
                                AccountsStateQueryResponse::AccountWithdrawalHistory(withdrawals)
                            }
                            Ok(None) => AccountsStateQueryResponse::Error(QueryError::not_found(
                                format!("Account {}", account),
                            )),
                            Err(e) => AccountsStateQueryResponse::Error(
                                QueryError::internal_error(e.to_string()),
                            ),
                        }
                    }
                    AccountsStateQuery::GetAccountRewardHistory { account } => {
                        match state.lock().await.get_reward_history(account).await {
                            Ok(Some(rewards)) => {
                                AccountsStateQueryResponse::AccountRewardHistory(rewards)
                            }
                            Ok(None) => AccountsStateQueryResponse::Error(QueryError::not_found(
                                format!("Account {}", account),
                            )),
                            Err(e) => AccountsStateQueryResponse::Error(
                                QueryError::internal_error(e.to_string()),
                            ),
                        }
                    }
                    AccountsStateQuery::GetAccountAssociatedAddresses { account } => {
                        match state.lock().await.get_addresses(account).await {
                            Ok(Some(addresses)) => {
                                AccountsStateQueryResponse::AccountAssociatedAddresses(addresses)
                            }
                            Ok(None) => AccountsStateQueryResponse::Error(QueryError::not_found(
                                format!("Account {}", account),
                            )),
                            Err(e) => AccountsStateQueryResponse::Error(
                                QueryError::internal_error(e.to_string()),
                            ),
                        }
                    }
                    AccountsStateQuery::GetAccountTotalTxCount { account } => {
                        match state.lock().await.get_total_tx_count(account).await {
                            Ok(count) => AccountsStateQueryResponse::AccountTotalTxCount(count),
                            Err(e) => AccountsStateQueryResponse::Error(
                                QueryError::internal_error(e.to_string()),
                            ),
                        }
                    }
                    _ => AccountsStateQueryResponse::Error(QueryError::not_implemented(format!(
                        "Unimplemented query variant: {:?}",
                        query
                    ))),
                };

                Arc::new(Message::StateQueryResponse(StateQueryResponse::Accounts(
                    response,
                )))
            }
        });

        // Subscribe
        let rewards_reader = RewardsReader::new(&context, &config).await?;
        let certs_reader = CertsReader::new(&context, &config).await?;
        let withdrawals_reader = WithdrawalsReader::new(&context, &config).await?;
        let stake_deltas_reader = StakeDeltasReader::new(&context, &config).await?;
        let params_reader = ParamsReader::new(&context, &config).await?;

        // Start run task
        context.run(async move {
            Self::run(
                state_mutex,
                rewards_reader,
                certs_reader,
                withdrawals_reader,
                stake_deltas_reader,
                params_reader,
                is_snapshot_mode,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
