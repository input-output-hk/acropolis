//! Acropolis accounts state module for Caryatid
//! Manages stake and reward accounts state

use acropolis_common::{
    caryatid::SubscriptionExt,
    configuration::StartupMethod,
    messages::{
        AccountsBootstrapMessage, CardanoMessage, Message, SnapshotMessage, SnapshotStateMessage,
        StateQuery, StateQueryResponse, StateTransitionMessage,
    },
    queries::accounts::{DrepDelegators, PoolDelegators, DEFAULT_ACCOUNTS_QUERY_TOPIC},
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{message_bus::Subscription, module, Context};
use config::Config;
use std::sync::Arc;
use tokio::{join, sync::Mutex};
use tracing::{error, info, info_span, Instrument};

mod drep_distribution_publisher;
use drep_distribution_publisher::DRepDistributionPublisher;
mod spo_distribution_publisher;
use spo_distribution_publisher::SPODistributionPublisher;
mod spo_rewards_publisher;
use spo_rewards_publisher::SPORewardsPublisher;
mod stake_reward_deltas_publisher;
mod state;
use stake_reward_deltas_publisher::StakeRewardDeltasPublisher;
use state::State;
mod monetary;
mod rewards;
mod verifier;
use acropolis_common::queries::accounts::{
    AccountInfo, AccountsStateQuery, AccountsStateQueryResponse,
};
use acropolis_common::queries::errors::QueryError;
use verifier::Verifier;

use crate::spo_distribution_store::{SPDDStore, SPDDStoreConfig};
mod spo_distribution_store;

const DEFAULT_SPO_STATE_TOPIC: &str = "cardano.spo.state";
const DEFAULT_EPOCH_ACTIVITY_TOPIC: &str = "cardano.epoch.activity";
const DEFAULT_TX_CERTIFICATES_TOPIC: &str = "cardano.certificates";
const DEFAULT_WITHDRAWALS_TOPIC: &str = "cardano.withdrawals";
const DEFAULT_POT_DELTAS_TOPIC: &str = "cardano.pot.deltas";
const DEFAULT_STAKE_DELTAS_TOPIC: &str = "cardano.stake.deltas";
const DEFAULT_DREP_STATE_TOPIC: &str = "cardano.drep.state";
const DEFAULT_DREP_DISTRIBUTION_TOPIC: &str = "cardano.drep.distribution";
const DEFAULT_SPO_DISTRIBUTION_TOPIC: &str = "cardano.spo.distribution";
const DEFAULT_SPO_REWARDS_TOPIC: &str = "cardano.spo.rewards";
const DEFAULT_PROTOCOL_PARAMETERS_TOPIC: &str = "cardano.protocol.parameters";
const DEFAULT_STAKE_REWARD_DELTAS_TOPIC: &str = "cardano.stake.reward.deltas";

/// Topic for receiving bootstrap data when starting from a CBOR dump snapshot
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");
/// Topic signaling that the snapshot bootstrap is complete
const DEFAULT_SNAPSHOT_COMPLETION_TOPIC: (&str, &str) =
    ("snapshot-completion-topic", "cardano.snapshot.complete");

const DEFAULT_SPDD_DB_PATH: (&str, &str) = ("spdd-db-path", "./fjall-spdd");
const DEFAULT_SPDD_RETENTION_EPOCHS: (&str, u64) = ("spdd-retention-epochs", 0);
const DEFAULT_SPDD_CLEAR_ON_START: (&str, bool) = ("spdd-clear-on-start", true);

/// Accounts State module
#[module(
    message_type(Message),
    name = "accounts-state",
    description = "Stake and reward accounts state"
)]
pub struct AccountsState;

impl AccountsState {
    /// Handle bootstrap message from snapshot
    fn handle_bootstrap(state: &mut State, accounts_data: AccountsBootstrapMessage) {
        let epoch = accounts_data.epoch;
        let accounts_len = accounts_data.accounts.len();

        // Initialize accounts state from snapshot data
        state.bootstrap(accounts_data);

        info!(
            "Accounts state bootstrapped successfully for epoch {} with {} accounts",
            epoch, accounts_len
        );
    }

    /// Wait for and process snapshot bootstrap messages
    /// Returns the BlockInfo from SnapshotComplete if bootstrap occurred, None otherwise
    async fn wait_for_bootstrap(
        history: Arc<Mutex<StateHistory<State>>>,
        mut snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        mut completion_subscription: Option<Box<dyn Subscription<Message>>>,
    ) -> Result<()> {
        let snapshot_subscription = match snapshot_subscription.as_mut() {
            Some(sub) => sub,
            None => {
                info!("No snapshot subscription available, using default state");
                return Ok(());
            }
        };

        info!("Waiting for snapshot bootstrap messages...");
        loop {
            let (_, message) = snapshot_subscription.read().await?;
            let message = Arc::try_unwrap(message).unwrap_or_else(|arc| (*arc).clone());

            match message {
                Message::Snapshot(SnapshotMessage::Startup) => {
                    info!("Received snapshot startup signal, awaiting bootstrap data...");
                }
                Message::Snapshot(SnapshotMessage::Bootstrap(
                    SnapshotStateMessage::AccountsState(accounts_data),
                )) => {
                    info!("Received AccountsState bootstrap message");
                    let epoch = accounts_data.epoch;
                    let mut state = history.lock().await.get_or_init_with(State::default);
                    Self::handle_bootstrap(&mut state, accounts_data);
                    history.lock().await.commit(epoch, state);
                    info!("Accounts state bootstrap complete");
                    break;
                }
                _ => {
                    // Ignore other messages (e.g., EpochState, SPOState bootstrap messages)
                }
            }
        }

        let completion_subscription = match completion_subscription.as_mut() {
            Some(sub) => sub,
            None => {
                info!("No completion subscription available");
                return Ok(());
            }
        };

        info!("Waiting for snapshot complete message...");
        let (_, message) = completion_subscription.read().await?;
        match message.as_ref() {
            Message::Cardano((_, CardanoMessage::SnapshotComplete)) => {
                info!("Received snapshot complete message");
                Ok(())
            }
            other => {
                error!(
                    "Unexpected message on completion topic: {:?}",
                    std::any::type_name_of_val(other)
                );
                Err(anyhow::anyhow!(format!(
                    "Unexpected message on completion topic: {:?}",
                    other
                )))
            }
        }
    }

    /// Async run loop
    #[allow(clippy::too_many_arguments)]
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        spdd_store: Option<Arc<Mutex<SPDDStore>>>,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        completion_subscription: Option<Box<dyn Subscription<Message>>>,
        mut drep_publisher: DRepDistributionPublisher,
        mut spo_publisher: SPODistributionPublisher,
        mut spo_rewards_publisher: SPORewardsPublisher,
        mut stake_reward_deltas_publisher: StakeRewardDeltasPublisher,
        mut spos_subscription: Box<dyn Subscription<Message>>,
        mut ea_subscription: Box<dyn Subscription<Message>>,
        mut certs_subscription: Box<dyn Subscription<Message>>,
        mut withdrawals_subscription: Box<dyn Subscription<Message>>,
        mut pots_subscription: Box<dyn Subscription<Message>>,
        mut stake_subscription: Box<dyn Subscription<Message>>,
        mut drep_state_subscription: Box<dyn Subscription<Message>>,
        mut parameters_subscription: Box<dyn Subscription<Message>>,
        verifier: &Verifier,
    ) -> Result<()> {
        // Wait for the snapshot bootstrap (if available)
        Self::wait_for_bootstrap(
            history.clone(),
            snapshot_subscription,
            completion_subscription,
        )
        .await?;

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
                history.lock().await.commit(block_info.number, state);
            }
        }

        // Main loop of synchronised messages
        loop {
            // Get a mutable state
            let mut state = history.lock().await.get_current_state();

            let mut current_block: Option<BlockInfo> = None;

            // Use certs_message as the synchroniser, but we have to handle it after the
            // epoch things, because they apply to the new epoch, not the last
            let (_, certs_message) = certs_subscription.read().await?;
            let new_epoch = match certs_message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::TxCertificates(_))) => {
                    // Handle rollbacks on this topic only
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(block_info.number);
                    }

                    current_block = Some(block_info.clone());
                    block_info.new_epoch && block_info.epoch > 0
                }
                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                )) => {
                    drep_publisher.publish_rollback(certs_message.clone()).await?;
                    spo_publisher.publish_rollback(certs_message.clone()).await?;
                    spo_rewards_publisher.publish_rollback(certs_message.clone()).await?;
                    stake_reward_deltas_publisher.publish_rollback(certs_message.clone()).await?;
                    false
                }
                _ => false,
            };

            // Notify the state of the block (used to schedule reward calculations)
            if let Some(block_info) = &current_block {
                state.notify_block(block_info);
            }

            // Read from epoch-boundary messages only when it's a new epoch
            if new_epoch {
                let spdd_store_guard = match spdd_store.as_ref() {
                    Some(s) => Some(s.lock().await),
                    None => None,
                };

                // Publish SPDD message before anything else and store spdd history if enabled
                if let Some(block_info) = current_block.as_ref() {
                    let spdd = state.generate_spdd();
                    verifier.verify_spdd(block_info, &spdd);
                    if let Err(e) = spo_publisher.publish_spdd(block_info, spdd).await {
                        error!("Error publishing SPO stake distribution: {e:#}")
                    }

                    // if we store spdd history
                    let spdd_state = state.dump_spdd_state();
                    if let Some(mut spdd_store) = spdd_store_guard {
                        // active stakes taken at beginning of epoch i is for epoch + 1
                        if let Err(e) = spdd_store.store_spdd(block_info.epoch + 1, spdd_state) {
                            error!("Error storing SPDD state: {e:#}")
                        }
                    }
                }

                // Handle DRep
                let (_, message) = drep_state_subscription.read_ignoring_rollbacks().await?;
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::DRepState(dreps_msg))) => {
                        let span = info_span!(
                            "account_state.handle_drep_state",
                            block = block_info.number
                        );
                        async {
                            Self::check_sync(&current_block, block_info);
                            state.handle_drep_state(dreps_msg);

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
                let (_, message) = spos_subscription.read_ignoring_rollbacks().await?;
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::SPOState(spo_msg))) => {
                        let span =
                            info_span!("account_state.handle_spo_state", block = block_info.number);
                        async {
                            Self::check_sync(&current_block, block_info);
                            state
                                .handle_spo_state(spo_msg)
                                .inspect_err(|e| error!("SPOState handling error: {e:#}"))
                                .ok();
                        }
                        .instrument(span)
                        .await;
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }

                let (_, message) = parameters_subscription.read_ignoring_rollbacks().await?;
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::ProtocolParams(params_msg))) => {
                        let span = info_span!(
                            "account_state.handle_parameters",
                            block = block_info.number
                        );
                        async {
                            Self::check_sync(&current_block, block_info);
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

                // Handle epoch activity
                let (_, message) = ea_subscription.read_ignoring_rollbacks().await?;
                match message.as_ref() {
                    Message::Cardano((block_info, CardanoMessage::EpochActivity(ea_msg))) => {
                        let span = info_span!(
                            "account_state.handle_epoch_activity",
                            block = block_info.number
                        );
                        async {
                            Self::check_sync(&current_block, block_info);
                            let after_epoch_result = state
                                .handle_epoch_activity(ea_msg, verifier)
                                .await
                                .inspect_err(|e| error!("EpochActivity handling error: {e:#}"))
                                .ok();
                            if let Some((spo_rewards, stake_reward_deltas)) = after_epoch_result {
                                // SPO Rewards Future
                                let spo_rewards_future = spo_rewards_publisher
                                    .publish_spo_rewards(block_info, spo_rewards);
                                // Stake Reward Deltas Future
                                let stake_reward_deltas_future = stake_reward_deltas_publisher
                                    .publish_stake_reward_deltas(block_info, stake_reward_deltas);

                                // publish in parallel
                                let (spo_rewards_result, stake_reward_deltas_result) =
                                    join!(spo_rewards_future, stake_reward_deltas_future);
                                spo_rewards_result.unwrap_or_else(|e| {
                                    error!("Error publishing SPO rewards: {e:#}")
                                });
                                stake_reward_deltas_result.unwrap_or_else(|e| {
                                    error!("Error publishing stake reward deltas: {e:#}")
                                });
                            }
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
                        Self::check_sync(&current_block, block_info);
                        state
                            .handle_tx_certificates(tx_certs_msg)
                            .inspect_err(|e| error!("TxCertificates handling error: {e:#}"))
                            .ok();
                    }
                    .instrument(span)
                    .await;
                }

                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                )) => {
                    // Ignore this, we already handled rollbacks
                }

                _ => error!("Unexpected message type: {certs_message:?}"),
            }

            // Handle withdrawals
            let (_, message) = withdrawals_subscription.read_ignoring_rollbacks().await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::Withdrawals(withdrawals_msg))) => {
                    let span = info_span!(
                        "account_state.handle_withdrawals",
                        block = block_info.number
                    );
                    async {
                        Self::check_sync(&current_block, block_info);
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
            let (_, message) = stake_subscription.read_ignoring_rollbacks().await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::StakeAddressDeltas(deltas_msg))) => {
                    let span = info_span!(
                        "account_state.handle_stake_deltas",
                        block = block_info.number
                    );
                    async {
                        Self::check_sync(&current_block, block_info);
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
                history.lock().await.commit(block_info.number, state);
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
        info!("Creating protocol parameters subscriber on '{parameters_topic}'");

        let snapshot_subscribe_topic = config
            .get_string(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.1.to_string());

        let snapshot_completion_topic = config
            .get_string(DEFAULT_SNAPSHOT_COMPLETION_TOPIC.0)
            .unwrap_or(DEFAULT_SNAPSHOT_COMPLETION_TOPIC.1.to_string());

        // Publishing topics
        let drep_distribution_topic = config
            .get_string("publish-drep-distribution-topic")
            .unwrap_or(DEFAULT_DREP_DISTRIBUTION_TOPIC.to_string());
        info!("Creating DRep distribution publisher on '{drep_distribution_topic}'");

        let spo_distribution_topic = config
            .get_string("publish-spo-distribution-topic")
            .unwrap_or(DEFAULT_SPO_DISTRIBUTION_TOPIC.to_string());
        info!("Creating SPO distribution publisher on '{spo_distribution_topic}'");

        let spo_rewards_topic = config
            .get_string("publish-spo-rewards-topic")
            .unwrap_or(DEFAULT_SPO_REWARDS_TOPIC.to_string());
        info!("Creating SPO rewards publisher on '{spo_rewards_topic}'");

        let stake_reward_deltas_topic = config
            .get_string("publish-stake-reward-deltas-topic")
            .unwrap_or(DEFAULT_STAKE_REWARD_DELTAS_TOPIC.to_string());
        info!("Creating stake reward deltas publisher on '{stake_reward_deltas_topic}'");

        // SPDD configs
        let spdd_db_path =
            config.get_string(DEFAULT_SPDD_DB_PATH.0).unwrap_or(DEFAULT_SPDD_DB_PATH.1.to_string());
        info!("SPDD database path: {spdd_db_path}");
        let spdd_retention_epochs = config
            .get_int(DEFAULT_SPDD_RETENTION_EPOCHS.0)
            .unwrap_or(DEFAULT_SPDD_RETENTION_EPOCHS.1 as i64)
            .max(0) as u64;
        info!("SPDD retention epochs: {:?}", spdd_retention_epochs);
        let spdd_clear_on_start =
            config.get_bool(DEFAULT_SPDD_CLEAR_ON_START.0).unwrap_or(DEFAULT_SPDD_CLEAR_ON_START.1);
        info!("SPDD clear on start: {spdd_clear_on_start}");

        // Query topics
        let accounts_query_topic = config
            .get_string(DEFAULT_ACCOUNTS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_ACCOUNTS_QUERY_TOPIC.1.to_string());
        info!("Creating query handler on '{}'", accounts_query_topic);

        // Create verifier and read comparison data according to config
        let mut verifier = Verifier::new();

        if let Ok(verify_pots_file) = config.get_string("verify-pots-file") {
            info!("Verifying pots against '{verify_pots_file}'");
            verifier.read_pots(&verify_pots_file);
        }

        if let Ok(verify_rewards_files) = config.get_string("verify-rewards-files") {
            info!("Verifying rewards against '{verify_rewards_files}'");
            verifier.set_rewards_template(&verify_rewards_files);
        }

        if let Ok(verify_spdd_files) = config.get_string("verify-spdd-files") {
            info!("Verifying rewards against '{verify_spdd_files}'");
            verifier.set_spdd_template(&verify_spdd_files);
        }

        // History
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "AccountsState",
            StateHistoryStore::default_block_store(),
        )));
        let history_query = history.clone();
        let history_tick = history.clone();

        // Spdd store
        let spdd_store_config = SPDDStoreConfig {
            path: spdd_db_path,
            retention_epochs: spdd_retention_epochs,
            clear_on_start: spdd_clear_on_start,
        };
        let spdd_store = if spdd_store_config.is_enabled() {
            Some(Arc::new(Mutex::new(SPDDStore::new(&spdd_store_config)?)))
        } else {
            None
        };
        let spdd_store_query = spdd_store.clone();

        context.handle(&accounts_query_topic, move |message| {
            let history = history_query.clone();
            let spdd_store = spdd_store_query.clone();
            async move {
                let Message::StateQuery(StateQuery::Accounts(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Accounts(
                        AccountsStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for accounts-state",
                        )),
                    )));
                };

                let guard = history.lock().await;
                let spdd_store_guard = match spdd_store.as_ref() {
                    Some(s) => Some(s.lock().await),
                    None => None,
                };

                let state = match guard.current() {
                    Some(s) => s,
                    None => {
                        return Arc::new(Message::StateQueryResponse(
                            StateQueryResponse::Accounts(AccountsStateQueryResponse::Error(
                                QueryError::not_found("Current state"),
                            )),
                        ));
                    }
                };

                let response = match query {
                    AccountsStateQuery::GetAccountInfo { account } => {
                        match state.get_stake_state(account) {
                            Some(account) => AccountsStateQueryResponse::AccountInfo(AccountInfo {
                                utxo_value: account.utxo_value,
                                rewards: account.rewards,
                                delegated_spo: account.delegated_spo,
                                delegated_drep: account.delegated_drep.clone(),
                            }),
                            None => AccountsStateQueryResponse::Error(QueryError::not_found(
                                format!("Account {}", account),
                            )),
                        }
                    }

                    AccountsStateQuery::GetPoolsLiveStakes { pools_operators } => {
                        AccountsStateQueryResponse::PoolsLiveStakes(
                            state.get_pools_live_stakes(pools_operators),
                        )
                    }

                    AccountsStateQuery::GetPoolDelegators { pool_operator } => {
                        AccountsStateQueryResponse::PoolDelegators(PoolDelegators {
                            delegators: state.get_pool_delegators(pool_operator),
                        })
                    }

                    AccountsStateQuery::GetPoolLiveStake { pool_operator } => {
                        AccountsStateQueryResponse::PoolLiveStake(
                            state.get_pool_live_stake_info(pool_operator),
                        )
                    }

                    AccountsStateQuery::GetDrepDelegators { drep } => {
                        AccountsStateQueryResponse::DrepDelegators(DrepDelegators {
                            delegators: state.get_drep_delegators(drep),
                        })
                    }

                    AccountsStateQuery::GetAccountsDrepDelegationsMap { stake_addresses } => {
                        match state.get_drep_delegations_map(stake_addresses) {
                            Some(map) => {
                                AccountsStateQueryResponse::AccountsDrepDelegationsMap(map)
                            }
                            None => AccountsStateQueryResponse::Error(QueryError::internal_error(
                                "Error retrieving DRep delegations map",
                            )),
                        }
                    }

                    AccountsStateQuery::GetOptimalPoolSizing => {
                        AccountsStateQueryResponse::OptimalPoolSizing(
                            state.get_optimal_pool_sizing(),
                        )
                    }

                    AccountsStateQuery::GetAccountsUtxoValuesMap { stake_addresses } => {
                        match state.get_accounts_utxo_values_map(stake_addresses) {
                            Some(map) => AccountsStateQueryResponse::AccountsUtxoValuesMap(map),
                            None => AccountsStateQueryResponse::Error(QueryError::not_found(
                                "One or more accounts not found",
                            )),
                        }
                    }

                    AccountsStateQuery::GetAccountsUtxoValuesSum { stake_addresses } => {
                        match state.get_accounts_utxo_values_sum(stake_addresses) {
                            Some(sum) => AccountsStateQueryResponse::AccountsUtxoValuesSum(sum),
                            None => AccountsStateQueryResponse::Error(QueryError::not_found(
                                "One or more accounts not found",
                            )),
                        }
                    }

                    AccountsStateQuery::GetAccountsBalancesMap { stake_addresses } => {
                        match state.get_accounts_balances_map(stake_addresses) {
                            Some(map) => AccountsStateQueryResponse::AccountsBalancesMap(map),
                            None => AccountsStateQueryResponse::Error(QueryError::not_found(
                                "One or more accounts not found",
                            )),
                        }
                    }

                    AccountsStateQuery::GetActiveStakes {} => {
                        AccountsStateQueryResponse::ActiveStakes(
                            state.get_latest_snapshot_account_balances(),
                        )
                    }

                    AccountsStateQuery::GetAccountsBalancesSum { stake_addresses } => {
                        match state.get_account_balances_sum(stake_addresses) {
                            Some(sum) => AccountsStateQueryResponse::AccountsBalancesSum(sum),
                            None => AccountsStateQueryResponse::Error(QueryError::not_found(
                                "One or more accounts not found",
                            )),
                        }
                    }

                    AccountsStateQuery::GetSPDDByEpoch { epoch } => match spdd_store_guard {
                        Some(spdd_store) => match spdd_store.query_by_epoch(*epoch) {
                            Ok(result) => AccountsStateQueryResponse::SPDDByEpoch(result),
                            Err(e) => AccountsStateQueryResponse::Error(
                                QueryError::internal_error(e.to_string()),
                            ),
                        },
                        None => {
                            AccountsStateQueryResponse::Error(QueryError::storage_disabled("SPDD"))
                        }
                    },

                    AccountsStateQuery::GetSPDDByEpochAndPool { epoch, pool_id } => {
                        match spdd_store_guard {
                            Some(spdd_store) => {
                                match spdd_store.query_by_epoch_and_pool(*epoch, pool_id) {
                                    Ok(result) => {
                                        AccountsStateQueryResponse::SPDDByEpochAndPool(result)
                                    }
                                    Err(e) => AccountsStateQueryResponse::Error(
                                        QueryError::internal_error(e.to_string()),
                                    ),
                                }
                            }
                            None => AccountsStateQueryResponse::Error(
                                QueryError::storage_disabled("SPDD"),
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
        let spo_rewards_publisher = SPORewardsPublisher::new(context.clone(), spo_rewards_topic);
        let stake_reward_deltas_publisher =
            StakeRewardDeltasPublisher::new(context.clone(), stake_reward_deltas_topic);

        // Subscribe
        let spos_subscription = context.subscribe(&spo_state_topic).await?;
        let ea_subscription = context.subscribe(&epoch_activity_topic).await?;
        let certs_subscription = context.subscribe(&tx_certificates_topic).await?;
        let withdrawals_subscription = context.subscribe(&withdrawals_topic).await?;
        let pot_deltas_subscription = context.subscribe(&pot_deltas_topic).await?;
        let stake_subscription = context.subscribe(&stake_deltas_topic).await?;
        let drep_state_subscription = context.subscribe(&drep_state_topic).await?;
        let parameters_subscription = context.subscribe(&parameters_topic).await?;

        // Only subscribe to Snapshot if we're using Snapshot to start-up
        let (snapshot_subscription, completion_subscription) =
            if StartupMethod::from_config(config.as_ref()).is_snapshot() {
                info!("Creating subscriber for snapshot on '{snapshot_subscribe_topic}'");
                info!(
                    "Creating subscriber for snapshot completion on '{snapshot_completion_topic}'"
                );
                (
                    Some(context.subscribe(&snapshot_subscribe_topic).await?),
                    Some(context.subscribe(&snapshot_completion_topic).await?),
                )
            } else {
                info!("Skipping snapshot subscription (startup method is not snapshot)");
                (None, None)
            };

        // Start run task
        context.run(async move {
            Self::run(
                history,
                spdd_store,
                snapshot_subscription,
                completion_subscription,
                drep_publisher,
                spo_publisher,
                spo_rewards_publisher,
                stake_reward_deltas_publisher,
                spos_subscription,
                ea_subscription,
                certs_subscription,
                withdrawals_subscription,
                pot_deltas_subscription,
                stake_subscription,
                drep_state_subscription,
                parameters_subscription,
                &verifier,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
