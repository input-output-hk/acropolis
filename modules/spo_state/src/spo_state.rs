//! Acropolis SPO state module for Caryatid
//! Accepts certificate events and derives the SPO state in memory

use acropolis_common::caryatid::{PrimaryRead, RollbackWrapper, ValidationContext};
use acropolis_common::configuration::StartupMode;
use acropolis_common::declare_cardano_reader;
use acropolis_common::messages::{
    EpochActivityMessage, GovernanceProceduresMessage, ProtocolParamsMessage, RawBlockMessage,
    SPORewardsMessage, SPOStakeDistributionMessage, StakeAddressDeltasMessage,
    StakeRewardDeltasMessage, StateTransitionMessage, TxCertificatesMessage, WithdrawalsMessage,
};
use acropolis_common::queries::errors::QueryError;

use acropolis_common::state_history::StoreType;
use acropolis_common::{
    messages::{
        CardanoMessage, Message, SPOStateMessage, SnapshotMessage, SnapshotStateMessage,
        StateQuery, StateQueryResponse,
    },
    queries::pools::{
        PoolActiveStakeInfo, PoolDelegators, PoolsListWithInfo, PoolsStateQuery,
        PoolsStateQueryResponse, DEFAULT_POOLS_QUERY_TOPIC,
    },
    rational_number::RationalNumber,
    state_history::{StateHistory, StateHistoryStore},
    Era, PoolId,
};
use anyhow::{bail, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use pallas::ledger::traverse::MultiEraHeader;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod epochs_history;
mod historical_spo_state;
mod registration_updates_publisher;
mod retired_pools_history;
mod spo_state_publisher;
mod state;
mod store_config;
#[cfg(test)]
mod test_utils;

use crate::{
    epochs_history::EpochsHistoryState,
    registration_updates_publisher::PoolRegistrationUpdatesPublisher,
    retired_pools_history::RetiredPoolsHistoryState, spo_state_publisher::SPOStatePublisher,
};
use state::State;
use store_config::StoreConfig;

// Subscribe Topics
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
    GovReader,
    "governance-subscribe-topic",
    "cardano.governance",
    GovernanceProcedures,
    GovernanceProceduresMessage
);
declare_cardano_reader!(
    BlockReader,
    "blocks-subscribe-topic",
    "cardano.block.proposed",
    BlockAvailable,
    RawBlockMessage
);
declare_cardano_reader!(
    EpochActivityReader,
    "epoch-activity-subscribe-topic",
    "cardano.epoch.activity",
    EpochActivity,
    EpochActivityMessage
);
declare_cardano_reader!(
    SPDDReader,
    "spdd-subscribe-topic",
    "cardano.spo.distribution",
    SPOStakeDistribution,
    SPOStakeDistributionMessage
);
declare_cardano_reader!(
    StakeDeltasReader,
    "stake-deltas-subscribe-topic",
    "cardano.stake.deltas",
    StakeAddressDeltas,
    StakeAddressDeltasMessage
);
declare_cardano_reader!(
    SPORewardsReader,
    "spo-rewards-subscribe-topic",
    "cardano.spo.rewards",
    SPORewards,
    SPORewardsMessage
);
declare_cardano_reader!(
    RewardsReader,
    "stake-reward-deltas-subscribe-topic",
    "cardano.stake.reward.deltas",
    StakeRewardDeltas,
    StakeRewardDeltasMessage
);
declare_cardano_reader!(
    ParamsReader,
    "protocol-parameters-subscribe-topic",
    "cardano.protocol.parameters",
    ProtocolParams,
    ProtocolParamsMessage
);
const DEFAULT_CLOCK_TICK_SUBSCRIBE_TOPIC: (&str, &str) =
    ("clock-tick-subscribe-topic", "clock.tick");
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

// Publish Topics
const DEFAULT_SPO_STATE_PUBLISH_TOPIC: (&str, &str) =
    ("publish-spo-state-topic", "cardano.spo.state");

const DEFAULT_POOL_REGISTRATION_UPDATES_PUBLISH_TOPIC: (&str, &str) = (
    "publish-pool-registration-updates-topic",
    "cardano.pool.registration.updates",
);

const DEFAULT_VALIDATION_PUBLISH_TOPIC: (&str, &str) =
    ("publish-validation-topic", "cardano.validation.spo");

/// SPO State module
#[module(
    message_type(Message),
    name = "spo-state",
    description = "In-memory SPO State from certificate events"
)]
pub struct SPOState;

impl SPOState {
    /// Wait for and process snapshot bootstrap messages
    async fn wait_for_bootstrap(
        history: Arc<Mutex<StateHistory<State>>>,
        mut snapshot_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        info!("Waiting for SPO state snapshot bootstrap messages...");

        loop {
            let Ok((_, message)) = snapshot_subscription.read().await else {
                info!("Snapshot subscription closed");
                return Ok(());
            };

            match message.as_ref() {
                Message::Snapshot(SnapshotMessage::Startup) => {
                    info!("Received snapshot startup signal, awaiting SPO bootstrap data...");
                }
                Message::Snapshot(SnapshotMessage::Bootstrap(SnapshotStateMessage::SPOState(
                    spo_bootstrap,
                ))) => {
                    info!(
                        "Bootstrapping SPO state: {} pools, {} pending updates, {} retiring",
                        spo_bootstrap.spo_state.pools.len(),
                        spo_bootstrap.spo_state.updates.len(),
                        spo_bootstrap.spo_state.retiring.len()
                    );
                    let block_number = spo_bootstrap.block_number;
                    let mut guard = history.lock().await;
                    guard.clear();
                    guard.bootstrap_init_with(spo_bootstrap.spo_state.clone().into(), block_number);
                    info!("SPO state bootstrap complete");
                }
                Message::Snapshot(SnapshotMessage::Complete) => {
                    info!("Snapshot complete, exiting SPO state bootstrap loop");
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    /// Main async run loop
    #[allow(clippy::too_many_arguments)]
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        epochs_history: EpochsHistoryState,
        retired_pools_history: RetiredPoolsHistoryState,
        context: Arc<Context<Message>>,
        store_config: &StoreConfig,
        // subscribers
        snapshot_subscription: Option<Box<dyn Subscription<Message>>>,
        mut certs_reader: CertsReader,
        mut block_reader: BlockReader,
        mut params_reader: ParamsReader,
        mut withdrawals_reader: Option<WithdrawalsReader>,
        mut gov_reader: Option<GovReader>,
        mut epoch_activity_reader: Option<EpochActivityReader>,
        mut spdd_reader: Option<SPDDReader>,
        mut stake_deltas_reader: Option<StakeDeltasReader>,
        mut spo_rewards_reader: Option<SPORewardsReader>,
        mut stake_reward_deltas_reader: Option<RewardsReader>,

        // publishers
        mut spo_state_publisher: SPOStatePublisher,
        mut pool_registration_updates_publisher: PoolRegistrationUpdatesPublisher,
        validation_publish_topic: String,
    ) -> Result<()> {
        // Wait for snapshot bootstrap if subscription is provided
        if let Some(subscription) = snapshot_subscription {
            Self::wait_for_bootstrap(history.clone(), subscription).await?;
        } else {
            // Consume initial protocol parameters (only needed for genesis bootstrap)
            match params_reader.read_with_rollbacks().await? {
                RollbackWrapper::Normal(_) => {}
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial params");
                }
            }
        }

        // Get the stake address deltas from the genesis bootstrap, which we know
        // don't contain any stake, plus an extra parameter state (!unexplained)
        // !TODO this seems overly specific to our startup process
        if let Some(sub) = stake_deltas_reader.as_mut() {
            match sub.read_with_rollbacks().await? {
                RollbackWrapper::Normal(_) => {}
                RollbackWrapper::Rollback(_) => {
                    bail!("Unexpected rollback while reading initial stake deltas");
                }
            }
        }

        // Main loop of synchronised messages
        loop {
            // Get a mutable state
            let mut state = history.lock().await.get_or_init_with(|| State::new(store_config));
            let mut ctx = ValidationContext::new(&context, &validation_publish_topic, "spo_state");

            // Use certs_message as the synchroniser
            let primary = PrimaryRead::from_sync(
                &mut ctx,
                "certs_reader",
                certs_reader.read_with_rollbacks().await,
            )?;

            if primary.is_rollback() {
                state = history.lock().await.get_rolled_back_state(primary.block_info().number);

                let rollback_message = primary
                    .rollback_message()
                    .cloned()
                    .expect("rollback primary read should include rollback message");
                spo_state_publisher.publish(rollback_message.clone()).await?;
                pool_registration_updates_publisher.publish(rollback_message).await?;
            }

            // handle blocks (handle_mint) before handle_tx_certs in case of epoch boundary
            match ctx.consume_sync("block_reader", block_reader.read_with_rollbacks().await)? {
                RollbackWrapper::Normal((block_info, block_msg)) => {
                    let span =
                        info_span!("spo_state.handle_block_header", block = block_info.number);

                    span.in_scope(|| {
                        // Derive the variant from the era - just enough to make
                        // MultiEraHeader::decode() work.
                        let variant = match block_info.era {
                            Era::Byron => 0,
                            Era::Shelley => 1,
                            Era::Allegra => 2,
                            Era::Mary => 3,
                            Era::Alonzo => 4,
                            _ => 5,
                        };

                        // Parse the header - note we ignore the subtag because EBBs
                        // are suppressed upstream
                        ctx.handle(
                            "MultiEraHeader::decode",
                            (|| {
                                let header =
                                    MultiEraHeader::decode(variant, None, &block_msg.header)
                                        .map_err(anyhow::Error::from)?;

                                if let Some(vrf_vkey) = header.vrf_vkey() {
                                    state.handle_mint(&block_info, vrf_vkey);
                                }

                                Ok(())
                            })(),
                        );
                    });
                }
                RollbackWrapper::Rollback(_) => {}
            }

            // handle tx certificates
            if let Some(tx_certs_msg) = primary.message() {
                let block_info = primary.block_info();
                let span = info_span!("spo_state.handle_certs", block = block_info.number);
                async {
                    let (message_opt, pool_registration_updates_message, outcomes) =
                        state.handle_tx_certs(block_info, tx_certs_msg);
                    ctx.merge("handle_tx_certs", Ok(outcomes));

                    if let Some(message) = message_opt {
                        if let Message::Cardano((
                            _,
                            CardanoMessage::SPOState(SPOStateMessage { retired_spos, .. }),
                        )) = message.as_ref()
                        {
                            let pool_ids: Vec<PoolId> =
                                retired_spos.iter().map(|(spo, _sa)| *spo).collect();
                            retired_pools_history.handle_deregistrations(block_info, &pool_ids);
                        }

                        // publish spo message
                        ctx.handle(
                            "spo_state_publisher.publish",
                            spo_state_publisher.publish(message).await,
                        );
                    }

                    ctx.handle(
                        "pool_registration_updates_publisher.publish",
                        pool_registration_updates_publisher
                            .publish(pool_registration_updates_message)
                            .await,
                    );
                }
                .instrument(span)
                .await;
            }

            // Init drains the epoch-0 bootstrap messages, so the main loop only
            // synchronizes these side readers on rollbacks and real transitions.
            if primary.should_read_epoch_transition_messages() {
                // Handle ProtocolParamsMessage
                match ctx
                    .consume_sync("params_reader", params_reader.read_with_rollbacks().await)?
                {
                    RollbackWrapper::Normal((block_info, params_msg)) => {
                        let span =
                            info_span!("spo_state.handle_parameters", block = block_info.number);
                        async {
                            state.handle_parameters(&params_msg);
                        }
                        .instrument(span)
                        .await;
                    }
                    RollbackWrapper::Rollback(_) => {}
                }

                if let Some(reader) = spdd_reader.as_mut() {
                    // Handle SPDD
                    match ctx.consume_sync("spdd", reader.read_with_rollbacks().await)? {
                        RollbackWrapper::Normal((block_info, spdd_message)) => {
                            // update epochs_history
                            epochs_history.handle_spdd(&block_info, &spdd_message);
                        }
                        RollbackWrapper::Rollback(_) => {}
                    }
                }

                // Handle SPO rewards
                if let Some(reader) = spo_rewards_reader.as_mut() {
                    match ctx
                        .consume_sync("spo_rewards_reader", reader.read_with_rollbacks().await)?
                    {
                        RollbackWrapper::Normal((block_info, spo_rewards_message)) => {
                            let span = info_span!(
                                "spo_state.handle_spo_rewards",
                                block = block_info.number
                            );
                            span.in_scope(|| {
                                // update epochs_history
                                ctx.handle(
                                    "handle_spo_rewards",
                                    epochs_history
                                        .handle_spo_rewards(&block_info, &spo_rewards_message)
                                        .as_result(),
                                );
                            });
                        }
                        RollbackWrapper::Rollback(_) => {}
                    }
                }

                // Handle Stake Reward Deltas
                if let Some(reader) = stake_reward_deltas_reader.as_mut() {
                    match ctx.consume_sync(
                        "stake_reward_deltas_reader",
                        reader.read_with_rollbacks().await,
                    )? {
                        RollbackWrapper::Normal((block_info, stake_reward_deltas_message)) => {
                            let span = info_span!(
                                "spo_state.handle_stake_reward_deltas",
                                block = block_info.number
                            );
                            span.in_scope(|| {
                                // update epochs_history
                                ctx.handle(
                                    "handle_stake_reward_deltas",
                                    state.handle_stake_reward_deltas(
                                        &block_info,
                                        &stake_reward_deltas_message,
                                    ),
                                );
                            });
                        }
                        RollbackWrapper::Rollback(_) => {}
                    }
                }

                // Handle EpochActivityMessage
                if let Some(reader) = epoch_activity_reader.as_mut() {
                    match ctx
                        .consume_sync("epoch_activity_reader", reader.read_with_rollbacks().await)?
                    {
                        RollbackWrapper::Normal((block_info, epoch_activity_message)) => {
                            let span = info_span!(
                                "spo_state.handle_epoch_activity",
                                block = block_info.number
                            );
                            span.in_scope(|| {
                                // update epochs_history
                                let spos: Vec<(PoolId, usize)> = epoch_activity_message
                                    .spo_blocks
                                    .iter()
                                    .map(|(hash, count)| (*hash, *count))
                                    .collect();
                                epochs_history.handle_epoch_activity(
                                    &block_info,
                                    &epoch_activity_message,
                                    &spos,
                                );
                            });
                        }
                        RollbackWrapper::Rollback(_) => {}
                    }
                }
            }

            // Handle withdrawals
            if let Some(reader) = withdrawals_reader.as_mut() {
                match ctx.consume_sync("withdrawals_reader", reader.read_with_rollbacks().await)? {
                    RollbackWrapper::Normal((block_info, withdrawals_msg)) => {
                        let span =
                            info_span!("spo_state.handle_withdrawals", block = block_info.number);
                        async {
                            ctx.handle(
                                "handle_withdrawals",
                                state.handle_withdrawals(&withdrawals_msg),
                            );
                        }
                        .instrument(span)
                        .await;
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
            }

            // Handle stake deltas
            if let Some(reader) = stake_deltas_reader.as_mut() {
                match ctx.consume_sync("stake_deltas_reader", reader.read_with_rollbacks().await)? {
                    RollbackWrapper::Normal((block_info, deltas_msg)) => {
                        let span =
                            info_span!("spo_state.handle_stake_deltas", block = block_info.number);
                        async {
                            ctx.handle(
                                "handle_stake_deltas",
                                state.handle_stake_deltas(&deltas_msg),
                            );
                        }
                        .instrument(span)
                        .await;
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
            }

            // Handle governance
            if let Some(reader) = gov_reader.as_mut() {
                match ctx.consume_sync("gov_reader", reader.read_with_rollbacks().await)? {
                    RollbackWrapper::Normal((block_info, governance_msg)) => {
                        let span =
                            info_span!("spo_state.handle_governance", block = block_info.number);
                        span.in_scope(|| {
                            ctx.handle(
                                "handle_governance",
                                state.handle_governance(&governance_msg.voting_procedures),
                            );
                        });
                    }
                    RollbackWrapper::Rollback(_) => {}
                }
            }

            // Commit the new state, publish validation outcome
            if primary.message().is_some() {
                let block_info = primary.block_info();
                history.lock().await.commit(block_info.number, state);

                if primary.do_validation() {
                    ctx.publish().await;
                }
            }
        }
    }

    /// Async run loop for clock tick messages
    async fn run_clock_tick_subscription(
        history: Arc<Mutex<StateHistory<State>>>,
        mut clock_tick_subscription: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        loop {
            // Subscribe to clock tick messages
            let (_, tick_message) = clock_tick_subscription.read().await?;
            if let Message::Clock(tick_message) = tick_message.as_ref() {
                if (tick_message.number % 60) == 0 {
                    let span = info_span!("spo_state.tick", number = tick_message.number);
                    async {
                        let state = history.lock().await.get_current_state();
                        state.tick().inspect_err(|e| error!("Tick error: {e}")).ok();
                    }
                    .instrument(span)
                    .await;
                }
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Get configuration

        let clock_tick_subscribe_topic = config
            .get_string(DEFAULT_CLOCK_TICK_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_CLOCK_TICK_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber on '{clock_tick_subscribe_topic}'");

        let snapshot_topic = config
            .get_string(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC.1.to_string());

        // Publish Topics
        let spo_state_publish_topic = config
            .get_string(DEFAULT_SPO_STATE_PUBLISH_TOPIC.0)
            .unwrap_or(DEFAULT_SPO_STATE_PUBLISH_TOPIC.1.to_string());
        info!("Creating SPO state publisher on '{spo_state_publish_topic}'");

        let pool_registration_updates_publish_topic = config
            .get_string(DEFAULT_POOL_REGISTRATION_UPDATES_PUBLISH_TOPIC.0)
            .unwrap_or(DEFAULT_POOL_REGISTRATION_UPDATES_PUBLISH_TOPIC.1.to_string());
        info!("Creating pool registration updates publisher on '{pool_registration_updates_publish_topic}'");

        let validation_publish_topic = config
            .get_string(DEFAULT_VALIDATION_PUBLISH_TOPIC.0)
            .unwrap_or(DEFAULT_VALIDATION_PUBLISH_TOPIC.1.to_string());
        info!("Validation outcome topic publisher on '{validation_publish_topic}'");

        // query topic
        let pools_query_topic = config
            .get_string(DEFAULT_POOLS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_POOLS_QUERY_TOPIC.1.to_string());
        info!("Creating query handler on '{}'", pools_query_topic);

        // store config
        let store_config = StoreConfig::from(config.clone());

        // Create history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "spo_state",
            StateHistoryStore::default_block_store(),
            &config,
            StoreType::Block,
        )));
        let history_spo_state = history.clone();
        let history_tick = history.clone();

        // Create epochs history
        let epochs_history = EpochsHistoryState::new(store_config.clone());
        let epochs_history_spo_state = epochs_history.clone();

        // Create Retired pools history
        let retired_pools_history = RetiredPoolsHistoryState::new(store_config.clone());
        let retired_pools_history_spo_state = retired_pools_history.clone();

        // handle pools-state query
        context.handle(&pools_query_topic, move |message| {
            let history = history_spo_state.clone();
            let epochs_history = epochs_history_spo_state.clone();
            let retired_pools_history = retired_pools_history_spo_state.clone();

            async move {
                let Message::StateQuery(StateQuery::Pools(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Pools(
                        PoolsStateQueryResponse::Error(QueryError::internal_error(
                            "Invalid message for pools-state",
                        )),
                    )));
                };

                let state = history.lock().await.get_current_state();

                let response = match query {
                    // NOTE:
                    // For now, we only store active pools
                    // But we need to store retired pool's information also
                    // for BF's compatibility
                    PoolsStateQuery::GetPoolInfo { pool_id } => match state.get(pool_id) {
                        Some(pool) => PoolsStateQueryResponse::PoolInfo(pool.clone()),
                        None => PoolsStateQueryResponse::Error(QueryError::not_found(format!(
                            "Pool {}",
                            pool_id
                        ))),
                    },

                    PoolsStateQuery::GetPoolsList => {
                        PoolsStateQueryResponse::PoolsList(state.list_pool_operators())
                    }

                    PoolsStateQuery::GetPoolsListWithInfo => {
                        let pools_list_with_info = PoolsListWithInfo {
                            pools: state.list_pools_with_info(),
                        };
                        PoolsStateQueryResponse::PoolsListWithInfo(pools_list_with_info)
                    }

                    PoolsStateQuery::GetPoolActiveStakeInfo {
                        pool_operator,
                        epoch,
                    } => {
                        if epochs_history.is_enabled() {
                            let epoch_state = epochs_history.get_epoch_state(pool_operator, *epoch);
                            PoolsStateQueryResponse::PoolActiveStakeInfo(PoolActiveStakeInfo {
                                active_stake: epoch_state
                                    .as_ref()
                                    .and_then(|state| state.active_stake)
                                    .unwrap_or(0),
                                active_size: epoch_state
                                    .as_ref()
                                    .and_then(|state| state.active_size.clone())
                                    .unwrap_or(RationalNumber::ZERO),
                            })
                        } else {
                            PoolsStateQueryResponse::Error(QueryError::storage_disabled(
                                "epochs history",
                            ))
                        }
                    }

                    PoolsStateQuery::GetPoolsActiveStakes {
                        pools_operators,
                        epoch,
                    } => {
                        if epochs_history.is_enabled() {
                            let active_stakes =
                                epochs_history.get_pools_active_stakes(pools_operators, *epoch);
                            PoolsStateQueryResponse::PoolsActiveStakes(
                                active_stakes.unwrap_or(vec![0; pools_operators.len()]),
                            )
                        } else {
                            PoolsStateQueryResponse::Error(QueryError::storage_disabled(
                                "epochs history",
                            ))
                        }
                    }

                    PoolsStateQuery::GetPoolsTotalBlocksMinted { pools_operators } => {
                        PoolsStateQueryResponse::PoolsTotalBlocksMinted(
                            state.get_total_blocks_minted_by_pools(pools_operators),
                        )
                    }

                    PoolsStateQuery::GetPoolHistory { pool_id } => {
                        if epochs_history.is_enabled() {
                            let history =
                                epochs_history.get_pool_history(pool_id).unwrap_or_default();
                            PoolsStateQueryResponse::PoolHistory(history)
                        } else {
                            PoolsStateQueryResponse::Error(QueryError::storage_disabled(
                                "pool epoch history",
                            ))
                        }
                    }

                    PoolsStateQuery::GetPoolsRetiringList => {
                        let retiring_pools = state.get_retiring_pools();
                        PoolsStateQueryResponse::PoolsRetiringList(retiring_pools)
                    }

                    PoolsStateQuery::GetPoolsRetiredList => {
                        if retired_pools_history.is_enabled() {
                            let retired_pools = retired_pools_history.get_retired_pools();
                            PoolsStateQueryResponse::PoolsRetiredList(retired_pools)
                        } else {
                            PoolsStateQueryResponse::Error(QueryError::storage_disabled(
                                "pool retirement history",
                            ))
                        }
                    }

                    PoolsStateQuery::GetPoolMetadata { pool_id } => {
                        let pool_metadata = state.get_pool_metadata(pool_id);
                        if let Some(pool_metadata) = pool_metadata {
                            PoolsStateQueryResponse::PoolMetadata(pool_metadata)
                        } else {
                            PoolsStateQueryResponse::Error(QueryError::not_found(format!(
                                "Pool metadata for {}",
                                pool_id
                            )))
                        }
                    }

                    PoolsStateQuery::GetPoolRelays { pool_id } => {
                        let pool_relays = state.get_pool_relays(pool_id);
                        if let Some(relays) = pool_relays {
                            PoolsStateQueryResponse::PoolRelays(relays)
                        } else {
                            PoolsStateQueryResponse::Error(QueryError::not_found(format!(
                                "Pool relays for {}",
                                pool_id
                            )))
                        }
                    }

                    PoolsStateQuery::GetPoolDelegators { pool_id } => {
                        if state.is_historical_delegators_enabled()
                            && state.is_stake_address_enabled()
                        {
                            let pool_delegators = state.get_pool_delegators(pool_id);
                            if let Some(pool_delegators) = pool_delegators {
                                PoolsStateQueryResponse::PoolDelegators(PoolDelegators {
                                    delegators: pool_delegators,
                                })
                            } else {
                                PoolsStateQueryResponse::Error(QueryError::not_found(format!(
                                    "Pool delegators for {}",
                                    pool_id
                                )))
                            }
                        } else {
                            PoolsStateQueryResponse::Error(QueryError::storage_disabled(
                                "pool delegators or stake addresses",
                            ))
                        }
                    }

                    PoolsStateQuery::GetPoolTotalBlocksMinted { pool_id } => {
                        PoolsStateQueryResponse::PoolTotalBlocksMinted(
                            state.get_total_blocks_minted_by_pool(pool_id),
                        )
                    }

                    PoolsStateQuery::GetBlocksByPool { pool_id } => {
                        if state.is_historical_blocks_enabled() {
                            PoolsStateQueryResponse::BlocksByPool(
                                state.get_blocks_by_pool(pool_id).unwrap_or_default(),
                            )
                        } else {
                            PoolsStateQueryResponse::Error(QueryError::storage_disabled(
                                "historical blocks",
                            ))
                        }
                    }

                    PoolsStateQuery::GetBlocksByPoolAndEpoch { pool_id, epoch } => {
                        if state.is_historical_blocks_enabled() {
                            PoolsStateQueryResponse::BlocksByPoolAndEpoch(
                                state
                                    .get_blocks_by_pool_and_epoch(pool_id, *epoch)
                                    .unwrap_or_default(),
                            )
                        } else {
                            PoolsStateQueryResponse::Error(QueryError::storage_disabled(
                                "historical blocks",
                            ))
                        }
                    }

                    PoolsStateQuery::GetPoolUpdates { pool_id } => {
                        if state.is_historical_updates_enabled() {
                            let pool_updates = state.get_pool_updates(pool_id);
                            if let Some(pool_updates) = pool_updates {
                                PoolsStateQueryResponse::PoolUpdates(pool_updates)
                            } else {
                                PoolsStateQueryResponse::Error(QueryError::not_found(format!(
                                    "Pool updates for {}",
                                    pool_id
                                )))
                            }
                        } else {
                            PoolsStateQueryResponse::Error(QueryError::storage_disabled(
                                "pool updates",
                            ))
                        }
                    }

                    PoolsStateQuery::GetPoolVotes { pool_id } => {
                        if state.is_historical_votes_enabled() {
                            PoolsStateQueryResponse::PoolVotes(
                                state.get_pool_votes(pool_id).unwrap_or_default(),
                            )
                        } else {
                            PoolsStateQueryResponse::Error(QueryError::storage_disabled(
                                "pool votes",
                            ))
                        }
                    }
                };

                Arc::new(Message::StateQueryResponse(StateQueryResponse::Pools(
                    response,
                )))
            }
        });

        // Subscribe for snapshot bootstrap if using snapshot startup
        let snapshot_subscription = if StartupMode::from_config(config.as_ref()).is_snapshot() {
            info!("Creating subscriber for snapshot on '{snapshot_topic}'");
            Some(context.subscribe(&snapshot_topic).await?)
        } else {
            info!("Skipping snapshot subscription (startup method is not snapshot)");
            None
        };

        // Subscriptions
        // Mandatory
        let params_reader = ParamsReader::new(&context, &config).await?;
        let certs_reader = CertsReader::new(&context, &config).await?;
        let block_reader = BlockReader::new(&context, &config).await?;
        let clock_tick_subscription = context.subscribe(&clock_tick_subscribe_topic).await?;

        // Optional depending on store features
        // only when stake_addresses are enabled
        let withdrawals_reader = if store_config.store_stake_addresses {
            Some(WithdrawalsReader::new(&context, &config).await?)
        } else {
            None
        };

        // when historical spo's votes are enabled
        let gov_reader = if store_config.store_votes {
            Some(GovReader::new(&context, &config).await?)
        } else {
            None
        };

        // when epochs_history is enabled
        let spo_rewards_reader = if store_config.store_epochs_history {
            Some(SPORewardsReader::new(&context, &config).await?)
        } else {
            None
        };
        let epoch_activity_reader = if store_config.store_epochs_history {
            Some(EpochActivityReader::new(&context, &config).await?)
        } else {
            None
        };
        let spdd_reader = if store_config.store_epochs_history {
            Some(SPDDReader::new(&context, &config).await?)
        } else {
            None
        };

        // when stake_addresses are enabled
        let stake_deltas_reader = if store_config.store_stake_addresses {
            Some(StakeDeltasReader::new(&context, &config).await?)
        } else {
            None
        };
        let stake_reward_deltas_reader = if store_config.store_stake_addresses {
            Some(RewardsReader::new(&context, &config).await?)
        } else {
            None
        };

        // Publishers
        let spo_state_publisher = SPOStatePublisher::new(context.clone(), spo_state_publish_topic);
        let pool_registration_updates_publisher = PoolRegistrationUpdatesPublisher::new(
            context.clone(),
            pool_registration_updates_publish_topic,
        );
        let context_copy = context.clone();

        context.run(async move {
            Self::run(
                history,
                epochs_history,
                retired_pools_history,
                context_copy,
                &store_config,
                snapshot_subscription,
                certs_reader,
                block_reader,
                params_reader,
                withdrawals_reader,
                gov_reader,
                epoch_activity_reader,
                spdd_reader,
                stake_deltas_reader,
                spo_rewards_reader,
                stake_reward_deltas_reader,
                spo_state_publisher,
                pool_registration_updates_publisher,
                validation_publish_topic,
            )
            .await
            .unwrap_or_else(|e| error!("Failed to run SPO State: {e}"));
        });

        context.run(async move {
            Self::run_clock_tick_subscription(history_tick, clock_tick_subscription)
                .await
                .unwrap_or_else(|e| error!("Failed to run SPO Clock Tick Subscription: {e}"));
        });

        Ok(())
    }
}
