//! Acropolis SPO state module for Caryatid
//! Accepts certificate events and derives the SPO state in memory

use acropolis_common::{
    ledger_state::SPOState as LedgerSPOState,
    messages::{
        CardanoMessage, Message, SPOStateMessage, SnapshotDumpMessage, SnapshotMessage,
        SnapshotStateMessage, StateQuery, StateQueryResponse,
    },
    queries::pools::{
        PoolActiveStakeInfo, PoolDelegators, PoolHistory, PoolRelays, PoolUpdates, PoolVotes,
        PoolsActiveStakes, PoolsList, PoolsListWithInfo, PoolsRetiredList, PoolsRetiringList,
        PoolsStateQuery, PoolsStateQueryResponse, DEFAULT_POOLS_QUERY_TOPIC,
    },
    rational_number::RationalNumber,
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus,
};
use anyhow::Result;
use caryatid_sdk::{module, Context, Module, Subscription};
use config::Config;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod epochs_history;
mod historical_spo_state;
mod retired_pools_history;
mod spo_state_publisher;
mod state;
mod store_config;
#[cfg(test)]
mod test_utils;

use crate::{
    epochs_history::EpochsHistoryState, retired_pools_history::RetiredPoolsHistoryState,
    spo_state_publisher::SPOStatePublisher,
};
use state::State;
use store_config::StoreConfig;

// Subscribe Topics
const DEFAULT_CERTIFICATES_SUBSCRIBE_TOPIC: (&str, &str) =
    ("certificates-subscribe-topic", "cardano.certificates");
const DEFAULT_WITHDRAWALS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("withdrawals-subscribe-topic", "cardano.withdrawals");
const DEFAULT_GOVERNANCE_SUBSCRIBE_TOPIC: (&str, &str) =
    ("governance-subscribe-topic", "cardano.governance");
const DEFAULT_EPOCH_ACTIVITY_SUBSCRIBE_TOPIC: (&str, &str) =
    ("epoch-activity-subscribe-topic", "cardano.epoch.activity");
const DEFAULT_SPDD_SUBSCRIBE_TOPIC: (&str, &str) =
    ("spdd-subscribe-topic", "cardano.spo.distribution");
const DEFAULT_STAKE_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("stake-deltas-subscribe-topic", "cardano.stake.deltas");
const DEFAULT_SPO_REWARDS_SUBSCRIBE_TOPIC: (&str, &str) =
    ("spo-rewards-subscribe-topic", "cardano.spo.rewards");
const DEFAULT_STAKE_REWARD_DELTAS_SUBSCRIBE_TOPIC: (&str, &str) = (
    "stake-reward-deltas-subscribe-topic",
    "cardano.stake.reward.deltas",
);
const DEFAULT_CLOCK_TICK_SUBSCRIBE_TOPIC: (&str, &str) =
    ("clock-tick-subscribe-topic", "clock.tick");
const MAYBE_SNAPSHOT_SUBSCRIBE_TOPIC: &str = "snapshot-subscribe-topic";

// Publish Topics
const DEFAULT_SPO_STATE_PUBLISH_TOPIC: (&str, &str) =
    ("publish-spo-state-topic", "cardano.spo.state");

/// SPO State module
#[module(
    message_type(Message),
    name = "spo-state",
    description = "In-memory SPO State from certificate events"
)]
pub struct SPOState;

impl SPOState {
    /// Main async run loop
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        epochs_history: EpochsHistoryState,
        retired_pools_history: RetiredPoolsHistoryState,
        store_config: &StoreConfig,
        // subscribers
        mut certificates_subscription: Box<dyn Subscription<Message>>,
        mut withdrawals_subscription: Option<Box<dyn Subscription<Message>>>,
        mut governance_subscription: Option<Box<dyn Subscription<Message>>>,
        mut epoch_activity_subscription: Box<dyn Subscription<Message>>,
        mut spdd_subscription: Box<dyn Subscription<Message>>,
        mut stake_deltas_subscription: Option<Box<dyn Subscription<Message>>>,
        mut spo_rewards_subscription: Option<Box<dyn Subscription<Message>>>,
        mut stake_reward_deltas_subscription: Option<Box<dyn Subscription<Message>>>,
        // publishers
        mut spo_state_publisher: SPOStatePublisher,
    ) -> Result<()> {
        // Get the stake address deltas from the genesis bootstrap, which we know
        // don't contain any stake, plus an extra parameter state (!unexplained)
        // !TODO this seems overly specific to our startup process
        match stake_deltas_subscription.as_mut() {
            Some(sub) => {
                let _ = sub.read().await?;
            }
            None => {}
        }

        // Main loop of synchronised messages
        loop {
            // Get a mutable state
            let mut state = history.lock().await.get_or_init_with(|| State::new(store_config));
            let mut current_block: Option<BlockInfo> = None;

            // read per-block topics in parallel
            let certs_message_f = certificates_subscription.read();
            let withdrawals_message_f = withdrawals_subscription.as_mut().map(|s| s.read());
            let governance_message_f = governance_subscription.as_mut().map(|s| s.read());
            let stake_deltas_message_f = stake_deltas_subscription.as_mut().map(|s| s.read());

            // Use certs_message as the synchroniser
            let (_, certs_message) = certs_message_f.await?;
            let new_epoch = match certs_message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::TxCertificates(tx_certs_msg))) => {
                    // Handle rollbacks on this topic only
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(block_info.number);
                    }
                    current_block = Some(block_info.clone());

                    let span = info_span!("spo_state.handle_certs", block = block_info.number);
                    async {
                        Self::check_sync(&current_block, &block_info);
                        let maybe_message = state
                            .handle_tx_certs(block_info, tx_certs_msg)
                            .inspect_err(|e| error!("TxCerts Messages handling error: {e}"))
                            .ok();

                        if let Some(Some(message)) = maybe_message {
                            if let Message::Cardano((
                                _,
                                CardanoMessage::SPOState(SPOStateMessage { retired_spos, .. }),
                            )) = message.as_ref()
                            {
                                retired_pools_history
                                    .handle_deregistrations(block_info, retired_spos);
                            }

                            // publish spo message
                            if let Err(e) = spo_state_publisher.publish(message).await {
                                error!("Error publishing SPO State: {e:#}")
                            }
                        }
                    }
                    .instrument(span)
                    .await;

                    // new_epoch?
                    block_info.new_epoch && block_info.epoch > 0
                }

                _ => {
                    error!("Unexpected message type: {certs_message:?}");
                    false
                }
            };

            // read from epoch-boundary messages only when it's a new epoch
            if new_epoch {
                let spdd_message_f = spdd_subscription.read();
                let spo_rewards_message_f = spo_rewards_subscription.as_mut().map(|s| s.read());
                let ea_message_f = epoch_activity_subscription.read();
                let stake_reward_deltas_message_f =
                    stake_reward_deltas_subscription.as_mut().map(|s| s.read());

                // Handle SPDD
                let (_, spdd_message) = spdd_message_f.await?;
                if let Message::Cardano((
                    block_info,
                    CardanoMessage::SPOStakeDistribution(spdd_message),
                )) = spdd_message.as_ref()
                {
                    let span = info_span!("spo_state.handle_spdd", block = block_info.number);
                    span.in_scope(|| {
                        Self::check_sync(&current_block, &block_info);
                        // update epochs_history
                        epochs_history.handle_spdd(block_info, spdd_message);
                    });
                }

                // Handle SPO rewards
                if let Some(spo_rewards_message_f) = spo_rewards_message_f {
                    let (_, spo_rewards_message) = spo_rewards_message_f.await?;
                    if let Message::Cardano((
                        block_info,
                        CardanoMessage::SPORewards(spo_rewards_message),
                    )) = spo_rewards_message.as_ref()
                    {
                        let span =
                            info_span!("spo_state.handle_spo_rewards", block = block_info.number);
                        span.in_scope(|| {
                            Self::check_sync(&current_block, &block_info);
                            // update epochs_history
                            epochs_history.handle_spo_rewards(block_info, spo_rewards_message);
                        });
                    }
                }

                // Handle Stake Reward Deltas
                if let Some(stake_reward_deltas_message_f) = stake_reward_deltas_message_f {
                    let (_, stake_reward_deltas_message) = stake_reward_deltas_message_f.await?;
                    if let Message::Cardano((
                        block_info,
                        CardanoMessage::StakeRewardDeltas(stake_reward_deltas_message),
                    )) = stake_reward_deltas_message.as_ref()
                    {
                        let span = info_span!(
                            "spo_state.handle_stake_reward_deltas",
                            block = block_info.number
                        );
                        span.in_scope(|| {
                            Self::check_sync(&current_block, &block_info);
                            // update epochs_history
                            state
                                .handle_stake_reward_deltas(block_info, stake_reward_deltas_message)
                                .inspect_err(|e| error!("StakeRewardDeltas handling error: {e:#}"))
                                .ok();
                        });
                    }
                }

                // Handle EochActivityMessage
                let (_, ea_message) = ea_message_f.await?;
                if let Message::Cardano((
                    block_info,
                    CardanoMessage::EpochActivity(epoch_activity_message),
                )) = ea_message.as_ref()
                {
                    let span =
                        info_span!("spo_state.handle_epoch_activity", block = block_info.number);
                    span.in_scope(|| {
                        Self::check_sync(&current_block, &block_info);
                        // update epochs_history
                        // epochs_history is keyed by spo not vrf_key_hash
                        let spos = state
                            .get_blocks_minted_by_spos(&epoch_activity_message.vrf_vkey_hashes);
                        epochs_history.handle_epoch_activity(
                            block_info,
                            epoch_activity_message,
                            &spos,
                        );
                    });
                }
            }

            // Handle withdrawals
            if let Some(withdrawals_message_f) = withdrawals_message_f {
                let (_, message) = withdrawals_message_f.await?;
                match message.as_ref() {
                    Message::Cardano((
                        block_info,
                        CardanoMessage::Withdrawals(withdrawals_msg),
                    )) => {
                        let span =
                            info_span!("spo_state.handle_withdrawals", block = block_info.number);
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
            }

            // Handle stake deltas
            if let Some(stake_deltas_message_f) = stake_deltas_message_f {
                let (_, message) = stake_deltas_message_f.await?;
                match message.as_ref() {
                    Message::Cardano((
                        block_info,
                        CardanoMessage::StakeAddressDeltas(deltas_msg),
                    )) => {
                        let span =
                            info_span!("spo_state.handle_stake_deltas", block = block_info.number);
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
            }

            // Handle governance
            if let Some(governance_message_f) = governance_message_f {
                let (_, message) = governance_message_f.await?;
                match message.as_ref() {
                    Message::Cardano((
                        block_info,
                        CardanoMessage::GovernanceProcedures(governance_msg),
                    )) => {
                        let span =
                            info_span!("spo_state.handle_governance", block = block_info.number);
                        span.in_scope(|| {
                            Self::check_sync(&current_block, &block_info);
                            state
                                .handle_governance(&governance_msg.voting_procedures)
                                .inspect_err(|e| error!("Governance handling error: {e:#}"))
                                .ok();
                        });
                    }

                    _ => error!("Unexpected message type: {message:?}"),
                }
            }

            // Commit the new state
            if let Some(block_info) = current_block {
                history.lock().await.commit(block_info.number, state);
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
        let certificates_subscribe_topic = config
            .get_string(DEFAULT_CERTIFICATES_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_CERTIFICATES_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber on '{certificates_subscribe_topic}'");

        let withdrawals_subscribe_topic = config
            .get_string(DEFAULT_WITHDRAWALS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_WITHDRAWALS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating withdrawals subscriber on '{withdrawals_subscribe_topic}'");

        let governance_subscribe_topic = config
            .get_string(DEFAULT_GOVERNANCE_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_GOVERNANCE_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating governance subscriber on '{governance_subscribe_topic}'");

        let stake_deltas_subscribe_topic = config
            .get_string(DEFAULT_STAKE_DELTAS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_STAKE_DELTAS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating stake deltas subscriber on '{stake_deltas_subscribe_topic}'");

        let epoch_activity_subscribe_topic = config
            .get_string(DEFAULT_EPOCH_ACTIVITY_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_EPOCH_ACTIVITY_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber on '{epoch_activity_subscribe_topic}'");

        let spdd_subscribe_topic = config
            .get_string(DEFAULT_SPDD_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SPDD_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber on '{spdd_subscribe_topic}'");

        let spo_rewards_subscribe_topic = config
            .get_string(DEFAULT_SPO_REWARDS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_SPO_REWARDS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating SPO rewards subscriber on '{spo_rewards_subscribe_topic}'");

        let stake_reward_deltas_subscribe_topic = config
            .get_string(DEFAULT_STAKE_REWARD_DELTAS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_STAKE_REWARD_DELTAS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating stake reward deltas subscriber on '{stake_reward_deltas_subscribe_topic}'");

        let clock_tick_subscribe_topic = config
            .get_string(DEFAULT_CLOCK_TICK_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_CLOCK_TICK_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber on '{clock_tick_subscribe_topic}'");

        let maybe_snapshot_topic = config
            .get_string(MAYBE_SNAPSHOT_SUBSCRIBE_TOPIC)
            .ok()
            .inspect(|snapshot_topic| info!("Creating subscriber on '{snapshot_topic}'"));

        // Publish Topics
        let spo_state_publish_topic = config
            .get_string(DEFAULT_SPO_STATE_PUBLISH_TOPIC.0)
            .unwrap_or(DEFAULT_SPO_STATE_PUBLISH_TOPIC.1.to_string());
        info!("Creating SPO state publisher on '{spo_state_publish_topic}'");

        // query topic
        let pools_query_topic = config
            .get_string(DEFAULT_POOLS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_POOLS_QUERY_TOPIC.1.to_string());
        info!("Creating query handler on '{}'", pools_query_topic);

        // store config
        let store_config = StoreConfig::from(config);

        // Create history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "spo_state",
            StateHistoryStore::default_block_store(),
        )));
        let history_spo_state = history.clone();
        let history_tick = history.clone();
        let history_snapshot = history.clone();

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
                        PoolsStateQueryResponse::Error("Invalid message for pools-state".into()),
                    )));
                };

                let state = history.lock().await.get_current_state();

                let response = match query {
                    PoolsStateQuery::GetPoolsList => {
                        let pools_list = PoolsList {
                            pool_operators: state.list_pool_operators(),
                        };
                        PoolsStateQueryResponse::PoolsList(pools_list)
                    }
                    PoolsStateQuery::GetPoolsListWithInfo => {
                        let pools_list_with_info = PoolsListWithInfo {
                            pools: state.list_pools_with_info(),
                        };
                        PoolsStateQueryResponse::PoolsListWithInfo(pools_list_with_info)
                    }
                    PoolsStateQuery::GetPoolInfo { pool_id } => {
                        let pool_info = state.get(pool_id);
                        if let Some(pool_info) = pool_info {
                            PoolsStateQueryResponse::PoolInfo(pool_info.clone())
                        } else {
                            PoolsStateQueryResponse::NotFound
                        }
                    }

                    PoolsStateQuery::GetPoolActiveStakeInfo { pool_operator, epoch } => {
                        if epochs_history.is_enabled() {
                            let epoch_state = epochs_history.get_epoch_state(pool_operator, *epoch);
                            PoolsStateQueryResponse::PoolActiveStakeInfo(PoolActiveStakeInfo {
                                active_stake: epoch_state.as_ref().and_then(|state| state.active_stake).unwrap_or(0),
                                active_size: epoch_state.as_ref().and_then(|state| state.active_size).unwrap_or(RationalNumber::from(0)),
                            })
                        } else {
                            PoolsStateQueryResponse::Error("Epochs history is not enabled".into())
                        }
                    },

                    PoolsStateQuery::GetPoolsActiveStakes {
                        pools_operators,
                        epoch,
                    } => {
                        if epochs_history.is_enabled() {
                            let active_stakes = epochs_history.get_pools_active_stakes(pools_operators, *epoch);
                            PoolsStateQueryResponse::PoolsActiveStakes(PoolsActiveStakes {
                                active_stakes: active_stakes.unwrap_or(vec![0; pools_operators.len()]),
                            })
                        } else {
                            PoolsStateQueryResponse::Error("Epochs history is not enabled".into())
                        }
                    }

                    PoolsStateQuery::GetPoolsRetiringList => {
                        let retiring_pools = state.get_retiring_pools();
                        PoolsStateQueryResponse::PoolsRetiringList(PoolsRetiringList {
                            retiring_pools,
                        })
                    }

                    PoolsStateQuery::GetPoolsRetiredList => {
                        if retired_pools_history.is_enabled() {
                            let retired_pools = retired_pools_history.get_retired_pools();
                            PoolsStateQueryResponse::PoolsRetiredList(PoolsRetiredList {
                                retired_pools,
                            })
                        } else {
                            PoolsStateQueryResponse::Error(
                                "Pool retirement history is not enabled".into(),
                            )
                        }
                    }

                    PoolsStateQuery::GetPoolHistory { pool_id } => {
                        if epochs_history.is_enabled() {
                            let history =
                                epochs_history.get_pool_history(pool_id).unwrap_or(Vec::new());
                            PoolsStateQueryResponse::PoolHistory(PoolHistory { history })
                        } else {
                            PoolsStateQueryResponse::Error(
                                "Pool Epoch history is not enabled".into(),
                            )
                        }
                    }

                    PoolsStateQuery::GetPoolMetadata { pool_id } => {
                        // NOTE:
                        // we need to check retired pools metadata
                        // to do so, we need to save retired pool's registration
                        //
                        let pool_metadata = state.get_pool_metadata(pool_id);
                        if let Some(pool_metadata) = pool_metadata {
                            PoolsStateQueryResponse::PoolMetadata(pool_metadata)
                        } else {
                            PoolsStateQueryResponse::NotFound
                        }
                    }

                    PoolsStateQuery::GetPoolRelays { pool_id } => {
                        let pool_relays = state.get_pool_relays(pool_id);
                        if let Some(relays) = pool_relays {
                            PoolsStateQueryResponse::PoolRelays(PoolRelays { relays })
                        } else {
                            PoolsStateQueryResponse::NotFound
                        }
                    }

                    PoolsStateQuery::GetPoolDelegators { pool_id } => {
                        if state.is_historical_delegators_enabled() && state.is_stake_address_enabled() {
                            let pool_delegators = state.get_pool_delegators(pool_id);
                            if let Some(pool_delegators) = pool_delegators {
                                PoolsStateQueryResponse::PoolDelegators(PoolDelegators {
                                    delegators: pool_delegators,
                                })
                            } else {
                                PoolsStateQueryResponse::NotFound
                            }
                        } else {
                            PoolsStateQueryResponse::Error("Pool delegators are not enabled or stake addresses are not enabled".into())
                        }
                    }

                    PoolsStateQuery::GetPoolUpdates { pool_id } => {
                        if state.is_historical_updates_enabled() {
                            let pool_updates = state.get_pool_updates(pool_id);
                            if let Some(pool_updates) = pool_updates {
                                PoolsStateQueryResponse::PoolUpdates(PoolUpdates {
                                    updates: pool_updates,
                                })
                            } else {
                                PoolsStateQueryResponse::NotFound
                            }
                        } else {
                            PoolsStateQueryResponse::Error("Pool updates are not enabled".into())
                        }
                    }

                    PoolsStateQuery::GetPoolVotes { pool_id } => {
                        if state.is_historical_votes_enabled() {
                            let pool_votes = state.get_pool_votes(pool_id);
                            if let Some(pool_votes) = pool_votes {
                                PoolsStateQueryResponse::PoolVotes(PoolVotes {
                                    votes: pool_votes,
                                })
                            } else {
                                PoolsStateQueryResponse::NotFound
                            }
                        } else {
                            PoolsStateQueryResponse::Error("Pool updates are not enabled".into())
                        }
                    }
                };

                Arc::new(Message::StateQueryResponse(StateQueryResponse::Pools(
                    response,
                )))
            }
        });

        // Subscribe for snapshot messages, if allowed
        if let Some(snapshot_topic) = maybe_snapshot_topic {
            let mut subscription = context.subscribe(&snapshot_topic).await?;
            let context_snapshot = context.clone();
            let history = history_snapshot.clone();
            context.run(async move {
                let Ok((_, message)) = subscription.read().await else {
                    return;
                };

                let mut guard = history.lock().await;
                match message.as_ref() {
                    Message::Snapshot(SnapshotMessage::Bootstrap(
                        SnapshotStateMessage::SPOState(spo_state),
                    )) => {
                        guard.clear();
                        guard.commit_forced(spo_state.clone().into());
                    }
                    Message::Snapshot(SnapshotMessage::DumpRequest(SnapshotDumpMessage {
                        block_height,
                    })) => {
                        info!("inspecting state at block height {}", block_height);
                        let maybe_spo_state = guard
                            .get_by_index_reverse(*block_height)
                            .map(|state| LedgerSPOState::from(state));

                        if let Some(spo_state) = maybe_spo_state {
                            context_snapshot
                                .message_bus
                                .publish(
                                    &snapshot_topic,
                                    Arc::new(Message::Snapshot(SnapshotMessage::Dump(
                                        SnapshotStateMessage::SPOState(spo_state),
                                    ))),
                                )
                                .await
                                .unwrap_or_else(|e| error!("failed to publish snapshot dump: {e}"))
                        }
                    }
                    _ => error!("Unexpected message type: {message:?}"),
                }
            });
        }

        // Subscriptions
        let certificates_subscription = context.subscribe(&certificates_subscribe_topic).await?;
        // only when stake_addresses are enabled
        let withdrawals_subscription = if store_config.store_stake_addresses {
            Some(context.subscribe(&withdrawals_subscribe_topic).await?)
        } else {
            None
        };
        // when historical spo's votes are enabled
        let governance_subscription = if store_config.store_votes {
            Some(context.subscribe(&governance_subscribe_topic).await?)
        } else {
            None
        };
        let epoch_activity_subscription =
            context.subscribe(&epoch_activity_subscribe_topic).await?;
        let spdd_subscription = context.subscribe(&spdd_subscribe_topic).await?;
        // when epochs_history is enabled
        let spo_rewards_subscription = if store_config.store_epochs_history {
            Some(context.subscribe(&spo_rewards_subscribe_topic).await?)
        } else {
            None
        };
        // when state_addresses are enabled
        let stake_deltas_subscription = if store_config.store_stake_addresses {
            Some(context.subscribe(&stake_deltas_subscribe_topic).await?)
        } else {
            None
        };
        let stake_reward_deltas_subscription = if store_config.store_stake_addresses {
            Some(context.subscribe(&stake_reward_deltas_subscribe_topic).await?)
        } else {
            None
        };
        let clock_tick_subscription = context.subscribe(&clock_tick_subscribe_topic).await?;

        // Publishers
        let spo_state_publisher = SPOStatePublisher::new(context.clone(), spo_state_publish_topic);

        context.run(async move {
            Self::run(
                history,
                epochs_history,
                retired_pools_history,
                &store_config,
                certificates_subscription,
                withdrawals_subscription,
                governance_subscription,
                epoch_activity_subscription,
                spdd_subscription,
                stake_deltas_subscription,
                spo_rewards_subscription,
                stake_reward_deltas_subscription,
                spo_state_publisher,
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
}
