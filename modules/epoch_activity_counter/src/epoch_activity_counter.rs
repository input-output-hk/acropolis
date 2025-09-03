//! Acropolis epoch activity counter module for Caryatid
//! Unpacks block bodies to get transaction fees

use acropolis_common::{
    messages::{CardanoMessage, Message, StateQuery, StateQueryResponse},
    queries::epochs::{
        BlocksMintedByPools, EpochsStateQuery, EpochsStateQueryResponse, LatestEpoch,
        TotalBlocksMintedByPools, DEFAULT_EPOCHS_QUERY_TOPIC,
    },
    rest_helper::{handle_rest, handle_rest_with_path_parameter},
    state_history::{StateHistory, StateHistoryStore},
    BlockInfo, BlockStatus, Era,
};
use anyhow::Result;
use caryatid_sdk::{message_bus::Subscription, module, Context, Module};
use config::Config;
use pallas::ledger::traverse::MultiEraHeader;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, info_span, Instrument};

mod epoch_activity_publisher;
mod epochs_history;
mod state;
mod store_config;
use state::State;
mod rest;
use rest::{handle_epoch, handle_historical_epoch};

use crate::{
    epoch_activity_publisher::EpochActivityPublisher, epochs_history::EpochsHistoryState,
    store_config::StoreConfig,
};

const DEFAULT_SUBSCRIBE_HEADERS_TOPIC: &str = "cardano.block.header";
const DEFAULT_SUBSCRIBE_FEES_TOPIC: &str = "cardano.block.fees";
const DEFAULT_PUBLISH_TOPIC: &str = "cardano.epoch.activity";
const DEFAULT_HANDLE_CURRENT_TOPIC: (&str, &str) = ("handle-topic-current-epoch", "rest.get.epoch");
const DEFAULT_HANDLE_HISTORICAL_TOPIC: (&str, &str) =
    ("handle-topic-historical-epoch", "rest.get.epochs.*");

/// Epoch activity counter module
#[module(
    message_type(Message),
    name = "epoch-activity-counter",
    description = "Epoch activity counter"
)]
pub struct EpochActivityCounter;

impl EpochActivityCounter {
    /// Run loop
    async fn run(
        history: Arc<Mutex<StateHistory<State>>>,
        epochs_history: EpochsHistoryState,
        mut headers_subscription: Box<dyn Subscription<Message>>,
        mut fees_subscription: Box<dyn Subscription<Message>>,
        mut epoch_activity_publisher: EpochActivityPublisher,
    ) -> Result<()> {
        loop {
            // Get a mutable state
            let mut state = history.lock().await.get_or_init_with(|| State::new());
            let mut current_block: Option<BlockInfo> = None;

            // Read both topics in parallel
            let headers_message_f = headers_subscription.read();
            let fees_message_f = fees_subscription.read();

            // Handle headers first
            let (_, message) = headers_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::BlockHeader(header_msg))) => {
                    let span = info_span!(
                        "epoch_activity_counter.handle_block_header",
                        block = block_info.number
                    );

                    // handle rollback here
                    if block_info.status == BlockStatus::RolledBack {
                        state = history.lock().await.get_rolled_back_state(block_info.number);
                    }
                    current_block = Some(block_info.clone());
                    let is_new_epoch = block_info.new_epoch && block_info.epoch > 0;

                    async {
                        // End of epoch?
                        if is_new_epoch {
                            let ea = state.end_epoch(&block_info);

                            // update epochs history
                            epochs_history.handle_epoch_activity(&block_info, &ea);

                            // publish epoch activity message
                            epoch_activity_publisher
                                .publish(Arc::new(Message::Cardano((
                                    block_info.clone(),
                                    CardanoMessage::EpochActivity(ea),
                                ))))
                                .await
                                .unwrap_or_else(|e| error!("Failed to publish: {e}"));
                        }

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
                        match MultiEraHeader::decode(variant, None, &header_msg.raw) {
                            Ok(header) => {
                                if let Some(vrf_vkey) = header.vrf_vkey() {
                                    state.handle_mint(&block_info, Some(vrf_vkey));
                                }
                            }

                            Err(e) => error!("Can't decode header {}: {e}", block_info.slot),
                        }
                    }
                    .instrument(span)
                    .await;
                }

                _ => error!("Unexpected message type: {message:?}"),
            }

            // Handle block fees second so new epoch's fees don't get counted in the last one
            let (_, message) = fees_message_f.await?;
            match message.as_ref() {
                Message::Cardano((block_info, CardanoMessage::BlockFees(fees_msg))) => {
                    let span = info_span!(
                        "epoch_activity_counter.handle_block_fees",
                        block = block_info.number
                    );
                    async {
                        Self::check_sync(&current_block, &block_info);
                        state.handle_fees(&block_info, fees_msg.total_fees);
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

    /// Main init function
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Subscription topics
        let subscribe_headers_topic = config
            .get_string("subscribe-headers-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_HEADERS_TOPIC.to_string());
        info!("Creating subscriber for headers on '{subscribe_headers_topic}'");

        let subscribe_fees_topic = config
            .get_string("subscribe-fees-topic")
            .unwrap_or(DEFAULT_SUBSCRIBE_FEES_TOPIC.to_string());
        info!("Creating subscriber for fees on '{subscribe_fees_topic}'");

        // REST handler topics
        let handle_current_topic = config
            .get_string(DEFAULT_HANDLE_CURRENT_TOPIC.0)
            .unwrap_or(DEFAULT_HANDLE_CURRENT_TOPIC.1.to_string());
        info!("Creating request handler on '{}'", handle_current_topic);

        let handle_historical_topic = config
            .get_string(DEFAULT_HANDLE_HISTORICAL_TOPIC.0)
            .unwrap_or(DEFAULT_HANDLE_HISTORICAL_TOPIC.1.to_string());
        info!("Creating request handler on '{}'", handle_historical_topic);

        // Publish topic
        let publish_topic =
            config.get_string("publish-topic").unwrap_or(DEFAULT_PUBLISH_TOPIC.to_string());
        info!("Publishing on '{publish_topic}'");

        // query topic
        let epochs_query_topic = config
            .get_string(DEFAULT_EPOCHS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_EPOCHS_QUERY_TOPIC.1.to_string());
        info!("Creating query handler on '{}'", epochs_query_topic);

        // store config
        let store_config = StoreConfig::from(config.clone());

        // state history
        let history = Arc::new(Mutex::new(StateHistory::<State>::new(
            "epoch_activity_counter",
            StateHistoryStore::default_block_store(),
        )));
        let history_query = history.clone();
        let history_rest = history.clone();

        // epochs history
        let epochs_history = EpochsHistoryState::new(&store_config);
        let epochs_history_rest = epochs_history.clone();

        // Publisher
        let epoch_activity_publisher = EpochActivityPublisher::new(context.clone(), publish_topic);

        // Subscribe
        let headers_subscription = context.subscribe(&subscribe_headers_topic).await?;
        let fees_subscription = context.subscribe(&subscribe_fees_topic).await?;

        // handle epochs query
        context.handle(&epochs_query_topic, move |message| {
            let history = history_query.clone();

            async move {
                let Message::StateQuery(StateQuery::Epochs(query)) = message.as_ref() else {
                    return Arc::new(Message::StateQueryResponse(StateQueryResponse::Epochs(
                        EpochsStateQueryResponse::Error("Invalid message for epochs-state".into()),
                    )));
                };

                let state = history.lock().await.get_current_state();
                let response = match query {
                    EpochsStateQuery::GetLatestEpoch => {
                        EpochsStateQueryResponse::LatestEpoch(LatestEpoch {
                            epoch: state.get_epoch_activity_message(),
                        })
                    }

                    EpochsStateQuery::GetBlocksMintedByPools { vrf_key_hashes } => {
                        EpochsStateQueryResponse::BlocksMintedByPools(BlocksMintedByPools {
                            blocks_minted: state.get_blocks_minted_by_pools(vrf_key_hashes),
                        })
                    }

                    EpochsStateQuery::GetTotalBlocksMintedByPools { vrf_key_hashes } => {
                        EpochsStateQueryResponse::TotalBlocksMintedByPools(
                            TotalBlocksMintedByPools {
                                total_blocks_minted: state
                                    .get_total_blocks_minted_by_pools(vrf_key_hashes),
                            },
                        )
                    }

                    _ => EpochsStateQueryResponse::Error(format!(
                        "Unimplemented query variant: {:?}",
                        query
                    )),
                };
                Arc::new(Message::StateQueryResponse(StateQueryResponse::Epochs(
                    response,
                )))
            }
        });

        handle_rest(context.clone(), &handle_current_topic, {
            let history = history_rest.clone();
            move || handle_epoch(history.clone())
        });

        handle_rest_with_path_parameter(context.clone(), &handle_historical_topic, {
            let epochs_history = epochs_history_rest.clone();
            move |param| handle_historical_epoch(epochs_history.clone(), param[0].to_string())
        });

        // Start run task
        context.run(async move {
            Self::run(
                history,
                epochs_history,
                headers_subscription,
                fees_subscription,
                epoch_activity_publisher,
            )
            .await
            .unwrap_or_else(|e| error!("Failed: {e}"));
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
