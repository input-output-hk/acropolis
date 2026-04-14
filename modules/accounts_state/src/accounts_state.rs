//! Acropolis accounts state module for Caryatid
//! Manages stake and reward accounts state

use acropolis_common::{
    caryatid::{PrimaryRead, RollbackWrapper, ValidationContext},
    declare_cardano_reader,
    messages::{
        CardanoMessage, EpochActivityMessage, GenesisCompleteMessage, GovernanceOutcomesMessage,
        GovernanceProceduresMessage, Message, ProtocolParamsMessage, SPOStateMessage,
        SnapshotMessage, SnapshotStateMessage, StakeAddressDeltasMessage, StateQueryResponse,
        StateTransitionMessage, TxCertificatesMessage, WithdrawalsMessage,
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
mod pots_publisher;
mod queries;
mod rewards;
mod runtime;
mod verifier;

use runtime::{AccountsRuntime, BlockStakeAddressUndoRecorder};
use verifier::Verifier;

use crate::{
    configuration::AccountsConfig, pots_publisher::PotsPublisher, queries::handle_accounts_query,
};

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
    GenesisReader,
    "genesis-subscribe-topic",
    "cardano.sequence.bootstrapped",
    GenesisComplete,
    GenesisCompleteMessage
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

struct AccountsReaders {
    // Single use readers
    pub genesis: GenesisReader,
    pub snapshot: Option<Box<dyn Subscription<Message>>>,

    // Block readers
    pub certs: CertsReader,
    pub withdrawals: WithdrawalsReader,
    pub stake_deltas: StakeDeltasReader,
    pub gov_procedures: GovProceduresReader,

    // Epoch readers
    pub params: ParamsReader,
    pub spos: SPOReader,
    pub epoch_activity: EpochActivityReader,
    pub gov_outcomes: GovOutcomesReader,
}

// Publishers
struct AccountsPublishers {
    pub drep_distribution: DRepDistributionPublisher,
    pub spo_distribution: SPODistributionPublisher,
    pub spo_rewards: SPORewardsPublisher,
    pub stake_reward_deltas: StakeRewardDeltasPublisher,
    pub registration_updates: StakeRegistrationUpdatesPublisher,
    pub pots: PotsPublisher,
}

/// Accounts State module
#[module(
    message_type(Message),
    name = "accounts-state",
    description = "Stake and reward accounts state"
)]
pub struct AccountsState;

impl AccountsState {
    /// Wait for and process snapshot bootstrap messages
    async fn wait_for_bootstrap(
        history: Arc<Mutex<StateHistory<State>>>,
        mut snapshot_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        loop {
            let (_, message) = snapshot_subscription.read().await?;
            let message = Arc::try_unwrap(message).unwrap_or_else(Arc::unwrap_or_clone);

            if let Message::Snapshot(SnapshotMessage::Bootstrap(
                SnapshotStateMessage::AccountsState(accounts_data),
            )) = message
            {
                let block_number = accounts_data.block_number;

                let mut state = State::default();
                state.bootstrap(accounts_data)?;
                history.lock().await.bootstrap_init_with(state, block_number);

                info!("Accounts state bootstrap complete");

                return Ok(());
            }
        }
    }

    /// Async run loop
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        context: Arc<Context<Message>>,
        mut readers: AccountsReaders,
        mut publishers: AccountsPublishers,
        validation_outcomes_topic: String,
        verifier: &Verifier,
    ) -> Result<()> {
        // Wait for the snapshot bootstrap (if available)
        // Skip genesis-specific initialization when starting from snapshot
        // (pots are already loaded from snapshot bootstrap data)
        let snapshot_mode = readers.snapshot.is_some();
        if let Some(subscription) = readers.snapshot {
            Self::wait_for_bootstrap(history.clone(), subscription).await?;
        } else {
            if let RollbackWrapper::Rollback(_) = readers.params.read_with_rollbacks().await? {
                bail!("Unexpected rollback while reading initial params");
            }
            if let RollbackWrapper::Rollback(_) = readers.gov_outcomes.read_with_rollbacks().await?
            {
                bail!("Unexpected rollback while reading initial gov outcomes");
            }

            match readers.genesis.read_with_rollbacks().await? {
                RollbackWrapper::Normal((block_info, genesis_msg)) => {
                    let mut state = State::default();
                    state.handle_initial_pots(&genesis_msg.values.initial_pots)?;
                    history.lock().await.commit(block_info.number, state);

                    publishers
                        .pots
                        .publish_pots(&block_info, genesis_msg.values.initial_pots.clone())
                        .await?;
                }
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial pots");
                }
            }
        }

        // Initialize the rewards and undo log runtime
        let mut runtime = AccountsRuntime::default();

        let mut skip_first_epoch_rewards = snapshot_mode;

        // Main loop of synchronised messages
        loop {
            let mut ctx =
                ValidationContext::new(&context, &validation_outcomes_topic, "accounts_state");

            // Get a mutable state
            let mut state = {
                let history = history.lock().await;
                history.get_current_state()
            };

            let mut stake_address_undo = BlockStakeAddressUndoRecorder::default();

            // Use certs_message as the synchroniser, but we have to handle it after the
            // epoch things, because they apply to the new epoch, not the last
            let primary = PrimaryRead::from_sync(
                &mut ctx,
                "readers.certs",
                readers.certs.read_with_rollbacks().await,
            )?;

            if let Some(rollback_message) = primary.rollback_message() {
                state.rollback_stake_addresses(
                    &mut runtime.stake_address_undo_history,
                    primary.block_info().number,
                );
                state = history.lock().await.get_rolled_back_state(primary.block_info().number);
                runtime.rewards.rollback_to(primary.block_info());

                publishers.drep_distribution.publish_message(rollback_message.clone()).await?;
                publishers.spo_distribution.publish_message(rollback_message.clone()).await?;
                publishers.spo_rewards.publish_message(rollback_message.clone()).await?;
                publishers.stake_reward_deltas.publish_message(rollback_message.clone()).await?;
                publishers.registration_updates.publish_message(rollback_message.clone()).await?;
            }

            // Init drains the epoch-0 bootstrap messages, so the main loop only
            // synchronizes these side readers on rollbacks and real transitions.
            if primary.should_read_epoch_transition_messages() {
                match ctx
                    .consume_sync("readers.params", readers.params.read_with_rollbacks().await)?
                {
                    RollbackWrapper::Normal((block_info, params_msg)) => {
                        info_span!("account_state.handle_parameters", block = block_info.number)
                            .in_scope(|| state.handle_parameters(&params_msg));
                    }
                    RollbackWrapper::Rollback(_) => {}
                }

                // EPOCH rule
                // a. SNAP: Take the snapshot and pool distribution
                // rotate the snapshots (mark, set, go)
                // b. POOLREAP: for any retiring pools, refund,
                // remove from pool registry, clear delegations
                let mut stake_reward_deltas = if !primary.is_rollback() {
                    let block_info = primary.block_info();

                    let (spo_rewards, stake_reward_deltas) = ctx.handle(
                        "complete_previous_epoch_rewards_calculation",
                        state
                            .complete_previous_epoch_rewards_calculation(
                                verifier,
                                skip_first_epoch_rewards,
                                &mut runtime.rewards,
                                &mut stake_address_undo,
                            )
                            .await,
                    );

                    // Publish pool owner rewards
                    ctx.handle(
                        "publish_spo_rewards",
                        publishers
                            .spo_rewards
                            .publish_spo_rewards(primary.block_info(), spo_rewards)
                            .await,
                    );

                    // Apply pending MIRs before generating SPDD so they're included in active stake
                    state.apply_pending_mirs(&mut stake_address_undo);

                    // At the Conway hard fork, pointer addresses lose their staking
                    // functionality (Conway spec 9.1.2). Subtract accumulated pointer
                    // address UTxO values from utxo_value so they no longer count
                    // towards the stake distribution.
                    // Skip in snapshot mode: the snapshot already reflects post-Conway
                    // state, so applying the subtraction again would double-count.
                    if block_info.is_new_era && block_info.era == Era::Conway && !snapshot_mode {
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

                    // Verify and publish Stake Pool Delegation Distribution
                    let spdd = state.generate_spdd();
                    verifier.verify_spdd(block_info, &spdd);
                    ctx.handle(
                        "publish_spdd",
                        publishers.spo_distribution.publish_spdd(block_info, spdd).await,
                    );

                    stake_reward_deltas
                } else {
                    Vec::new()
                };

                // Handle SPOs
                match ctx.consume_sync("readers.spos", readers.spos.read_with_rollbacks().await)? {
                    RollbackWrapper::Normal((block_info, spo_msg)) => {
                        info_span!("account_state.handle_spo_state", block = block_info.number)
                            .in_scope(|| state.handle_spo_state(&spo_msg));
                    }
                    RollbackWrapper::Rollback(_) => {}
                }

                // Handle epoch activity
                match ctx.consume_sync(
                    "readers.epoch_activity",
                    readers.epoch_activity.read_with_rollbacks().await,
                )? {
                    RollbackWrapper::Normal((block_info, ea_msg)) => {
                        async {
                            // Add refund deltas to the stake reward deltas
                            stake_reward_deltas.extend(
                                ctx.handle(
                                    "handle_epoch_activity",
                                    state
                                        .handle_epoch_activity(
                                            context.clone(),
                                            &ea_msg,
                                            &block_info,
                                            verifier,
                                            &mut runtime.rewards,
                                            &mut stake_address_undo,
                                        )
                                        .await,
                                ),
                            );

                            // Publish stake account reward deltas
                            ctx.handle(
                                "publish_stake_reward_deltas",
                                publishers
                                    .stake_reward_deltas
                                    .publish_stake_reward_deltas(&block_info, stake_reward_deltas)
                                    .await,
                            );

                            // Publish DRep Delegation Distribution
                            ctx.handle(
                                "publish_drdd",
                                publishers
                                    .drep_distribution
                                    .publish_drdd(&block_info, state.generate_drdd())
                                    .await,
                            );
                        }
                        .instrument(info_span!(
                            "account_state.handle_epoch_activity",
                            block = block_info.number
                        ))
                        .await;
                    }
                    RollbackWrapper::Rollback(_) => {}
                }

                // Handle governance outcomes (enacted/expired proposals) at epoch boundary
                match ctx.consume_sync(
                    "readers.gov_outcomes",
                    readers.gov_outcomes.read_with_rollbacks().await,
                )? {
                    RollbackWrapper::Normal((block_info, outcomes_msg)) => {
                        async {
                            ctx.handle(
                                "handle_governance_outcomes",
                                state.handle_governance_outcomes(
                                    &outcomes_msg,
                                    &mut stake_address_undo,
                                ),
                            );
                        }
                        .instrument(info_span!(
                            "account_state.handle_governance_outcomes",
                            block = block_info.number
                        ))
                        .await;
                    }
                    RollbackWrapper::Rollback(_) => {}
                }

                // Clear the skip flag after first transition handling.
                skip_first_epoch_rewards = false;

                // publish current pots after handling the epoch transition, so that the published pots reflect the new epoch's state
                ctx.handle(
                    "publish_pots",
                    publishers.pots.publish_pots(primary.block_info(), state.get_pots()).await,
                );
            }

            // Now handle the certs_message properly
            if let Some(tx_certs_msg) = primary.message() {
                // Notify the state of the block (used to schedule reward calculations)
                state.notify_block(primary.block_info(), &mut runtime.rewards);

                let block_info = primary.block_info();
                async {
                    match state.handle_tx_certificates(
                        tx_certs_msg,
                        block_info.epoch_slot,
                        block_info.era,
                        &mut ctx,
                        &mut stake_address_undo,
                    ) {
                        Ok(updates) => ctx.handle(
                            "publishers.registration_updates.publish",
                            publishers.registration_updates.publish(block_info, updates).await,
                        ),
                        Err(e) => {
                            ctx.handle_error("handle_tx_certificates", &e);
                        }
                    }
                }
                .instrument(info_span!(
                    "account_state.handle_tx_certificates",
                    block = block_info.number
                ))
                .await;
            }

            // Handle withdrawals
            match ctx.consume_sync(
                "readers.withdrawals",
                readers.withdrawals.read_with_rollbacks().await,
            )? {
                RollbackWrapper::Normal((block_info, withdrawals_msg)) => {
                    info_span!(
                        "account_state.handle_withdrawals",
                        block = block_info.number
                    )
                    .in_scope(|| {
                        state.handle_withdrawals(
                            &withdrawals_msg,
                            &mut ctx,
                            &mut stake_address_undo,
                        );
                    });
                }
                RollbackWrapper::Rollback(_) => {}
            }

            // Handle stake address deltas
            match ctx.consume_sync(
                "stake_deltas_reader",
                readers.stake_deltas.read_with_rollbacks().await,
            )? {
                RollbackWrapper::Normal((block_info, deltas_msg)) => {
                    info_span!(
                        "account_state.handle_stake_deltas",
                        block = block_info.number
                    )
                    .in_scope(|| {
                        state.handle_stake_deltas(&deltas_msg, &mut ctx, &mut stake_address_undo);
                    });
                }
                RollbackWrapper::Rollback(_) => {}
            }

            match ctx.consume_sync(
                "governance_procedures_reader",
                readers.gov_procedures.read_with_rollbacks().await,
            )? {
                RollbackWrapper::Normal((block_info, procedures)) => {
                    info_span!(
                        "account_state.handle_governance_procedures",
                        block = block_info.number
                    )
                    .in_scope(|| {
                        state.handle_governance_procedures(&procedures);
                    });
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
        let accounts_cfg = AccountsConfig::load(context.clone(), &config).await?;

        // History
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "accounts_state",
            StateHistoryStore::default_block_store(),
        )));
        let history_query = history.clone();
        let history_tick = history.clone();

        context.handle(&accounts_cfg.accounts_query_topic, move |message| {
            let history = history_query.clone();
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

                handle_accounts_query(state, message.as_ref())
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
                context_copy,
                accounts_cfg.readers,
                accounts_cfg.publishers,
                accounts_cfg.validation_outcomes_topic,
                &accounts_cfg.verifier,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
