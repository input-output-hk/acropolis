//! Acropolis accounts state module for Caryatid
//! Manages stake and reward accounts state

use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper, ValidationContext},
    configuration::StartupMode,
    declare_cardano_reader,
    messages::{
        AccountsBootstrapMessage, CardanoMessage, EpochActivityMessage, GovernanceOutcomesMessage,
        GovernanceProceduresMessage, Message, PotDeltasMessage, ProtocolParamsMessage,
        SPOStateMessage, SnapshotMessage, SnapshotStateMessage, StakeAddressDeltasMessage,
        StateQuery, StateQueryResponse, StateTransitionMessage, TxCertificatesMessage,
        WithdrawalsMessage,
    },
    queries::{
        accounts::{
            AccountInfo, AccountsStateQuery, AccountsStateQueryResponse, DrepDelegators,
            PoolDelegators, DEFAULT_ACCOUNTS_QUERY_TOPIC,
        },
        errors::QueryError,
    },
    state_history::{StateHistory, StateHistoryStore},
    Era,
};
use anyhow::{bail, Result};
use caryatid_sdk::{message_bus::Subscription, module, Context};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod drep_distribution_publisher;
use drep_distribution_publisher::DRepDistributionPublisher;
mod spo_distribution_publisher;
use spo_distribution_publisher::SPODistributionPublisher;
mod spo_rewards_publisher;
use spo_rewards_publisher::SPORewardsPublisher;
mod registration_updates_publisher;
use registration_updates_publisher::StakeRegistrationUpdatesPublisher;
mod stake_reward_deltas_publisher;
mod state;
use stake_reward_deltas_publisher::StakeRewardDeltasPublisher;
use state::State;
mod monetary;
mod rewards;
mod runtime;
mod verifier;

use runtime::{AccountsRuntime, BlockStakeAddressUndoRecorder};
use verifier::Verifier;

use crate::spo_distribution_store::{SPDDStore, SPDDStoreConfig};
mod spo_distribution_store;

// Subscriptions
declare_cardano_reader!(
    SPOReader,
    "spo-state-subscribe-topic",
    "cardano.spo.state",
    SPOState,
    SPOStateMessage
);
declare_cardano_reader!(
    EpochActivityReader,
    "epoch-activity-subscribe-topic",
    "cardano.epoch.activity",
    EpochActivity,
    EpochActivityMessage
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
    PotsReader,
    "pots-subscribe-topic",
    "cardano.pot.deltas",
    PotDeltas,
    PotDeltasMessage
);
declare_cardano_reader!(
    StakeDeltasReader,
    "stake-deltas-subscribe-topic",
    "cardano.stake.deltas",
    StakeAddressDeltas,
    StakeAddressDeltasMessage
);
declare_cardano_reader!(
    ParamsReader,
    "protocol-parameters-subscribe-topic",
    "cardano.protocol.parameters",
    ProtocolParams,
    ProtocolParamsMessage
);
declare_cardano_reader!(
    GovProceduresReader,
    "governance-procedures-subscribe-topic",
    "cardano.governance",
    GovernanceProcedures,
    GovernanceProceduresMessage
);
declare_cardano_reader!(
    GovOutcomesReader,
    "governance-outcomes-subscribe-topic",
    "cardano.enact.state",
    GovernanceOutcomes,
    GovernanceOutcomesMessage
);

// Publishers
const DEFAULT_DREP_DISTRIBUTION_TOPIC: &str = "cardano.drep.distribution";
const DEFAULT_SPO_DISTRIBUTION_TOPIC: &str = "cardano.spo.distribution";
const DEFAULT_SPO_REWARDS_TOPIC: &str = "cardano.spo.rewards";
const DEFAULT_STAKE_REWARD_DELTAS_TOPIC: &str = "cardano.stake.reward.deltas";
const DEFAULT_STAKE_REGISTRATION_UPDATES_TOPIC: &str = "cardano.stake.registration.updates";
const DEFAULT_VALIDATION_OUTCOMES_TOPIC: &str = "cardano.validation.accounts";

/// Topic for receiving bootstrap data when starting from a CBOR dump snapshot
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

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
    fn handle_bootstrap(state: &mut State, accounts_data: AccountsBootstrapMessage) -> Result<()> {
        let epoch = accounts_data.epoch;
        let accounts_len = accounts_data.accounts.len();

        // Initialize accounts state from snapshot data
        state.bootstrap(accounts_data)?;

        info!(
            "Accounts state bootstrapped successfully for epoch {} with {} accounts",
            epoch, accounts_len
        );

        Ok(())
    }

    /// Wait for and process snapshot bootstrap messages
    async fn wait_for_bootstrap(
        history: Arc<Mutex<StateHistory<State>>>,
        mut snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
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

                    let block_number = accounts_data.block_number;
                    let mut state = State::default();

                    Self::handle_bootstrap(&mut state, accounts_data)?;
                    history.lock().await.bootstrap_init_with(state, block_number);
                    info!("Accounts state bootstrap complete");
                }
                Message::Snapshot(SnapshotMessage::Complete) => {
                    info!("Snapshot complete, exiting accounts state bootstrap loop");
                    return Ok(());
                }
                _ => {
                    // Ignore other messages (e.g., EpochState, SPOState bootstrap messages)
                }
            }
        }
    }

    /// Async run loop
    #[allow(clippy::too_many_arguments)]
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        spdd_store: Option<Arc<Mutex<SPDDStore>>>,
        context: Arc<Context<Message>>,
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        mut drep_publisher: DRepDistributionPublisher,
        mut spo_publisher: SPODistributionPublisher,
        mut spo_rewards_publisher: SPORewardsPublisher,
        mut stake_reward_deltas_publisher: StakeRewardDeltasPublisher,
        mut stake_registration_updates_publisher: StakeRegistrationUpdatesPublisher,
        validation_outcomes_topic: String,
        mut spos_reader: SPOReader,
        mut ea_reader: EpochActivityReader,
        mut certs_reader: CertsReader,
        mut withdrawals_reader: WithdrawalsReader,
        mut pot_deltas_reader: PotsReader,
        mut stake_deltas_reader: StakeDeltasReader,
        mut governance_procedures_reader: GovProceduresReader,
        mut governance_outcomes_reader: GovOutcomesReader,
        mut params_reader: ParamsReader,
        verifier: &Verifier,
        is_snapshot_mode: bool,
    ) -> Result<()> {
        // Wait for the snapshot bootstrap (if available)
        Self::wait_for_bootstrap(history.clone(), snapshot_subscription).await?;

        // Skip genesis-specific initialization when starting from snapshot
        // (pots are already loaded from snapshot bootstrap data)
        if !is_snapshot_mode {
            match params_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal(_) => {}
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial params");
                }
            }
            match governance_outcomes_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal(_) => {}
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial gov outcomes");
                }
            }

            // Initialisation messages
            {
                match pot_deltas_reader.read_with_rollbacks().await? {
                    RollbackWrapper::Normal((block_info, pot_deltas_msg)) => {
                        let mut state = history.lock().await.get_current_state();

                        state
                            .handle_pot_deltas(&pot_deltas_msg)
                            .inspect_err(|e| error!("Pots handling error: {e:#}"))
                            .ok();

                        history.lock().await.commit(block_info.number, state);
                    }
                    RollbackWrapper::Rollback(_) => {
                        bail!("Unexpected rollback while reading initial pots");
                    }
                }
            }
        }

        // Track if this is the first epoch after snapshot bootstrap
        // We skip rewards calculation on the first epoch since pot deltas were already applied
        let mut skip_first_epoch_rewards = is_snapshot_mode;
        let mut runtime = AccountsRuntime::default();

        // Main loop of synchronised messages
        loop {
            let mut ctx =
                ValidationContext::new(&context, &validation_outcomes_topic, "accounts_state");

            // Get a mutable state
            let mut state = history.lock().await.get_current_state();
            let mut stake_address_undo = BlockStakeAddressUndoRecorder::default();

            // Use certs_message as the synchroniser, but we have to handle it after the
            // epoch things, because they apply to the new epoch, not the last
            let primary = PrimaryRead::from_sync(
                &mut ctx,
                "certs_reader",
                certs_reader.read_with_rollbacks().await,
            )?;

            if primary.is_rollback() {
                state.rollback_stake_addresses(
                    &mut runtime.stake_address_undo_history,
                    primary.block_info().number,
                );
                state = history.lock().await.get_rolled_back_state(primary.block_info().number);
                runtime.rewards.rollback_to(primary.block_info());

                let rollback_message = primary
                    .rollback_message()
                    .cloned()
                    .expect("rollback primary read should include rollback message");
                drep_publisher.publish_message(rollback_message.clone()).await?;
                spo_publisher.publish_message(rollback_message.clone()).await?;
                spo_rewards_publisher.publish_message(rollback_message.clone()).await?;
                stake_reward_deltas_publisher.publish_message(rollback_message.clone()).await?;
                stake_registration_updates_publisher.publish_message(rollback_message).await?;
            } else {
                // Notify the state of the block (used to schedule reward calculations)
                state.notify_block(primary.block_info(), &mut runtime.rewards);
            }

            let epoch = primary.epoch();

            // Init drains the epoch-0 bootstrap messages, so the main loop only
            // synchronizes these side readers on rollbacks and real transitions.
            if primary.should_read_epoch_transition_messages() {
                match ctx
                    .consume_sync("params_reader", params_reader.read_with_rollbacks().await)?
                {
                    RollbackWrapper::Normal((block_info, params_msg)) => {
                        let span = info_span!(
                            "account_state.handle_parameters",
                            block = block_info.number
                        );
                        async {
                            ctx.handle("handle_parameters", state.handle_parameters(&params_msg));
                        }
                        .instrument(span)
                        .await;
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
                let mut stake_reward_deltas = if epoch.is_some() {
                    let block_info = primary.block_info();
                    // Applies rewards from previous epoch
                    match state
                        .complete_previous_epoch_rewards_calculation(
                            verifier,
                            skip_first_epoch_rewards,
                            &mut runtime.rewards,
                            &mut stake_address_undo,
                        )
                        .await
                    {
                        Ok((spo_rewards, stake_reward_deltas)) => {
                            ctx.handle(
                                "publish_spo_rewards",
                                spo_rewards_publisher
                                    .publish_spo_rewards(block_info, spo_rewards)
                                    .await,
                            );
                            stake_reward_deltas
                        }
                        Err(e) => {
                            ctx.handle_error("complete_previous_epoch_rewards_calculation", &e);
                            Vec::new()
                        }
                    }
                } else {
                    Vec::new()
                };

                // EPOCH rule
                // a. SNAP: Take the snapshot and pool distribution
                // rotate the snapshots (mark, set, go)
                // b. POOLREAP: for any retiring pools, refund,
                // remove from pool registry, clear delegations

                if primary.message().is_some() {
                    let block_info = primary.block_info();
                    // Apply pending MIRs before generating SPDD so they're included in active stake
                    state.apply_pending_mirs(&mut stake_address_undo);

                    // At the Conway hard fork, pointer addresses lose their staking
                    // functionality (Conway spec 9.1.2). Subtract accumulated pointer
                    // address UTxO values from utxo_value so they no longer count
                    // towards the stake distribution.
                    // Skip in snapshot mode: the snapshot already reflects post-Conway
                    // state, so applying the subtraction again would double-count.
                    if block_info.is_new_era && block_info.era == Era::Conway && !is_snapshot_mode {
                        ctx.handle(
                            "remove_pointer_address_stake",
                            state
                                .remove_pointer_address_stake(
                                    context.clone(),
                                    &mut stake_address_undo,
                                )
                                .await,
                        );
                    }

                    let spdd = state.generate_spdd();
                    verifier.verify_spdd(block_info, &spdd);
                    ctx.handle(
                        "publish_spdd",
                        spo_publisher.publish_spdd(block_info, spdd).await,
                    );

                    // store spdd history if enabled
                    let spdd_store_guard = match spdd_store.as_ref() {
                        Some(s) => Some(s.lock().await),
                        None => None,
                    };
                    if let Some(mut spdd_store) = spdd_store_guard {
                        let spdd_state = state.dump_spdd_state();
                        // stakes distribution taken at beginning of epoch i is active for epoch + 1
                        ctx.handle(
                            "store_spdd",
                            spdd_store
                                .store_spdd(block_info.epoch + 1, spdd_state)
                                .map_err(|e| e.into()),
                        );
                    }
                }

                // Handle SPOs
                match ctx.consume_sync("spos_reader", spos_reader.read_with_rollbacks().await)? {
                    RollbackWrapper::Normal((block_info, spo_msg)) => {
                        let span =
                            info_span!("account_state.handle_spo_state", block = block_info.number);
                        async {
                            ctx.handle("handle_spo_state", state.handle_spo_state(&spo_msg));
                        }
                        .instrument(span)
                        .await;
                    }
                    RollbackWrapper::Rollback(_) => {}
                }

                // Handle epoch activity
                let deltas_block_info =
                    match ctx.consume_sync("ea_reader", ea_reader.read_with_rollbacks().await)? {
                        RollbackWrapper::Normal((block_info, ea_msg)) => {
                            let span = info_span!(
                                "account_state.handle_epoch_activity",
                                block = block_info.number
                            );
                            async {
                                match state
                                    .handle_epoch_activity(
                                        context.clone(),
                                        &ea_msg,
                                        &block_info,
                                        verifier,
                                        &mut runtime.rewards,
                                        &mut stake_address_undo,
                                    )
                                    .await
                                {
                                    Ok(refund_deltas) => {
                                        stake_reward_deltas.extend(refund_deltas);
                                    }
                                    Err(e) => {
                                        ctx.handle_error("handle_epoch_activity", &e);
                                    }
                                }

                                let drdd = state.generate_drdd();
                                ctx.handle(
                                    "publish_drdd",
                                    drep_publisher.publish_drdd(&block_info, drdd).await,
                                );
                                Some(block_info)
                            }
                            .instrument(span)
                            .await
                        }
                        RollbackWrapper::Rollback(_) => None,
                    };

                // Handle governance outcomes (enacted/expired proposals) at epoch boundary
                match ctx.consume_sync(
                    "governance_outcomes_reader",
                    governance_outcomes_reader.read_with_rollbacks().await,
                )? {
                    RollbackWrapper::Normal((block_info, outcomes_msg)) => {
                        let span = info_span!(
                            "account_state.handle_governance_outcomes",
                            block = block_info.number
                        );
                        async {
                            let refund_deltas = ctx.handle(
                                "handle_governance_outcomes",
                                state.handle_governance_outcomes(
                                    &outcomes_msg,
                                    &mut stake_address_undo,
                                ),
                            );
                            stake_reward_deltas.extend(refund_deltas);
                        }
                        .instrument(span)
                        .await;
                    }
                    RollbackWrapper::Rollback(_) => {}
                }

                // publish stake reward deltas if we're not in rollback (that is, processing
                // some normal block with data and number).
                if let Some(block_info) = deltas_block_info {
                    async {
                        ctx.handle(
                            "publish_stake_reward_deltas",
                            stake_reward_deltas_publisher
                                .publish_stake_reward_deltas(&block_info, stake_reward_deltas)
                                .await,
                        )
                    }
                    .await;
                }

                // Clear the skip flag after first epoch transition
                skip_first_epoch_rewards = false;
            }

            // Now handle the certs_message properly
            if let Some(tx_certs_msg) = primary.message() {
                let block_info = primary.block_info();
                let span = info_span!("account_state.handle_certs", block = block_info.number);
                async {
                    match state.handle_tx_certificates(
                        tx_certs_msg,
                        block_info.epoch_slot,
                        block_info.era,
                        &mut ctx,
                        &mut stake_address_undo,
                    ) {
                        Ok(updates) => ctx.handle(
                            "stake_registration_updates_publisher.publish",
                            stake_registration_updates_publisher.publish(block_info, updates).await,
                        ),
                        Err(e) => {
                            ctx.handle_error("handle_tx_certificates", &e);
                        }
                    }
                }
                .instrument(span)
                .await;
            }

            // Handle withdrawals
            match ctx.consume_sync(
                "withdrawals_reader",
                withdrawals_reader.read_with_rollbacks().await,
            )? {
                RollbackWrapper::Normal((block_info, withdrawals_msg)) => {
                    let span = info_span!(
                        "account_state.handle_withdrawals",
                        block = block_info.number
                    );
                    async {
                        state.handle_withdrawals(
                            &withdrawals_msg,
                            &mut ctx,
                            &mut stake_address_undo,
                        );
                    }
                    .instrument(span)
                    .await;
                }
                RollbackWrapper::Rollback(_) => {}
            }

            // Handle stake address deltas
            match ctx.consume_sync(
                "stake_deltas_reader",
                stake_deltas_reader.read_with_rollbacks().await,
            )? {
                RollbackWrapper::Normal((block_info, deltas_msg)) => {
                    let span = info_span!(
                        "account_state.handle_stake_deltas",
                        block = block_info.number
                    );
                    async {
                        state.handle_stake_deltas(&deltas_msg, &mut ctx, &mut stake_address_undo);
                    }
                    .instrument(span)
                    .await;
                }
                RollbackWrapper::Rollback(_) => {}
            }

            match ctx.consume_sync(
                "governance_procedures_reader",
                governance_procedures_reader.read_with_rollbacks().await,
            )? {
                RollbackWrapper::Normal((block_info, procedures)) => {
                    let span = info_span!(
                        "account_state.handle_governance_procedures",
                        block = block_info.number
                    );
                    async { state.handle_governance_procedures(&procedures) }
                        .instrument(span)
                        .await;
                }
                RollbackWrapper::Rollback(_) => {}
            }

            // Commit the new state
            if primary.message().is_some() {
                let block_info = primary.block_info();
                runtime.stake_address_undo_history.commit(block_info.number, stake_address_undo);
                history.lock().await.commit(block_info.number, state);
                if primary.do_validation() {
                    ctx.publish().await;
                }
            } else {
                ctx.get_validation().print_errors("accounts_state", None);
            }
        }
    }

    /// Async initialisation
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration

        // Subscription topics
        let snapshot_subscribe_topic = config
            .get_string(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.1.to_string());

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

        let stake_registration_updates_topic = config
            .get_string("publish-stake-registration-updates-topic")
            .unwrap_or(DEFAULT_STAKE_REGISTRATION_UPDATES_TOPIC.to_string());
        info!(
            "Creating stake registration updates publisher on '{stake_registration_updates_topic}'"
        );

        let validation_outcomes_topic = config
            .get_string("validation-outcomes-topic")
            .unwrap_or(DEFAULT_VALIDATION_OUTCOMES_TOPIC.to_string());
        info!("Validation outcomes are to be published on '{validation_outcomes_topic}'");

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
            verifier.set_spdd_template(&verify_spdd_files)?;
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
        let stake_registration_updates_publisher = StakeRegistrationUpdatesPublisher::new(
            context.clone(),
            stake_registration_updates_topic,
        );

        // Subscribe
        let spos_reader = SPOReader::new(&context, &config).await?;
        let ea_reader = EpochActivityReader::new(&context, &config).await?;
        let certs_reader = CertsReader::new(&context, &config).await?;
        let withdrawals_reader = WithdrawalsReader::new(&context, &config).await?;
        let pot_deltas_reader = PotsReader::new(&context, &config).await?;
        let stake_deltas_reader = StakeDeltasReader::new(&context, &config).await?;
        let governance_procedures_reader = GovProceduresReader::new(&context, &config).await?;
        let governance_outcomes_reader = GovOutcomesReader::new(&context, &config).await?;
        let params_reader = ParamsReader::new(&context, &config).await?;

        // Only subscribe to Snapshot if we're using Snapshot to start-up
        let is_snapshot_mode = StartupMode::from_config(config.as_ref()).is_snapshot();
        let snapshot_subscription = if is_snapshot_mode {
            info!("Creating subscriber for snapshot on '{snapshot_subscribe_topic}'");
            Some(context.subscribe(&snapshot_subscribe_topic).await?)
        } else {
            info!("Skipping snapshot subscription (startup method is not snapshot)");
            None
        };

        let context_copy = context.clone();
        // Start run task
        context.run(async move {
            Self::run(
                history,
                spdd_store,
                context_copy,
                snapshot_subscription,
                drep_publisher,
                spo_publisher,
                spo_rewards_publisher,
                stake_reward_deltas_publisher,
                stake_registration_updates_publisher,
                validation_outcomes_topic,
                spos_reader,
                ea_reader,
                certs_reader,
                withdrawals_reader,
                pot_deltas_reader,
                stake_deltas_reader,
                governance_procedures_reader,
                governance_outcomes_reader,
                params_reader,
                &verifier,
                is_snapshot_mode,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
