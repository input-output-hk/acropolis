//! Acropolis accounts state module for Caryatid
//! Manages stake and reward accounts state

use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper, ValidationContext},
    declare_cardano_reader,
    messages::{
        AccountsBootstrapMessage, CardanoMessage, EpochActivityMessage, GovernanceOutcomesMessage,
        GovernanceProceduresMessage, Message, PotDeltasMessage, ProtocolParamsMessage,
        SPOStateMessage, SnapshotMessage, SnapshotStateMessage, StakeAddressDeltasMessage,
        StateQueryResponse, StateTransitionMessage, TxCertificatesMessage, WithdrawalsMessage,
    },
    queries::{accounts::AccountsStateQueryResponse, errors::QueryError},
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
mod configuration;
mod monetary;
mod queries;
mod rewards;
mod runtime;
mod verifier;

use runtime::{AccountsRuntime, BlockStakeAddressUndoRecorder};
use verifier::Verifier;

use crate::{
    configuration::AccountsConfig, queries::handle_accounts_query,
    spo_distribution_store::SPDDStore,
};
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
                                    // publish stake reward deltas
                                    stake_reward_deltas.extend(refund_deltas);
                                    ctx.handle(
                                        "publish_stake_reward_deltas",
                                        stake_reward_deltas_publisher
                                            .publish_stake_reward_deltas(
                                                &block_info,
                                                stake_reward_deltas,
                                            )
                                            .await,
                                    );
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
                        }
                        .instrument(span)
                        .await;
                    }
                    RollbackWrapper::Rollback(_) => {}
                }

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
                            ctx.handle(
                                "handle_governance_outcomes",
                                state.handle_governance_outcomes(
                                    &outcomes_msg,
                                    &mut stake_address_undo,
                                ),
                            );
                        }
                        .instrument(span)
                        .await;
                    }
                    RollbackWrapper::Rollback(_) => {}
                }

                // Clear the skip flag after first transition handling.
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
        let accounts_cfg = AccountsConfig::init(context.clone(), &config).await?;

        // History
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "accounts_state",
            StateHistoryStore::default_block_store(),
        )));
        let history_query = history.clone();
        let history_tick = history.clone();

        let spdd_store_query = accounts_cfg.spdd_store.clone();

        context.handle(&accounts_cfg.accounts_query_topic, move |message| {
            let history = history_query.clone();
            let spdd_store = spdd_store_query.clone();
            async move {
                let guard = history.lock().await;

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

                let spdd_store_guard = match spdd_store.as_ref() {
                    Some(s) => Some(s.lock().await),
                    None => None,
                };

                handle_accounts_query(state, spdd_store_guard.as_deref(), message.as_ref())
            }
        });

        // Ticker to log stats
        let mut tick_subscription = context.subscribe("clock.tick").await?;
        context.clone().run(async move {
            loop {
                match tick_subscription.read().await {
                    Ok((_, message)) => match message.as_ref() {
                        Message::Clock(message) if message.number % 60 == 0 => {
                            if let Some(state) = history_tick.lock().await.current() {
                                state.log_stats();
                            }
                        }
                        _ => continue,
                    },
                    Err(_) => return,
                }
            }
        });

        let context_copy = context.clone();
        // Start run task
        context.run(async move {
            Self::run(
                history,
                accounts_cfg.spdd_store,
                context_copy,
                accounts_cfg.snapshot_subscription,
                accounts_cfg.drep_publisher,
                accounts_cfg.spo_publisher,
                accounts_cfg.spo_rewards_publisher,
                accounts_cfg.stake_reward_deltas_publisher,
                accounts_cfg.stake_registration_updates_publisher,
                accounts_cfg.validation_outcomes_topic,
                accounts_cfg.spos_reader,
                accounts_cfg.ea_reader,
                accounts_cfg.certs_reader,
                accounts_cfg.withdrawals_reader,
                accounts_cfg.pot_deltas_reader,
                accounts_cfg.stake_deltas_reader,
                accounts_cfg.governance_procedures_reader,
                accounts_cfg.governance_outcomes_reader,
                accounts_cfg.params_reader,
                &accounts_cfg.verifier,
                accounts_cfg.is_snapshot_mode,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
