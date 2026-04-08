use std::sync::{atomic::Ordering, Arc};

use crate::{
    grpc::{
        midnight_state_proto::{
            midnight_state_server::MidnightState, utxo_event, AriadneParametersRequest,
            AriadneParametersResponse, AssetCreatesRequest, AssetCreatesResponse,
            AssetSpendsRequest, AssetSpendsResponse, Block, BlockByHashRequest,
            BlockByHashResponse, CardanoPosition, CouncilDatumRequest, CouncilDatumResponse,
            DeregistrationsRequest, DeregistrationsResponse, EpochCandidatesRequest,
            EpochCandidatesResponse, EpochNonceRequest, EpochNonceResponse, LatestBlockRequest,
            LatestBlockResponse, LatestStableBlockRequest, LatestStableBlockResponse,
            RegistrationsRequest, RegistrationsResponse, StableBlockRequest, StableBlockResponse,
            StakePoolEntry, TechnicalCommitteeDatumRequest, TechnicalCommitteeDatumResponse,
            UtxoEvent, UtxoEventsRequest, UtxoEventsResponse,
        },
        stats::{RequestStats, RequestStatsSnapshot},
        utxo_events::truncate_by_legacy_tx_capacity,
    },
    state::{StableBlockWindowBounds, State},
};
use acropolis_common::{
    messages::{Message, StateQuery, StateQueryResponse},
    queries::{
        blocks::{
            BlockInfo, BlocksStateQuery, BlocksStateQueryResponse, DEFAULT_BLOCKS_QUERY_TOPIC,
        },
        errors::QueryError,
        spdd::{SPDDStateQuery, SPDDStateQueryResponse},
        utils::query_state,
    },
    state_history::StateHistory,
    BlockHash,
};
use caryatid_sdk::Context;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

const MAX_EVENTS_PER_TX: usize = 64;

#[derive(Clone)]
pub struct MidnightStateService {
    history: Arc<Mutex<StateHistory<State>>>,
    context: Arc<Context<Message>>,
    stats: Arc<RequestStats>,
}

impl MidnightStateService {
    pub fn new(history: Arc<Mutex<StateHistory<State>>>, context: Arc<Context<Message>>) -> Self {
        Self {
            history,
            context,
            stats: Arc::new(RequestStats::default()),
        }
    }

    pub fn stats(&self) -> Option<RequestStatsSnapshot> {
        self.stats.snapshot()
    }
}

#[tonic::async_trait]
impl MidnightState for MidnightStateService {
    async fn get_asset_creates(
        &self,
        request: Request<AssetCreatesRequest>,
    ) -> Result<Response<AssetCreatesResponse>, Status> {
        let req = request.into_inner();

        let utxo_capacity = usize::try_from(req.utxo_capacity)
            .map_err(|_| Status::invalid_argument("utxo_capacity too large"))?;

        // TODO: Add additional request parameter constraints:
        // 1. start_block <= tip
        // 2. utxo_capacity <= MAX_CAPACITY

        let creates = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state
                .utxos
                .get_asset_creates(req.start_block.into(), req.start_tx_index, utxo_capacity)
                .map_err(|e| Status::internal(e.to_string()))?
        };

        let proto_creates = creates.into_iter().map(Into::into).collect();

        Ok(Response::new(AssetCreatesResponse {
            creates: proto_creates,
        }))
    }

    async fn get_asset_spends(
        &self,
        request: Request<AssetSpendsRequest>,
    ) -> Result<Response<AssetSpendsResponse>, Status> {
        let req = request.into_inner();

        let utxo_capacity = usize::try_from(req.utxo_capacity)
            .map_err(|_| Status::invalid_argument("utxo_capacity too large"))?;

        // TODO: Add additional request parameter constraints:
        // 1. start_block <= tip
        // 2. utxo_capacity <= MAX_CAPACITY

        let spends = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state
                .utxos
                .get_asset_spends(req.start_block.into(), req.start_tx_index, utxo_capacity)
                .map_err(|e| Status::internal(e.to_string()))?
        };

        let proto_spends = spends.into_iter().map(Into::into).collect();

        Ok(Response::new(AssetSpendsResponse {
            spends: proto_spends,
        }))
    }

    async fn get_registrations(
        &self,
        request: Request<RegistrationsRequest>,
    ) -> Result<Response<RegistrationsResponse>, Status> {
        let req = request.into_inner();

        let utxo_capacity = usize::try_from(req.utxo_capacity)
            .map_err(|_| Status::invalid_argument("utxo_capacity too large"))?;

        // TODO: Add additional request parameter constraints:
        // 1. start_block <= tip
        // 2. utxo_capacity <= MAX_CAPACITY

        let registrations = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.mapping_registrations.get_registrations(
                req.start_block.into(),
                req.start_tx_index,
                utxo_capacity,
            )
        };

        let proto_registrations = registrations.into_iter().map(Into::into).collect();

        Ok(Response::new(RegistrationsResponse {
            registrations: proto_registrations,
        }))
    }

    async fn get_deregistrations(
        &self,
        request: Request<DeregistrationsRequest>,
    ) -> Result<Response<DeregistrationsResponse>, Status> {
        let req = request.into_inner();

        let utxo_capacity = usize::try_from(req.utxo_capacity)
            .map_err(|_| Status::invalid_argument("utxo_capacity too large"))?;

        // TODO: Add additional request parameter constraints:
        // 1. start_block <= tip
        // 2. utxo_capacity <= MAX_CAPACITY

        let deregistrations = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.mapping_registrations.get_deregistrations(
                req.start_block.into(),
                req.start_tx_index,
                utxo_capacity,
            )
        };

        let proto_deregistrations = deregistrations.into_iter().map(Into::into).collect();

        Ok(Response::new(DeregistrationsResponse {
            deregistrations: proto_deregistrations,
        }))
    }

    async fn get_utxo_events(
        &self,
        request: Request<UtxoEventsRequest>,
    ) -> Result<Response<UtxoEventsResponse>, Status> {
        self.stats.utxo_events.fetch_add(1, Ordering::Relaxed);
        let req = request.into_inner();

        let start_position =
            req.start_position.ok_or_else(|| Status::invalid_argument("missing start_position"))?;
        let start_block = start_position.block_number;
        let start_tx_index = start_position.tx_index;

        let tx_capacity = usize::try_from(req.tx_capacity)
            .map_err(|_| Status::invalid_argument("tx_capacity too large"))?;

        // TODO: Add additional request parameter constraints:
        // 1. start_block <= tip
        // 2. tx_capacity <= MAX_CAPACITY

        let event_capacity = tx_capacity.saturating_mul(MAX_EVENTS_PER_TX);

        let mut events = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            let mut events = Vec::with_capacity(event_capacity);

            events.extend(
                state
                    .utxos
                    .get_asset_creates(start_block.into(), start_tx_index, event_capacity)
                    .map_err(|e| Status::internal(e.to_string()))?
                    .into_iter()
                    .map(|e| UtxoEvent {
                        kind: Some(utxo_event::Kind::AssetCreate(e.into())),
                    }),
            );

            events.extend(
                state
                    .utxos
                    .get_asset_spends(start_block.into(), start_tx_index, event_capacity)
                    .map_err(|e| Status::internal(e.to_string()))?
                    .into_iter()
                    .map(|e| UtxoEvent {
                        kind: Some(utxo_event::Kind::AssetSpend(e.into())),
                    }),
            );

            events.extend(
                state
                    .mapping_registrations
                    .get_registrations(start_block.into(), start_tx_index, event_capacity)
                    .into_iter()
                    .map(|e| UtxoEvent {
                        kind: Some(utxo_event::Kind::Registration(e.into())),
                    }),
            );

            events.extend(
                state
                    .mapping_registrations
                    .get_deregistrations(start_block.into(), start_tx_index, event_capacity)
                    .into_iter()
                    .map(|e| UtxoEvent {
                        kind: Some(utxo_event::Kind::Deregistration(e.into())),
                    }),
            );

            events
        };

        let end_block_hash = BlockHash::try_from(req.end_block_hash)
            .map_err(|_| Status::invalid_argument("invalid end block hash"))?;
        let end_block = query_block_by_hash_info(&self.context, end_block_hash).await?;
        let end_position = (end_block.number, end_block.tx_count as u32);
        events.retain(|event| {
            let position = event.position();
            position.0 < end_position.0
                || (position.0 == end_position.0 && position.1 < end_position.1)
        });

        let truncated = truncate_by_legacy_tx_capacity(events, tx_capacity);
        let next_position = if truncated.num_txs < tx_capacity {
            CardanoPosition {
                block_hash: end_block.hash.to_vec(),
                block_number: u32::try_from(end_block.number)
                    .map_err(|_| Status::internal("block number exceeds u32"))?,
                tx_index: u32::try_from(end_block.tx_count)
                    .map_err(|_| Status::internal("tx count exceeds u32"))?
                    .saturating_add(1),
                block_timestamp_unix_millis: i64::try_from(
                    end_block.timestamp.saturating_mul(1000),
                )
                .map_err(|_| Status::internal("block timestamp exceeds i64"))?,
            }
        } else {
            truncated.events.last().map_or_else(
                || {
                    let mut next_position = start_position.clone();
                    next_position.tx_index = next_position.tx_index.saturating_add(1);
                    next_position
                },
                UtxoEvent::incremented_position,
            )
        };

        Ok(Response::new(UtxoEventsResponse {
            events: truncated.events,
            next_position: Some(next_position),
        }))
    }

    async fn get_technical_committee_datum(
        &self,
        request: Request<TechnicalCommitteeDatumRequest>,
    ) -> Result<Response<TechnicalCommitteeDatumResponse>, Status> {
        self.stats.technical_committee_datum.fetch_add(1, Ordering::Relaxed);
        let req = request.into_inner();

        let technical_committee = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.get_technical_committee_datum(req.block_number)
        };

        let (source_block_number, datum) = technical_committee.ok_or_else(|| {
            Status::not_found(format!(
                "no technical committee datum found at or before block {}",
                req.block_number
            ))
        })?;
        let datum = datum
            .to_bytes()
            .ok_or_else(|| Status::failed_precondition("only inline datums are supported"))?;

        Ok(Response::new(TechnicalCommitteeDatumResponse {
            source_block_number,
            datum,
        }))
    }

    async fn get_council_datum(
        &self,
        request: Request<CouncilDatumRequest>,
    ) -> Result<Response<CouncilDatumResponse>, Status> {
        self.stats.council_datum.fetch_add(1, Ordering::Relaxed);
        let req = request.into_inner();

        let council = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.get_council_datum(req.block_number)
        };

        let (source_block_number, datum) = council.ok_or_else(|| {
            Status::not_found(format!(
                "no council datum found at or before block {}",
                req.block_number
            ))
        })?;
        let datum = datum
            .to_bytes()
            .ok_or_else(|| Status::failed_precondition("only inline datums are supported"))?;

        Ok(Response::new(CouncilDatumResponse {
            source_block_number,
            datum,
        }))
    }

    async fn get_ariadne_parameters(
        &self,
        request: Request<AriadneParametersRequest>,
    ) -> Result<Response<AriadneParametersResponse>, Status> {
        self.stats.ariadne_parameters.fetch_add(1, Ordering::Relaxed);
        let req = request.into_inner();

        let params = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.get_ariadne_parameters_with_epoch(req.epoch)
        };

        let (source_epoch, datum) = params.ok_or_else(|| {
            Status::not_found(format!(
                "no ariadne parameters found at or before epoch {}",
                req.epoch
            ))
        })?;
        let datum = datum
            .to_bytes()
            .ok_or_else(|| Status::failed_precondition("only inline datums are supported"))?;

        Ok(Response::new(AriadneParametersResponse {
            source_epoch,
            datum,
        }))
    }

    async fn get_block_by_hash(
        &self,
        request: Request<BlockByHashRequest>,
    ) -> Result<Response<BlockByHashResponse>, Status> {
        self.stats.block_by_hash.fetch_add(1, Ordering::Relaxed);
        let req = request.into_inner();
        let block_hash = BlockHash::try_from(req.block_hash)
            .map_err(|_| Status::invalid_argument("invalid block hash"))?;

        let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
            BlocksStateQuery::GetBlockByHash { block_hash },
        )));

        let block_info =
            query_state(
                &self.context,
                "cardano.query.blocks",
                msg,
                |message| match message {
                    Message::StateQueryResponse(StateQueryResponse::Blocks(
                        BlocksStateQueryResponse::BlockByHash(block_info),
                    )) => Ok(block_info),
                    Message::StateQueryResponse(StateQueryResponse::Blocks(
                        BlocksStateQueryResponse::Error(e),
                    )) => Err(e),
                    _ => Err(QueryError::internal_error(
                        "Unexpected message type while retrieving block info",
                    )),
                },
            )
            .await
            .map_err(|e| Status::internal(e.to_string()))?;

        Ok(Response::new(BlockByHashResponse {
            block_number: u32::try_from(block_info.number)
                .map_err(|_| Status::internal("block number overflow"))?,
            block_timestamp_unix: i64::try_from(block_info.timestamp)
                .map_err(|_| Status::internal("timestamp overflow"))?,
            tx_count: u32::try_from(block_info.tx_count)
                .map_err(|_| Status::internal("tx count overflow"))?,
            epoch_number: u32::try_from(block_info.epoch)
                .map_err(|_| Status::internal("epoch overflow"))?,
            slot_number: block_info.slot,
        }))
    }

    async fn get_epoch_nonce(
        &self,
        request: Request<EpochNonceRequest>,
    ) -> Result<Response<EpochNonceResponse>, Status> {
        self.stats.epoch_nonce.fetch_add(1, Ordering::Relaxed);
        let req = request.into_inner();

        let nonce_opt = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.get_epoch_nonce(req.epoch)
        };

        Ok(Response::new(EpochNonceResponse { nonce: nonce_opt }))
    }

    async fn get_epoch_candidates(
        &self,
        request: Request<EpochCandidatesRequest>,
    ) -> Result<Response<EpochCandidatesResponse>, Status> {
        self.stats.epoch_candidates.fetch_add(1, Ordering::Relaxed);
        let req = request.into_inner();

        let candidates = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.get_epoch_candidates(req.epoch)
        };

        let msg = Arc::new(Message::StateQuery(StateQuery::SPDD(
            SPDDStateQuery::GetEpochSPDD { epoch: req.epoch },
        )));

        let spdd = query_state(
            &self.context,
            "cardano.query.spdd",
            msg,
            |message| match message {
                Message::StateQueryResponse(StateQueryResponse::SPDD(
                    SPDDStateQueryResponse::EpochSPDD(block_info),
                )) => Ok(block_info),
                Message::StateQueryResponse(StateQueryResponse::SPDD(
                    SPDDStateQueryResponse::Error(e),
                )) => Err(e),
                _ => Err(QueryError::internal_error(
                    "Unexpected message type while retrieving SPDD",
                )),
            },
        )
        .await
        .map_err(|e| Status::internal(e.to_string()))?;

        let stake_distribution = spdd
            .into_iter()
            .map(|(pool_id, stake)| StakePoolEntry {
                pool_hash: pool_id.to_vec(),
                stake,
            })
            .collect();

        Ok(Response::new(EpochCandidatesResponse {
            candidates,
            stake_distribution,
        }))
    }

    async fn get_stable_block(
        &self,
        request: Request<StableBlockRequest>,
    ) -> Result<Response<StableBlockResponse>, Status> {
        self.stats.stable_block_by_hash.fetch_add(1, Ordering::Relaxed);
        let req = request.into_inner();
        let as_of_timestamp_unix_millis = req.as_of_timestamp_unix_millis;
        if as_of_timestamp_unix_millis == 0 {
            return Err(Status::invalid_argument(
                "as_of_timestamp_unix_millis must be set",
            ));
        }
        let block_hash = BlockHash::try_from(req.block_hash)
            .map_err(|_| Status::invalid_argument("invalid block hash"))?;
        let window =
            stable_block_window_from_state(&self.history, as_of_timestamp_unix_millis).await?;

        let block_info_opt = query_stable_block_by_hash_as_of(
            &self.context,
            block_hash,
            req.stability_offset,
            &window,
        )
        .await?;

        let block_proto = block_info_opt.map(Block::try_from).transpose()?;

        Ok(Response::new(StableBlockResponse { block: block_proto }))
    }

    async fn get_latest_stable_block(
        &self,
        request: Request<LatestStableBlockRequest>,
    ) -> Result<Response<LatestStableBlockResponse>, Status> {
        self.stats.latest_stable_block.fetch_add(1, Ordering::Relaxed);
        let req = request.into_inner();
        let as_of_timestamp_unix_millis = req.as_of_timestamp_unix_millis;
        if as_of_timestamp_unix_millis == 0 {
            return Err(Status::invalid_argument(
                "as_of_timestamp_unix_millis must be set",
            ));
        }
        let window =
            stable_block_window_from_state(&self.history, as_of_timestamp_unix_millis).await?;

        let block_info_opt =
            query_latest_stable_block_as_of(&self.context, req.stability_offset, &window).await?;

        let block_proto = block_info_opt.map(Block::try_from).transpose()?;

        Ok(Response::new(LatestStableBlockResponse {
            block: block_proto,
        }))
    }

    async fn get_latest_block(
        &self,
        _request: Request<LatestBlockRequest>,
    ) -> Result<Response<LatestBlockResponse>, Status> {
        self.stats.latest_block.fetch_add(1, Ordering::Relaxed);

        let block_info = query_latest_block(&self.context)
            .await
            .map_err(|e| Status::internal(e.to_string()))?
            .ok_or_else(|| Status::not_found("No blocks available"))?;

        let block_proto = Block::try_from(block_info)?;

        Ok(Response::new(LatestBlockResponse {
            block: Some(block_proto),
        }))
    }
}

async fn query_latest_block(
    context: &Arc<Context<Message>>,
) -> Result<Option<BlockInfo>, QueryError> {
    let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetLatestBlock,
    )));

    query_state(
        context,
        DEFAULT_BLOCKS_QUERY_TOPIC.1,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::LatestBlock(block_info),
            )) => Ok(Some(block_info)),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(QueryError::NotFound { .. }),
            )) => Ok(None),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving latest block",
            )),
        },
    )
    .await
}

async fn query_block_by_hash_info(
    context: &Arc<Context<Message>>,
    block_hash: BlockHash,
) -> Result<BlockInfo, Status> {
    let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetBlockByHash { block_hash },
    )));

    query_state(
        context,
        DEFAULT_BLOCKS_QUERY_TOPIC.1,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::BlockByHash(block_info),
            )) => Ok(block_info),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving block info",
            )),
        },
    )
    .await
    .map_err(|e| Status::internal(e.to_string()))
}

async fn stable_block_window_from_state(
    history: &Arc<Mutex<StateHistory<State>>>,
    as_of_timestamp_unix_millis: u64,
) -> Result<StableBlockWindow, Status> {
    let history = history.lock().await;
    let state = history.current().ok_or_else(|| Status::internal("state not initialized"))?;
    let bounds = state.stable_block_window_bounds().ok_or_else(|| {
        Status::failed_precondition("stable block window bounds are not initialized")
    })?;

    Ok(stable_block_window(bounds, as_of_timestamp_unix_millis))
}

async fn query_latest_stable_block_as_of(
    context: &Arc<Context<Message>>,
    stability_offset: u32,
    window: &StableBlockWindow,
) -> Result<Option<BlockInfo>, Status> {
    let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetLatestStableBlockAsOf {
            stability_offset,
            min_block_timestamp_unix_millis: window.min_block_timestamp_unix_millis,
            max_block_timestamp_unix_millis: window.max_block_timestamp_unix_millis,
        },
    )));

    query_state(
        context,
        DEFAULT_BLOCKS_QUERY_TOPIC.1,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::LatestStableBlockAsOf(block_info),
            )) => Ok(block_info),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving latest stable block",
            )),
        },
    )
    .await
    .map_err(|e| Status::internal(e.to_string()))
}

async fn query_stable_block_by_hash_as_of(
    context: &Arc<Context<Message>>,
    block_hash: BlockHash,
    stability_offset: u32,
    window: &StableBlockWindow,
) -> Result<Option<BlockInfo>, Status> {
    let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
        BlocksStateQuery::GetStableBlockByHashAsOf {
            block_hash,
            stability_offset,
            min_block_timestamp_unix_millis: window.min_block_timestamp_unix_millis,
            max_block_timestamp_unix_millis: window.max_block_timestamp_unix_millis,
        },
    )));

    query_state(
        context,
        DEFAULT_BLOCKS_QUERY_TOPIC.1,
        msg,
        |message| match message {
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::StableBlockByHashAsOf(block_info),
            )) => Ok(block_info),
            Message::StateQueryResponse(StateQueryResponse::Blocks(
                BlocksStateQueryResponse::Error(e),
            )) => Err(e),
            _ => Err(QueryError::internal_error(
                "Unexpected message type while retrieving stable block by hash",
            )),
        },
    )
    .await
    .map_err(|e| Status::internal(e.to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StableBlockWindow {
    min_block_timestamp_unix_millis: u64,
    max_block_timestamp_unix_millis: u64,
}

#[allow(clippy::result_large_err)]
fn stable_block_window(
    bounds: StableBlockWindowBounds,
    as_of_timestamp_unix_millis: u64,
) -> StableBlockWindow {
    StableBlockWindow {
        min_block_timestamp_unix_millis: as_of_timestamp_unix_millis
            .saturating_sub(bounds.max_block_age_millis),
        max_block_timestamp_unix_millis: as_of_timestamp_unix_millis
            .saturating_sub(bounds.min_block_age_millis),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{Arc, Mutex as StdMutex},
        time::Duration,
    };

    use acropolis_common::{
        Address, AssetName, BlockHash, BlockInfo, BlockIntent, BlockStatus, CreatedUTxOExtended, Datum, DatumHash, Era, ExtendedAddressDelta, KeyHash, NetworkId, PolicyId, ShelleyAddress, ShelleyAddressDelegationPart, ShelleyAddressPaymentPart, SpentUTxOExtended, StakeAddress, StakeCredential, TxHash, TxIdentifier, UTxOIdentifier, ValueMap, messages::{AddressDeltasMessage, Message, StateQuery, StateQueryResponse}, protocol_params::{ProtocolParams, ShelleyParams}, queries::{blocks::{
            BlockInfo as QueryBlockInfo, BlocksStateQuery, BlocksStateQueryResponse,
        }, spdd::{SPDDStateQuery, SPDDStateQueryResponse}}, rational_number::RationalNumber, state_history::{StateHistory, StateHistoryStore, StoreType}
    };
    use caryatid_sdk::{async_trait, Context, MessageBus, Subscription};
    use config::Config;
    use tokio::sync::Mutex;
    use tonic::{Code, Request};

    use crate::{
        configuration::MidnightConfig,
        grpc::midnight_state_proto::{
            utxo_event, AriadneParametersRequest, AssetCreatesRequest, AssetSpendsRequest,
            CardanoPosition, EpochCandidatesRequest, LatestStableBlockRequest, StableBlockRequest,
            UtxoEventsRequest,
        },
        state::State,
    };

    use super::{MidnightState, MidnightStateService};

    pub struct DummyBus;

    #[derive(Clone)]
    pub struct StableQueryBus {
        requests: Arc<StdMutex<Vec<BlocksStateQuery>>>,
    }

    impl StableQueryBus {
        fn new(requests: Arc<StdMutex<Vec<BlocksStateQuery>>>) -> Self {
            Self { requests }
        }
    }

    #[async_trait]
    impl MessageBus<Message> for DummyBus {
        async fn publish(&self, _topic: &str, _message: Arc<Message>) -> anyhow::Result<()> {
            Ok(())
        }

        fn request_timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        async fn request(
            &self,
            _topic: &str,
            message: Arc<Message>,
        ) -> anyhow::Result<Arc<Message>> {
            let message = Arc::try_unwrap(message).unwrap_or_else(|arc| (*arc).clone());

            let response = match message {
                Message::StateQuery(StateQuery::SPDD(SPDDStateQuery::GetEpochSPDD { .. })) => {
                    Message::StateQueryResponse(StateQueryResponse::SPDD(
                        SPDDStateQueryResponse::EpochSPDD(Vec::new()),
                    ))
                }
                Message::StateQuery(StateQuery::Blocks(BlocksStateQuery::GetLatestBlock)) => {
                    Message::StateQueryResponse(StateQueryResponse::Blocks(
                        BlocksStateQueryResponse::LatestBlock(query_block_info(100, 8)),
                    ))
                }
                Message::StateQuery(StateQuery::Blocks(BlocksStateQuery::GetBlockByHash {
                    block_hash,
                })) => Message::StateQueryResponse(StateQueryResponse::Blocks(
                    BlocksStateQueryResponse::BlockByHash(query_block_info_with_hash(
                        100, 10, 5, block_hash,
                    )),
                )),
                _ => return Err(anyhow::anyhow!("unsupported request in tests")),
            };

            Ok(Arc::new(response))
        }

        async fn subscribe(&self, _topic: &str) -> anyhow::Result<Box<dyn Subscription<Message>>> {
            Err(anyhow::anyhow!("subscriptions not supported in tests"))
        }

        async fn shutdown(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[async_trait]
    impl MessageBus<Message> for StableQueryBus {
        async fn publish(&self, _topic: &str, _message: Arc<Message>) -> anyhow::Result<()> {
            Ok(())
        }

        fn request_timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        async fn request(
            &self,
            _topic: &str,
            message: Arc<Message>,
        ) -> anyhow::Result<Arc<Message>> {
            let message = Arc::try_unwrap(message).unwrap_or_else(|arc| (*arc).clone());

            let response = match message {
                Message::StateQuery(StateQuery::Blocks(
                    query @ BlocksStateQuery::GetLatestStableBlockAsOf { .. },
                )) => {
                    self.requests.lock().unwrap().push(query);
                    Message::StateQueryResponse(StateQueryResponse::Blocks(
                        BlocksStateQueryResponse::LatestStableBlockAsOf(Some(query_block_info(
                            100, 8,
                        ))),
                    ))
                }
                Message::StateQuery(StateQuery::Blocks(
                    query @ BlocksStateQuery::GetStableBlockByHashAsOf { block_hash, .. },
                )) => {
                    self.requests.lock().unwrap().push(query);
                    Message::StateQueryResponse(StateQueryResponse::Blocks(
                        BlocksStateQueryResponse::StableBlockByHashAsOf(Some(
                            query_block_info_with_hash(95, 9, 0, block_hash),
                        )),
                    ))
                }
                _ => return Err(anyhow::anyhow!("unsupported request in stable query tests")),
            };

            Ok(Arc::new(response))
        }

        async fn subscribe(&self, _topic: &str) -> anyhow::Result<Box<dyn Subscription<Message>>> {
            Err(anyhow::anyhow!("subscriptions not supported in tests"))
        }

        async fn shutdown(&self) -> anyhow::Result<()> {
            Ok(())
        }
    }
    fn test_block_info(number: u64, epoch: u64) -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Volatile,
            intent: BlockIntent::Apply,
            slot: number,
            number,
            hash: BlockHash::default(),
            epoch,
            epoch_slot: number,
            new_epoch: false,
            is_new_era: false,
            tip_slot: Some(number),
            timestamp: 0,
            era: Era::Conway,
        }
    }

    fn test_value_with_token(policy: PolicyId, asset: AssetName, amount: u64) -> ValueMap {
        let mut policy_assets = HashMap::new();
        policy_assets.insert(asset, amount);

        let mut assets = HashMap::new();
        assets.insert(policy, policy_assets);

        ValueMap {
            lovelace: 0,
            assets,
        }
    }

    fn key_hash(byte: u8) -> KeyHash {
        [byte; 28].into()
    }

    fn supported_owner_holder_address() -> Address {
        Address::Shelley(ShelleyAddress {
            network: NetworkId::Testnet,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(key_hash(1)),
            delegation: ShelleyAddressDelegationPart::StakeKeyHash(key_hash(2)),
        })
    }

    fn unsupported_owner_holder_address() -> Address {
        Address::Shelley(ShelleyAddress {
            network: NetworkId::Testnet,
            payment: ShelleyAddressPaymentPart::PaymentKeyHash(key_hash(3)),
            delegation: ShelleyAddressDelegationPart::None,
        })
    }

    fn expected_owner_address() -> Vec<u8> {
        StakeAddress::new(
            StakeCredential::AddrKeyHash(key_hash(2)),
            NetworkId::Testnet,
        )
        .to_bytes_key()
    }

    fn mapping_validator_address() -> Address {
        Address::Shelley(ShelleyAddress {
            network: NetworkId::Testnet,
            payment: ShelleyAddressPaymentPart::ScriptHash(key_hash(4)),
            delegation: ShelleyAddressDelegationPart::None,
        })
    }

    fn committee_candidate_address() -> Address {
        Address::Shelley(ShelleyAddress {
            network: NetworkId::Testnet,
            payment: ShelleyAddressPaymentPart::ScriptHash(key_hash(5)),
            delegation: ShelleyAddressDelegationPart::None,
        })
    }

    fn cnight_delta(
        address: Address,
        policy: PolicyId,
        asset: AssetName,
        tx_hash: TxHash,
        output_index: u16,
        tx_index: u16,
        block_number: u64,
    ) -> ExtendedAddressDelta {
        let value = test_value_with_token(policy, asset, 1);

        ExtendedAddressDelta {
            address,
            tx_identifier: TxIdentifier::new(block_number as u32, tx_index),
            spent_utxos: vec![],
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(tx_hash, output_index),
                value: value.clone(),
                datum: None,
            }],
            received: value,
            sent: ValueMap::default(),
        }
    }

    fn candidate_registration_delta(
        address: Address,
        policy: PolicyId,
        asset: AssetName,
        tx_hash: TxHash,
        output_index: u16,
        tx_index: u16,
        block_number: u64,
    ) -> ExtendedAddressDelta {
        let value = test_value_with_token(policy, asset, 1);

        ExtendedAddressDelta {
            address,
            tx_identifier: TxIdentifier::new(block_number as u32, tx_index),
            spent_utxos: vec![],
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(tx_hash, output_index),
                value: value.clone(),
                datum: Some(Datum::Inline(vec![0xAA])),
            }],
            received: value,
            sent: ValueMap::default(),
        }
    }

    fn test_parameter_delta(
        policy: PolicyId,
        asset: AssetName,
        datum: Datum,
        output_index: u16,
    ) -> ExtendedAddressDelta {
        let value = test_value_with_token(policy, asset, 1);
        ExtendedAddressDelta {
            address: Address::default(),
            tx_identifier: TxIdentifier::default(),
            spent_utxos: vec![],
            created_utxos: vec![CreatedUTxOExtended {
                utxo: UTxOIdentifier::new(TxHash::new([1u8; 32]), output_index),
                value: value.clone(),
                datum: Some(datum),
            }],
            sent: ValueMap::default(),
            received: value,
        }
    }

    fn service_with_committed_state_and_bus<B>(
        state: State,
        block_number: u64,
        bus: B,
    ) -> MidnightStateService
    where
        B: MessageBus<Message> + 'static,
    {
        let mut history = StateHistory::new(
            "midnight-state",
            StateHistoryStore::Unbounded,
            &Config::default(),
            StoreType::Block,
        );
        history.commit(block_number, state);

        let (_, startup_watch) = tokio::sync::watch::channel(true);
        MidnightStateService::new(
            Arc::new(Mutex::new(history)),
            Arc::new(Context::new(
                Arc::new(Config::default()),
                Arc::new(bus),
                startup_watch,
            )),
        )
    }

    fn service_with_committed_state(state: State, block_number: u64) -> MidnightStateService {
        service_with_committed_state_and_bus(state, block_number, DummyBus)
    }

    #[tokio::test]
    async fn should_return_parameters_and_source_epoch_when_epoch_has_prior_parameters() {
        let parameter_policy = PolicyId::new([9u8; 28]);
        let parameter_asset = AssetName::new(b"params").expect("params asset name");
        let config = MidnightConfig {
            permissioned_candidate_policy: parameter_policy,
            ..Default::default()
        };

        let mut state = State::new(config);
        let source_epoch = 4;
        let source_block = test_block_info(1, source_epoch);
        let expected_datum = vec![0xAA, 0xBB, 0xCC];
        let delta = test_parameter_delta(
            parameter_policy,
            parameter_asset,
            Datum::Inline(expected_datum.clone()),
            0,
        );
        state
            .handle_address_deltas(
                &source_block,
                &AddressDeltasMessage::ExtendedDeltas(vec![delta]),
            )
            .expect("address delta handling should succeed");

        let service = service_with_committed_state(state, source_block.number);
        let response = service
            .get_ariadne_parameters(Request::new(AriadneParametersRequest {
                epoch: source_epoch + 3,
            }))
            .await
            .expect("ariadne parameters should be found")
            .into_inner();

        assert_eq!(response.source_epoch, source_epoch);
        assert_eq!(response.datum, expected_datum);
    }

    #[tokio::test]
    async fn should_return_not_found_when_no_parameters_exist_for_requested_epoch() {
        let service = service_with_committed_state(State::new(MidnightConfig::default()), 1);
        let result = service
            .get_ariadne_parameters(Request::new(AriadneParametersRequest { epoch: 42 }))
            .await;

        let err = result.expect_err("missing parameters should return an error");
        assert_eq!(err.code(), Code::NotFound);
    }

    #[tokio::test]
    async fn should_return_failed_precondition_when_latest_parameters_datum_is_hash() {
        let parameter_policy = PolicyId::new([9u8; 28]);
        let parameter_asset = AssetName::new(b"params").expect("params asset name");
        let config = MidnightConfig {
            permissioned_candidate_policy: parameter_policy,
            ..Default::default()
        };

        let mut state = State::new(config);
        let source_block = test_block_info(2, 7);
        let delta = test_parameter_delta(
            parameter_policy,
            parameter_asset,
            Datum::Hash(DatumHash::new([3u8; 32])),
            1,
        );
        state
            .handle_address_deltas(
                &source_block,
                &AddressDeltasMessage::ExtendedDeltas(vec![delta]),
            )
            .expect("address delta handling should succeed");

        let service = service_with_committed_state(state, source_block.number);
        let result = service
            .get_ariadne_parameters(Request::new(AriadneParametersRequest {
                epoch: source_block.epoch,
            }))
            .await;

        let err = result.expect_err("hash datum should be rejected");
        assert_eq!(err.code(), Code::FailedPrecondition);
    }

    #[tokio::test]
    async fn should_return_epoch_candidates_from_committee_candidate_address() {
        let committee_address = committee_candidate_address();
        let mapping_address = mapping_validator_address();
        let auth_policy = PolicyId::new([9u8; 28]);
        let auth_asset = AssetName::new(b"auth").unwrap();
        let block = test_block_info(3, 7);
        let mut state = State::new(MidnightConfig {
            committee_candidate_address: committee_address.clone(),
            mapping_validator_address: mapping_address.clone(),
            auth_token_policy_id: auth_policy,
            auth_token_asset_name: auth_asset,
            ..Default::default()
        });

        state
            .handle_address_deltas(
                &block,
                &AddressDeltasMessage::ExtendedDeltas(vec![
                    ExtendedAddressDelta {
                        address: committee_address,
                        tx_identifier: TxIdentifier::new(block.number as u32, 1),
                        spent_utxos: vec![],
                        created_utxos: vec![CreatedUTxOExtended {
                            utxo: UTxOIdentifier::new(TxHash::new([1u8; 32]), 0),
                            value: ValueMap::default(),
                            datum: Some(Datum::Inline(vec![0xCA, 0xFE])),
                        }],
                        sent: ValueMap::default(),
                        received: ValueMap::default(),
                    },
                    candidate_registration_delta(
                        mapping_address,
                        auth_policy,
                        auth_asset,
                        TxHash::new([2u8; 32]),
                        1,
                        2,
                        block.number,
                    ),
                ]),
            )
            .expect("address delta handling should succeed");
        state.handle_new_epoch(&block, None);

        let service = service_with_committed_state(state, block.number);
        let response = service
            .get_epoch_candidates(Request::new(EpochCandidatesRequest { epoch: block.epoch }))
            .await
            .expect("epoch candidates should be returned")
            .into_inner();

        assert_eq!(response.candidates.len(), 1);
        assert_eq!(response.candidates[0].full_datum, vec![0xCA, 0xFE]);
        assert_eq!(response.candidates[0].utxo_tx_hash, vec![1u8; 32]);
        assert_eq!(response.candidates[0].utxo_index, 0);
    }

    #[tokio::test]
    async fn should_skip_asset_creates_with_unsupported_owner_addresses() {
        let cnight_policy = PolicyId::new([1u8; 28]);
        let cnight_asset = AssetName::new(b"cnight").unwrap();
        let block = test_block_info(1, 1);
        let mut state = State::new(MidnightConfig {
            cnight_policy_id: cnight_policy,
            cnight_asset_name: cnight_asset,
            ..Default::default()
        });

        state
            .handle_address_deltas(
                &block,
                &AddressDeltasMessage::ExtendedDeltas(vec![
                    cnight_delta(
                        supported_owner_holder_address(),
                        cnight_policy,
                        cnight_asset,
                        TxHash::new([1u8; 32]),
                        0,
                        1,
                        block.number,
                    ),
                    cnight_delta(
                        unsupported_owner_holder_address(),
                        cnight_policy,
                        cnight_asset,
                        TxHash::new([2u8; 32]),
                        1,
                        2,
                        block.number,
                    ),
                ]),
            )
            .expect("address delta handling should succeed");

        let service = service_with_committed_state(state, block.number);
        let response = service
            .get_asset_creates(Request::new(AssetCreatesRequest {
                start_block: 0,
                start_tx_index: 0,
                utxo_capacity: 10,
            }))
            .await
            .expect("asset creates should be returned")
            .into_inner();

        assert_eq!(response.creates.len(), 1);
        assert_eq!(response.creates[0].address, expected_owner_address());
    }

    #[tokio::test]
    async fn should_skip_asset_spends_with_unsupported_owner_addresses() {
        let cnight_policy = PolicyId::new([1u8; 28]);
        let cnight_asset = AssetName::new(b"cnight").unwrap();
        let mut state = State::new(MidnightConfig {
            cnight_policy_id: cnight_policy,
            cnight_asset_name: cnight_asset,
            ..Default::default()
        });
        let create_block = test_block_info(1, 1);
        let spend_block = test_block_info(2, 1);

        let supported_utxo = UTxOIdentifier::new(TxHash::new([1u8; 32]), 0);
        let unsupported_utxo = UTxOIdentifier::new(TxHash::new([2u8; 32]), 1);

        state
            .handle_address_deltas(
                &create_block,
                &AddressDeltasMessage::ExtendedDeltas(vec![
                    cnight_delta(
                        supported_owner_holder_address(),
                        cnight_policy,
                        cnight_asset,
                        supported_utxo.tx_hash,
                        supported_utxo.output_index,
                        1,
                        create_block.number,
                    ),
                    cnight_delta(
                        unsupported_owner_holder_address(),
                        cnight_policy,
                        cnight_asset,
                        unsupported_utxo.tx_hash,
                        unsupported_utxo.output_index,
                        2,
                        create_block.number,
                    ),
                ]),
            )
            .expect("create delta handling should succeed");

        state
            .handle_address_deltas(
                &spend_block,
                &AddressDeltasMessage::ExtendedDeltas(vec![ExtendedAddressDelta {
                    address: Address::default(),
                    tx_identifier: TxIdentifier::new(spend_block.number as u32, 4),
                    spent_utxos: vec![
                        SpentUTxOExtended {
                            spent_by: TxHash::new([3u8; 32]),
                            utxo: supported_utxo,
                        },
                        SpentUTxOExtended {
                            spent_by: TxHash::new([4u8; 32]),
                            utxo: unsupported_utxo,
                        },
                    ],
                    created_utxos: vec![],
                    received: ValueMap::default(),
                    sent: ValueMap::default(),
                }]),
            )
            .expect("spend delta handling should succeed");

        let service = service_with_committed_state(state, spend_block.number);
        let response = service
            .get_asset_spends(Request::new(AssetSpendsRequest {
                start_block: 0,
                start_tx_index: 0,
                utxo_capacity: 10,
            }))
            .await
            .expect("asset spends should be returned")
            .into_inner();

        assert_eq!(response.spends.len(), 1);
        assert_eq!(response.spends[0].address, expected_owner_address());
    }

    #[tokio::test]
    async fn should_keep_non_cnight_events_when_skipping_unsupported_utxo_events() {
        let cnight_policy = PolicyId::new([1u8; 28]);
        let cnight_asset = AssetName::new(b"cnight").unwrap();
        let auth_policy = PolicyId::new([9u8; 28]);
        let auth_asset = AssetName::new(b"auth").unwrap();
        let mapping_address = mapping_validator_address();
        let block = test_block_info(1, 1);
        let mut state = State::new(MidnightConfig {
            cnight_policy_id: cnight_policy,
            cnight_asset_name: cnight_asset,
            mapping_validator_address: mapping_address.clone(),
            auth_token_policy_id: auth_policy,
            auth_token_asset_name: auth_asset,
            ..Default::default()
        });

        state
            .handle_address_deltas(
                &block,
                &AddressDeltasMessage::ExtendedDeltas(vec![
                    cnight_delta(
                        supported_owner_holder_address(),
                        cnight_policy,
                        cnight_asset,
                        TxHash::new([1u8; 32]),
                        0,
                        1,
                        block.number,
                    ),
                    cnight_delta(
                        unsupported_owner_holder_address(),
                        cnight_policy,
                        cnight_asset,
                        TxHash::new([2u8; 32]),
                        1,
                        2,
                        block.number,
                    ),
                    candidate_registration_delta(
                        mapping_address,
                        auth_policy,
                        auth_asset,
                        TxHash::new([3u8; 32]),
                        2,
                        3,
                        block.number,
                    ),
                ]),
            )
            .expect("address delta handling should succeed");

        let service = service_with_committed_state(state, block.number);
        let response = service
            .get_utxo_events(Request::new(UtxoEventsRequest {
                tx_capacity: 10,
                end_block_hash: [9u8; 32].to_vec(),
                start_position: Some(CardanoPosition {
                    block_hash: vec![0u8; 32],
                    block_number: 0,
                    tx_index: 0,
                    block_timestamp_unix_millis: 0,
                }),
            }))
            .await
            .expect("utxo events should be returned")
            .into_inner();

        assert_eq!(response.events.len(), 2);
        let next_position = response
            .next_position
            .expect("next position should always be returned when end_block_hash is supplied");
        assert_eq!(next_position.block_hash, vec![9u8; 32]);
        assert_eq!(next_position.block_number, 100);
        assert_eq!(next_position.tx_index, 6);
        assert_eq!(next_position.block_timestamp_unix_millis, 10_000);

        let mut asset_creates = response.events.iter().filter_map(|event| match &event.kind {
            Some(utxo_event::Kind::AssetCreate(create)) => Some(create),
            _ => None,
        });
        let registrations = response
            .events
            .iter()
            .filter(|event| matches!(event.kind, Some(utxo_event::Kind::Registration(_))))
            .count();

        let asset_create = asset_creates.next().expect("supported cNIGHT create should remain");
        assert!(asset_creates.next().is_none());
        assert_eq!(asset_create.address, expected_owner_address());
        assert_eq!(registrations, 1);
    }

    #[tokio::test]
    async fn should_return_incremented_start_position_for_empty_legacy_truncation() {
        let auth_policy = PolicyId::new([9u8; 28]);
        let auth_asset = AssetName::new(b"auth").unwrap();
        let mapping_address = mapping_validator_address();
        let mut block = test_block_info(100, 0);
        block.hash = [9u8; 32].into();
        block.timestamp = 10;
        let mut state = State::new(MidnightConfig {
            mapping_validator_address: mapping_address.clone(),
            auth_token_policy_id: auth_policy,
            auth_token_asset_name: auth_asset,
            ..Default::default()
        });
        let start_position = CardanoPosition {
            block_hash: vec![7u8; 32],
            block_number: 55,
            tx_index: 0,
            block_timestamp_unix_millis: 12_345,
        };

        state
            .handle_address_deltas(
                &block,
                &AddressDeltasMessage::ExtendedDeltas(vec![candidate_registration_delta(
                    mapping_address,
                    auth_policy,
                    auth_asset,
                    TxHash::new([3u8; 32]),
                    0,
                    0,
                    block.number,
                )]),
            )
            .expect("address delta handling should succeed");

        let service = service_with_committed_state(state, block.number);
        let response = service
            .get_utxo_events(Request::new(UtxoEventsRequest {
                tx_capacity: 1,
                end_block_hash: [9u8; 32].to_vec(),
                start_position: Some(start_position.clone()),
            }))
            .await
            .expect("utxo events should be returned")
            .into_inner();

        assert!(response.events.is_empty());
        let next_position = response
            .next_position
            .expect("next position should be returned from start_position fallback");
        assert_eq!(next_position.block_hash, start_position.block_hash);
        assert_eq!(next_position.block_number, start_position.block_number);
        assert_eq!(next_position.tx_index, start_position.tx_index + 1);
        assert_eq!(
            next_position.block_timestamp_unix_millis,
            start_position.block_timestamp_unix_millis
        );
    }

    #[tokio::test]
    async fn should_return_latest_stable_block_using_bounds_from_state() {
        let protocol_params = ProtocolParams {
            shelley: Some(ShelleyParams {
                security_param: 1,
                active_slots_coeff: RationalNumber::new(1, 1),
                slot_length: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut state = State::new(MidnightConfig::default());
        state
            .update_stable_block_window_bounds(&protocol_params)
            .expect("stable bounds should be derived");
        let bounds = state.stable_block_window_bounds().expect("stable bounds should be stored");
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let service =
            service_with_committed_state_and_bus(state, 100, StableQueryBus::new(requests.clone()));
        let response = service
            .get_latest_stable_block(Request::new(LatestStableBlockRequest {
                stability_offset: 5,
                as_of_timestamp_unix_millis: 10_000,
            }))
            .await
            .expect("latest stable block should be returned")
            .into_inner();

        let block = response
            .block
            .expect("block should be present when stable boundary falls within cached window");
        assert_eq!(block.block_number, 100);
        assert_eq!(block.block_timestamp_unix, 8);

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        match &requests[0] {
            BlocksStateQuery::GetLatestStableBlockAsOf {
                stability_offset: 5,
                min_block_timestamp_unix_millis,
                max_block_timestamp_unix_millis,
            } => {
                assert_eq!(
                    *min_block_timestamp_unix_millis,
                    10_000_u64.saturating_sub(bounds.max_block_age_millis)
                );
                assert_eq!(
                    *max_block_timestamp_unix_millis,
                    10_000_u64.saturating_sub(bounds.min_block_age_millis)
                );
            }
            other => panic!("unexpected query: {other:?}"),
        }
    }

    #[tokio::test]
    async fn should_return_stable_block_using_bounds_from_state() {
        let protocol_params = ProtocolParams {
            shelley: Some(ShelleyParams {
                security_param: 1,
                active_slots_coeff: RationalNumber::new(1, 1),
                slot_length: 1,
                ..Default::default()
            }),
            ..Default::default()
        };
        let mut state = State::new(MidnightConfig::default());
        state
            .update_stable_block_window_bounds(&protocol_params)
            .expect("stable bounds should be derived");
        let bounds = state.stable_block_window_bounds().expect("stable bounds should be stored");
        let requests = Arc::new(StdMutex::new(Vec::new()));
        let service =
            service_with_committed_state_and_bus(state, 100, StableQueryBus::new(requests.clone()));
        let response = service
            .get_stable_block(Request::new(StableBlockRequest {
                block_hash: vec![7u8; 32],
                stability_offset: 5,
                as_of_timestamp_unix_millis: 10_000,
            }))
            .await
            .expect("stable block should be returned")
            .into_inner();

        let block = response
            .block
            .expect("block should be present when stable block matches the query window");
        assert_eq!(block.block_number, 95);
        assert_eq!(block.block_hash, vec![7u8; 32]);
        assert_eq!(block.block_timestamp_unix, 9);

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        match &requests[0] {
            BlocksStateQuery::GetStableBlockByHashAsOf {
                block_hash,
                stability_offset: 5,
                min_block_timestamp_unix_millis,
                max_block_timestamp_unix_millis,
            } => {
                let expected_block_hash = BlockHash::try_from(vec![7u8; 32]).unwrap();
                assert_eq!(*block_hash, expected_block_hash);
                assert_eq!(
                    *min_block_timestamp_unix_millis,
                    10_000_u64.saturating_sub(bounds.max_block_age_millis)
                );
                assert_eq!(
                    *max_block_timestamp_unix_millis,
                    10_000_u64.saturating_sub(bounds.min_block_age_millis)
                );
            }
            other => panic!("unexpected query: {other:?}"),
        }
    }

    fn query_block_info(number: u64, timestamp: u64) -> QueryBlockInfo {
        QueryBlockInfo {
            timestamp,
            number,
            hash: BlockHash::default(),
            slot: number,
            epoch: 0,
            epoch_slot: number,
            issuer: None,
            size: 0,
            tx_count: 0,
            output: None,
            fees: None,
            block_vrf: None,
            op_cert: None,
            op_cert_counter: None,
            previous_block: None,
            next_block: None,
            confirmations: 0,
        }
    }

    fn query_block_info_with_hash(
        number: u64,
        timestamp: u64,
        tx_count: u64,
        hash: BlockHash,
    ) -> QueryBlockInfo {
        QueryBlockInfo {
            hash,
            tx_count,
            ..query_block_info(number, timestamp)
        }
    }

    #[test]
    fn stable_block_window_should_match_expected_bounds() {
        let window = super::stable_block_window(
            super::StableBlockWindowBounds {
                min_block_age_millis: 2_000,
                max_block_age_millis: 5_000,
            },
            10_000,
        );

        assert_eq!(window.min_block_timestamp_unix_millis, 5_000);
        assert_eq!(window.max_block_timestamp_unix_millis, 8_000);
    }
}
