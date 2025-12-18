//! Acropolis SPO state module for Caryatid
//! Accepts certificate events and derives the SPO state in memory

use acropolis_common::caryatid::SubscriptionExt;
use acropolis_common::configuration::StartupMethod;
use acropolis_common::messages::StateTransitionMessage;
use acropolis_common::queries::errors::QueryError;
use acropolis_common::validation::ValidationOutcomes;
use acropolis_common::{
    ledger_state::SPOState as LedgerSPOState,
    messages::{
        CardanoMessage, Message, SPOStateMessage, SnapshotDumpMessage, SnapshotMessage,
        SnapshotStateMessage, StateQuery, StateQueryResponse,
    },
    queries::pools::{
        PoolActiveStakeInfo, PoolDelegators, PoolsListWithInfo, PoolsStateQuery,
        PoolsStateQueryResponse, DEFAULT_POOLS_QUERY_TOPIC,
    },
    rational_number::RationalNumber,
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus, Era, PoolId,
};
use anyhow::{anyhow, Result};
use caryatid_sdk::{module, Context, Subscription};
use config::Config;
use pallas::ledger::traverse::MultiEraHeader;
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
const DEFAULT_BLOCK_SUBSCRIBE_TOPIC: (&str, &str) =
    ("block-subscribe-topic", "cardano.block.proposed");
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
const DEFAULT_SNAPSHOT_SUBSCRIBE_TOPIC: (&str, &str) =
    ("snapshot-subscribe-topic", "cardano.snapshot");

// Publish Topics
const DEFAULT_SPO_STATE_PUBLISH_TOPIC: (&str, &str) =
    ("publish-spo-state-topic", "cardano.spo.state");

const DEFAULT_VALIDATION_PUBLISH_TOPIC: (&str, &str) =
    ("publish-validation-topic", "cardano.validation.spo");

/// SPO State module
#[module(
    message_type(Message),
    name = "spo-state",
    description = "In-memory SPO State from certificate events"
)]
pub struct SPOState;

struct SPOStateImpl {
    validation: ValidationOutcomes,
    current_block: Option<BlockInfo>,
    context: Arc<Context<Message>>,
    validation_topic: String,
}

impl SPOStateImpl {
    pub fn new(context: &Arc<Context<Message>>, validation_topic: &str) -> Self {
        Self {
            validation: ValidationOutcomes::new(),
            current_block: None,
            context: context.clone(),
            validation_topic: validation_topic.to_owned(),
        }
    }

    pub fn set_current_block(&mut self, block: &BlockInfo) {
        self.current_block = Some(block.clone());
    }

    pub fn unexpected_message_type(&mut self, topic: &str, msg: &Message) {
        self.validation.push_anyhow(anyhow!(
            "Unexpected message type for {topic} topic: {msg:?}"
        ));
    }

    pub fn handling_error(&mut self, handler: &str, error: &anyhow::Error) {
        self.validation.push_anyhow(anyhow!("Error handling {handler}: {error:#}"));
    }

    pub fn merge_handling(&mut self, handler: &str, outcome: Result<ValidationOutcomes>) {
        match outcome {
            Err(e) => self.handling_error(handler, &e),
            Ok(mut outcome) => self.validation.merge(&mut outcome),
        }
    }

    pub async fn publish(&mut self) {
        if let Some(blk) = &self.current_block {
            if let Err(e) =
                self.validation.publish(&self.context, &self.validation_topic, blk).await
            {
                error!("Publish failed: {:?}", e);
            }
        } else {
            self.validation.print_errors(None);
        }
    }

    /// Check for synchronisation
    fn check_sync(&mut self, actual: &BlockInfo) {
        if let Some(ref block) = self.current_block {
            if block.number != actual.number {
                self.validation.push_anyhow(anyhow!(
                    "Messages out of sync: expected {}, actual {}",
                    block.number,
                    actual.number
                ));
            }
        }
    }
}

impl SPOState {
    /// Main async run loop
    #[allow(clippy::too_many_arguments)]
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        epochs_history: EpochsHistoryState,
        retired_pools_history: RetiredPoolsHistoryState,
        context: Arc<Context<Message>>,
        store_config: &StoreConfig,
        // subscribers
        mut certificates_subscription: Box<dyn Subscription<Message>>,
        mut block_subscription: Box<dyn Subscription<Message>>,
        mut withdrawals_subscription: Option<Box<dyn Subscription<Message>>>,
        mut governance_subscription: Option<Box<dyn Subscription<Message>>>,
        mut epoch_activity_subscription: Box<dyn Subscription<Message>>,
        mut spdd_subscription: Box<dyn Subscription<Message>>,
        mut stake_deltas_subscription: Option<Box<dyn Subscription<Message>>>,
        mut spo_rewards_subscription: Option<Box<dyn Subscription<Message>>>,
        mut stake_reward_deltas_subscription: Option<Box<dyn Subscription<Message>>>,
        // publishers
        mut spo_state_publisher: SPOStatePublisher,
        validation_publish_topic: String,
    ) -> Result<()> {
        // Get the stake address deltas from the genesis bootstrap, which we know
        // don't contain any stake, plus an extra parameter state (!unexplained)
        // !TODO this seems overly specific to our startup process
        if let Some(sub) = stake_deltas_subscription.as_mut() {
            let _ = sub.read().await?;
        }

        // Main loop of synchronised messages
        loop {
            // Get a mutable state
            let mut state = history.lock().await.get_or_init_with(|| State::new(store_config));
            let mut ctx = SPOStateImpl::new(&context, &validation_publish_topic);

            // Use certs_message as the synchroniser
            let (_, certs_message) = certificates_subscription.read().await?;
            let new_epoch = match certs_message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::TxCertificates(_))) => {
                    // Handle rollbacks on this topic only
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(block_info.number);
                    }
                    ctx.set_current_block(block_info);

                    // new_epoch?
                    block_info.new_epoch && block_info.epoch > 0
                }

                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                )) => {
                    spo_state_publisher.publish_rollback(certs_message.clone()).await?;
                    false
                }

                _ => {
                    ctx.unexpected_message_type("certificates", &certs_message);
                    false
                }
            };

            // handle blocks (handle_mint) before handle_tx_certs
            // in case of epoch boundary
            let (_, block_message) = block_subscription.read_ignoring_rollbacks().await?;
            match block_message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::BlockAvailable(block_msg))) => {
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
                        match MultiEraHeader::decode(variant, None, &block_msg.header) {
                            Ok(header) => {
                                if let Some(vrf_vkey) = header.vrf_vkey() {
                                    state.handle_mint(block_info, vrf_vkey);
                                }
                            }

                            Err(e) => ctx.validation.push_anyhow(anyhow!(
                                "Can't decode header {}: {e}",
                                block_info.slot
                            )),
                        }
                    });
                }

                _ => ctx.unexpected_message_type("block header", &block_message),
            }

            // handle tx certificates
            match certs_message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::TxCertificates(tx_certs_msg))) => {
                    let span = info_span!("spo_state.handle_certs", block = block_info.number);
                    async {
                        ctx.check_sync(block_info);
                        let maybe_message = state
                            .handle_tx_certs(block_info, tx_certs_msg, &mut ctx.validation)
                            .inspect_err(|e| ctx.handling_error("TxCerts", e))
                            .ok();

                        if let Some(Some(message)) = maybe_message {
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
                            if let Err(e) = spo_state_publisher.publish(message).await {
                                ctx.validation
                                    .push_anyhow(anyhow!("Error publishing SPO State: {e:#}"))
                            }
                        }
                    }
                    .instrument(span)
                    .await;
                }

                Message::Cardano((
                    _,
                    CardanoMessage::StateTransition(StateTransitionMessage::Rollback(_)),
                )) => {
                    // Do nothing, we handled rollback earlier
                }

                _ => ctx.unexpected_message_type("tx certificates", &certs_message),
            };

            // read from epoch-boundary messages only when it's a new epoch
            if new_epoch {
                // Handle SPDD
                let (_, spdd_message) = spdd_subscription.read_ignoring_rollbacks().await?;
                if let Message::Cardano((
                    block_info,
                    CardanoMessage::SPOStakeDistribution(spdd_message),
                )) = spdd_message.as_ref()
                {
                    let span = info_span!("spo_state.handle_spdd", block = block_info.number);
                    span.in_scope(|| {
                        ctx.check_sync(block_info);
                        // update epochs_history
                        epochs_history.handle_spdd(block_info, spdd_message);
                    });
                }

                // Handle SPO rewards
                if let Some(spo_rewards_subscription) = spo_rewards_subscription.as_mut() {
                    let (_, spo_rewards_message) =
                        spo_rewards_subscription.read_ignoring_rollbacks().await?;
                    if let Message::Cardano((
                        block_info,
                        CardanoMessage::SPORewards(spo_rewards_message),
                    )) = spo_rewards_message.as_ref()
                    {
                        let span =
                            info_span!("spo_state.handle_spo_rewards", block = block_info.number);
                        span.in_scope(|| {
                            ctx.check_sync(block_info);
                            // update epochs_history
                            ctx.validation.merge(
                                &mut epochs_history
                                    .handle_spo_rewards(block_info, spo_rewards_message),
                            );
                        });
                    }
                }

                // Handle Stake Reward Deltas
                if let Some(stake_reward_deltas_subscription) =
                    stake_reward_deltas_subscription.as_mut()
                {
                    let (_, stake_reward_deltas_message) =
                        stake_reward_deltas_subscription.read_ignoring_rollbacks().await?;
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
                            ctx.check_sync(block_info);
                            // update epochs_history
                            ctx.merge_handling(
                                "StakeRewardDeltas",
                                state.handle_stake_reward_deltas(
                                    block_info,
                                    stake_reward_deltas_message,
                                ),
                            );
                        });
                    }
                }

                // Handle EpochActivityMessage
                let (_, ea_message) = epoch_activity_subscription.read_ignoring_rollbacks().await?;
                if let Message::Cardano((
                    block_info,
                    CardanoMessage::EpochActivity(epoch_activity_message),
                )) = ea_message.as_ref()
                {
                    let span =
                        info_span!("spo_state.handle_epoch_activity", block = block_info.number);
                    span.in_scope(|| {
                        ctx.check_sync(block_info);
                        // update epochs_history
                        let spos: Vec<(PoolId, usize)> = epoch_activity_message
                            .spo_blocks
                            .iter()
                            .map(|(hash, count)| (*hash, *count))
                            .collect();
                        epochs_history.handle_epoch_activity(
                            block_info,
                            epoch_activity_message,
                            &spos,
                        );
                    });
                }
            }

            // Handle withdrawals
            if let Some(withdrawals_subscription) = withdrawals_subscription.as_mut() {
                let (_, message) = withdrawals_subscription.read_ignoring_rollbacks().await?;
                match message.as_ref() {
                    Message::Cardano((
                        block_info,
                        CardanoMessage::Withdrawals(withdrawals_msg),
                    )) => {
                        let span =
                            info_span!("spo_state.handle_withdrawals", block = block_info.number);
                        async {
                            ctx.check_sync(block_info);
                            state
                                .handle_withdrawals(withdrawals_msg)
                                .inspect_err(|e| ctx.handling_error("Withdrawals", e))
                                .ok();
                        }
                        .instrument(span)
                        .await;
                    }

                    _ => ctx.unexpected_message_type("spo state", &message),
                }
            }

            // Handle stake deltas
            if let Some(stake_deltas_subscription) = stake_deltas_subscription.as_mut() {
                let (_, message) = stake_deltas_subscription.read_ignoring_rollbacks().await?;
                match message.as_ref() {
                    Message::Cardano((
                        block_info,
                        CardanoMessage::StakeAddressDeltas(deltas_msg),
                    )) => {
                        let span =
                            info_span!("spo_state.handle_stake_deltas", block = block_info.number);
                        async {
                            ctx.check_sync(block_info);
                            state
                                .handle_stake_deltas(deltas_msg)
                                .inspect_err(|e| ctx.handling_error("StakeAddressDeltas", e))
                                .ok();
                        }
                        .instrument(span)
                        .await;
                    }

                    _ => ctx.unexpected_message_type("stake delta", &message),
                }
            }

            // Handle governance
            if let Some(governance_subscription) = governance_subscription.as_mut() {
                let (_, message) = governance_subscription.read_ignoring_rollbacks().await?;
                match message.as_ref() {
                    Message::Cardano((
                        block_info,
                        CardanoMessage::GovernanceProcedures(governance_msg),
                    )) => {
                        let span =
                            info_span!("spo_state.handle_governance", block = block_info.number);
                        span.in_scope(|| {
                            ctx.check_sync(block_info);
                            state
                                .handle_governance(&governance_msg.voting_procedures)
                                .inspect_err(|e| ctx.handling_error("Governance", e))
                                .ok();
                        });
                    }

                    _ => ctx.unexpected_message_type("governance", &message),
                }
            }

            // Commit the new state, publish validation outcome
            if let Some(block_info) = &ctx.current_block {
                history.lock().await.commit(block_info.number, state);
            }

            ctx.publish().await;
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

        let block_subscribe_topic = config
            .get_string(DEFAULT_BLOCK_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_BLOCK_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating block subscriber on '{block_subscribe_topic}'");

        let stake_reward_deltas_subscribe_topic = config
            .get_string(DEFAULT_STAKE_REWARD_DELTAS_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_STAKE_REWARD_DELTAS_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating stake reward deltas subscriber on '{stake_reward_deltas_subscribe_topic}'");

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

        // Subscribe for snapshot messages if using snapshot startup
        if StartupMethod::from_config(config.as_ref()).is_snapshot() {
            info!("Creating subscriber for snapshot on '{snapshot_topic}'");
            let mut subscription = context.subscribe(&snapshot_topic).await?;
            let snapshot_topic = snapshot_topic.clone();
            let context_snapshot = context.clone();
            let history = history_snapshot.clone();
            enum SnapshotState {
                Preparing,
                Started,
            }
            let mut snapshot_state = SnapshotState::Preparing;
            context.run(async move {
                loop {
                    let Ok((_, message)) = subscription.read().await else {
                        return;
                    };

                    let mut guard = history.lock().await;
                    match message.as_ref() {
                        Message::Snapshot(SnapshotMessage::Startup) => {
                            match snapshot_state {
                                SnapshotState::Preparing => {
                                    info!("Received snapshot startup signal, awaiting SPO bootstrap data...");
                                    snapshot_state = SnapshotState::Started;
                                }
                                _ => error!("Snapshot Startup message received but we have already left preparing state"),
                            }
                        }
                        Message::Snapshot(SnapshotMessage::Bootstrap(
                            SnapshotStateMessage::SPOState(spo_state),
                        )) => {
                            info!(
                                "Bootstrapping SPO state: {} pools, {} pending updates, {} retiring",
                                spo_state.pools.len(),
                                spo_state.updates.len(),
                                spo_state.retiring.len()
                            );
                            guard.clear();
                            guard.commit_forced(spo_state.clone().into());
                            info!("SPO state bootstrap complete");
                        }
                        Message::Snapshot(SnapshotMessage::DumpRequest(SnapshotDumpMessage {
                            block_height,
                        })) => {
                            info!("inspecting state at block height {}", block_height);
                            let maybe_spo_state =
                                guard.get_by_index_reverse(*block_height).map(LedgerSPOState::from);

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
                        Message::Snapshot(SnapshotMessage::Complete) => {
                            info!("Snapshot complete, exiting SPO state bootstrap loop");
                            return;
                        }
                        _ => ()
                    }
                }
            });
        } else {
            info!("Skipping snapshot subscription (startup method is not snapshot)");
        }

        // Subscriptions
        let certificates_subscription = context.subscribe(&certificates_subscribe_topic).await?;
        let block_subscription = context.subscribe(&block_subscribe_topic).await?;
        let epoch_activity_subscription =
            context.subscribe(&epoch_activity_subscribe_topic).await?;
        let spdd_subscription = context.subscribe(&spdd_subscribe_topic).await?;
        let clock_tick_subscription = context.subscribe(&clock_tick_subscribe_topic).await?;
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
        // when state_addresses are enabled
        let stake_reward_deltas_subscription = if store_config.store_stake_addresses {
            Some(context.subscribe(&stake_reward_deltas_subscribe_topic).await?)
        } else {
            None
        };

        // Publishers
        let spo_state_publisher = SPOStatePublisher::new(context.clone(), spo_state_publish_topic);
        let context_copy = context.clone();

        context.run(async move {
            Self::run(
                history,
                epochs_history,
                retired_pools_history,
                context_copy,
                &store_config,
                certificates_subscription,
                block_subscription,
                withdrawals_subscription,
                governance_subscription,
                epoch_activity_subscription,
                spdd_subscription,
                stake_deltas_subscription,
                spo_rewards_subscription,
                stake_reward_deltas_subscription,
                spo_state_publisher,
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
