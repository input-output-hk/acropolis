use std::sync::{atomic::Ordering, Arc};

use anyhow::anyhow;

use crate::{
    grpc::{
        midnight_state_proto::{
            bridge_checkpoint, bridge_utxos_request, midnight_state_server::MidnightState,
            utxo_event, AriadneParametersRequest, AriadneParametersResponse, AssetCreatesRequest,
            AssetCreatesResponse, AssetSpendsRequest, AssetSpendsResponse, Block,
            BlockByHashRequest, BlockByHashResponse, BridgeCheckpoint as BridgeCheckpointProto,
            BridgeUtxosRequest, BridgeUtxosResponse, CouncilDatumRequest, CouncilDatumResponse,
            DeregistrationsRequest, DeregistrationsResponse, EpochCandidatesRequest,
            EpochCandidatesResponse, EpochNonceRequest, EpochNonceResponse,
            LatestStableBlockRequest, LatestStableBlockResponse, RegistrationsRequest,
            RegistrationsResponse, StableBlockRequest, StableBlockResponse, StakePoolEntry,
            TechnicalCommitteeDatumRequest, TechnicalCommitteeDatumResponse, UtxoEvent,
            UtxoEventsRequest, UtxoEventsResponse,
        },
        stats::{RequestStats, RequestStatsSnapshot},
        utxo_events::truncate_by_tx_capacity,
    },
    indexes::bridge_state::{BridgeCheckpoint, BridgeState, BridgeStateError},
    state::State,
};
use acropolis_common::{
    messages::{Message, StateQuery, StateQueryResponse},
    queries::{
        blocks::{BlocksStateQuery, BlocksStateQueryResponse},
        errors::QueryError,
        spdd::{SPDDStateQuery, SPDDStateQueryResponse},
        utils::query_state,
    },
    state_history::StateHistory,
    BlockHash, TxHash, UTxOIdentifier,
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

    pub fn stats(&self) -> RequestStatsSnapshot {
        self.stats.snapshot()
    }
}

fn bridge_checkpoint_from_proto(
    checkpoint: Option<bridge_utxos_request::Checkpoint>,
) -> anyhow::Result<BridgeCheckpoint> {
    match checkpoint {
        Some(bridge_utxos_request::Checkpoint::BlockNumber(block_number)) => {
            Ok(BridgeCheckpoint::Block(block_number))
        }
        Some(bridge_utxos_request::Checkpoint::Utxo(utxo)) => {
            Ok(BridgeCheckpoint::Utxo(UTxOIdentifier::new(
                TxHash::try_from(utxo.tx_hash)
                    .map_err(|_| anyhow!("invalid bridge checkpoint tx hash"))?,
                u16::try_from(utxo.index)
                    .map_err(|_| anyhow!("invalid bridge checkpoint output index"))?,
            )))
        }
        None => Err(anyhow!("missing bridge checkpoint")),
    }
}

fn bridge_checkpoint_to_proto(checkpoint: BridgeCheckpoint) -> BridgeCheckpointProto {
    let kind = match checkpoint {
        BridgeCheckpoint::Block(block_number) => bridge_checkpoint::Kind::BlockNumber(block_number),
        BridgeCheckpoint::Utxo(utxo) => {
            bridge_checkpoint::Kind::Utxo(crate::grpc::midnight_state_proto::UtxoId {
                tx_hash: utxo.tx_hash.to_vec(),
                index: utxo.output_index.into(),
            })
        }
    };

    BridgeCheckpointProto { kind: Some(kind) }
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

    async fn get_bridge_utxos(
        &self,
        request: Request<BridgeUtxosRequest>,
    ) -> Result<Response<BridgeUtxosResponse>, Status> {
        self.stats.bridge_utxos.fetch_add(1, Ordering::Relaxed);
        let req = request.into_inner();
        if req.utxo_capacity == 0 {
            return Err(Status::invalid_argument(
                "utxo_capacity must be greater than zero",
            ));
        }

        let utxo_capacity = usize::try_from(req.utxo_capacity)
            .map_err(|_| Status::invalid_argument("utxo_capacity too large"))?;
        let checkpoint = bridge_checkpoint_from_proto(req.checkpoint)
            .map_err(|e| Status::invalid_argument(e.to_string()))?;

        let utxos = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.bridge.get_bridge_utxos(checkpoint, req.to_block, utxo_capacity).map_err(
                |err| match err {
                    BridgeStateError::UnknownCheckpointUtxo(_) => {
                        Status::not_found(err.to_string())
                    }
                },
            )?
        };

        let next_checkpoint = bridge_checkpoint_to_proto(BridgeState::next_checkpoint(
            &utxos,
            req.to_block,
            utxo_capacity,
        ));

        Ok(Response::new(BridgeUtxosResponse {
            utxos: utxos.into_iter().map(Into::into).collect(),
            next_checkpoint: Some(next_checkpoint),
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

            state.candidates.get_registrations(
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

            state.candidates.get_deregistrations(
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

        let start_block = req.start_block;
        let start_tx_index = req.start_tx_index;

        let tx_capacity = usize::try_from(req.tx_capacity)
            .map_err(|_| Status::invalid_argument("tx_capacity too large"))?;

        // TODO: Add additional request parameter constraints:
        // 1. start_block <= tip
        // 2. tx_capacity <= MAX_CAPACITY

        let event_capacity = tx_capacity.saturating_mul(MAX_EVENTS_PER_TX);

        let events = {
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
                    .candidates
                    .get_registrations(start_block.into(), start_tx_index, event_capacity)
                    .into_iter()
                    .map(|e| UtxoEvent {
                        kind: Some(utxo_event::Kind::Registration(e.into())),
                    }),
            );

            events.extend(
                state
                    .candidates
                    .get_deregistrations(start_block.into(), start_tx_index, event_capacity)
                    .into_iter()
                    .map(|e| UtxoEvent {
                        kind: Some(utxo_event::Kind::Deregistration(e.into())),
                    }),
            );

            events
        };

        let events = truncate_by_tx_capacity(events, tx_capacity);

        Ok(Response::new(UtxoEventsResponse { events }))
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
        let block_hash = BlockHash::try_from(req.block_hash)
            .map_err(|_| Status::invalid_argument("invalid block hash"))?;

        let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
            BlocksStateQuery::GetStableBlockByHash {
                block_hash,
                offset: req.offset,
            },
        )));

        let block_info_opt =
            query_state(
                &self.context,
                "cardano.query.blocks",
                msg,
                |message| match message {
                    Message::StateQueryResponse(StateQueryResponse::Blocks(
                        BlocksStateQueryResponse::StableBlockByHash(block_info),
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

        let block_proto = block_info_opt
            .map(|b| {
                Ok::<Block, Status>(Block {
                    block_number: u32::try_from(b.number)
                        .map_err(|_| Status::internal("block number overflow"))?,
                    block_hash: b.hash.to_vec(),
                    epoch_number: u32::try_from(b.epoch)
                        .map_err(|_| Status::internal("epoch overflow"))?,
                    slot_number: b.slot,
                    block_timestamp_unix: b.timestamp,
                })
            })
            .transpose()?;

        Ok(Response::new(StableBlockResponse { block: block_proto }))
    }

    async fn get_latest_stable_block(
        &self,
        request: Request<LatestStableBlockRequest>,
    ) -> Result<Response<LatestStableBlockResponse>, Status> {
        self.stats.latest_stable_block.fetch_add(1, Ordering::Relaxed);
        let req = request.into_inner();

        let msg = Arc::new(Message::StateQuery(StateQuery::Blocks(
            BlocksStateQuery::GetBlockByTipOffset { offset: req.offset },
        )));

        let block_info_opt =
            query_state(
                &self.context,
                "cardano.query.blocks",
                msg,
                |message| match message {
                    Message::StateQueryResponse(StateQueryResponse::Blocks(
                        BlocksStateQueryResponse::BlockByTipOffset(block_info),
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

        let block_proto = block_info_opt
            .map(|b| {
                Ok::<Block, Status>(Block {
                    block_number: u32::try_from(b.number)
                        .map_err(|_| Status::internal("block number overflow"))?,
                    block_hash: b.hash.to_vec(),
                    epoch_number: u32::try_from(b.epoch)
                        .map_err(|_| Status::internal("epoch overflow"))?,
                    slot_number: b.slot,
                    block_timestamp_unix: b.timestamp,
                })
            })
            .transpose()?;

        Ok(Response::new(LatestStableBlockResponse {
            block: block_proto,
        }))
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc, time::Duration};

    use acropolis_common::{
        messages::AddressDeltasMessage,
        state_history::{StateHistory, StateHistoryStore},
        Address, AssetName, BlockHash, BlockInfo, BlockIntent, BlockStatus, CreatedUTxOExtended,
        Datum, DatumHash, Era, ExtendedAddressDelta, PolicyId, SpentUTxOExtended, TxHash,
        TxIdentifier, UTxOIdentifier, ValueMap,
    };
    use caryatid_sdk::{async_trait, Context, MessageBounds, MessageBus, Subscription};
    use config::Config;
    use tokio::sync::Mutex;
    use tonic::{Code, Request};

    use crate::{
        configuration::MidnightConfig,
        grpc::midnight_state_proto::{
            bridge_checkpoint, bridge_utxos_request, AriadneParametersRequest, BridgeUtxosRequest,
            UtxoId,
        },
        state::State,
    };

    use super::{MidnightState, MidnightStateService};

    pub struct DummyBus;

    #[async_trait]
    impl<M: MessageBounds> MessageBus<M> for DummyBus {
        async fn publish(&self, _topic: &str, _message: Arc<M>) -> anyhow::Result<()> {
            Ok(())
        }

        fn request_timeout(&self) -> Duration {
            Duration::from_secs(1)
        }

        async fn subscribe(&self, _topic: &str) -> anyhow::Result<Box<dyn Subscription<M>>> {
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

    fn test_bridge_config(address: Address, policy: PolicyId, asset: AssetName) -> MidnightConfig {
        MidnightConfig {
            illiquid_circulation_supply_validator_address: address,
            bridge_token_policy_id: policy,
            bridge_token_asset_name: asset,
            ..Default::default()
        }
    }

    fn test_bridge_delta(
        address: Address,
        tx_index: u16,
        created_utxos: Vec<CreatedUTxOExtended>,
        spent_utxos: Vec<SpentUTxOExtended>,
    ) -> ExtendedAddressDelta {
        let mut received_assets: HashMap<PolicyId, HashMap<AssetName, u64>> = HashMap::new();
        for created in &created_utxos {
            for (policy, policy_assets) in &created.value.assets {
                let entry = received_assets.entry(*policy).or_default();
                for (asset_name, amount) in policy_assets {
                    *entry.entry(*asset_name).or_default() += *amount;
                }
            }
        }

        ExtendedAddressDelta {
            address,
            tx_identifier: TxIdentifier::new(1, tx_index),
            spent_utxos,
            created_utxos,
            sent: ValueMap::default(),
            received: ValueMap {
                lovelace: 0,
                assets: received_assets,
            },
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

    fn service_with_committed_state(state: State, block_number: u64) -> MidnightStateService {
        let mut history = StateHistory::new("midnight-state", StateHistoryStore::Unbounded);
        history.commit(block_number, state);

        let (_, startup_watch) = tokio::sync::watch::channel(true);
        MidnightStateService::new(
            Arc::new(Mutex::new(history)),
            Arc::new(Context::new(
                Arc::new(Config::default()),
                Arc::new(DummyBus),
                startup_watch,
            )),
        )
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
    async fn should_return_bridge_utxos_with_last_utxo_checkpoint_when_capacity_is_reached() {
        let bridge_policy = PolicyId::new([1u8; 28]);
        let bridge_asset = AssetName::new(b"").expect("empty asset name");
        let bridge_address =
            Address::from_string("addr_test1wzga9g4tw69twfvpjynyvdyzvpf5f0e88v6hnu9eh9qgdnqaw66xk")
                .expect("bridge address");
        let config = test_bridge_config(bridge_address.clone(), bridge_policy, bridge_asset);

        let mut state = State::new(config);
        let block = test_block_info(10, 1);
        let first = UTxOIdentifier::new(TxHash::new([1u8; 32]), 0);
        let second = UTxOIdentifier::new(TxHash::new([2u8; 32]), 1);

        state
            .handle_address_deltas(
                &block,
                &AddressDeltasMessage::ExtendedDeltas(vec![
                    test_bridge_delta(
                        bridge_address.clone(),
                        0,
                        vec![CreatedUTxOExtended {
                            utxo: first,
                            value: test_value_with_token(bridge_policy, bridge_asset, 10),
                            datum: Some(Datum::Inline(vec![0xAA])),
                        }],
                        vec![],
                    ),
                    test_bridge_delta(
                        bridge_address,
                        1,
                        vec![CreatedUTxOExtended {
                            utxo: second,
                            value: test_value_with_token(bridge_policy, bridge_asset, 20),
                            datum: Some(Datum::Inline(vec![0xBB])),
                        }],
                        vec![],
                    ),
                ]),
            )
            .expect("bridge deltas should be indexed");

        let service = service_with_committed_state(state, block.number);
        let response = service
            .get_bridge_utxos(Request::new(BridgeUtxosRequest {
                checkpoint: Some(bridge_utxos_request::Checkpoint::BlockNumber(0)),
                to_block: block.number,
                utxo_capacity: 2,
            }))
            .await
            .expect("bridge utxos should be returned")
            .into_inner();

        assert_eq!(response.utxos.len(), 2);
        assert_eq!(response.utxos[0].tokens_in, 0);
        assert_eq!(response.utxos[1].tokens_out, 20);
        assert_eq!(
            response
                .next_checkpoint
                .and_then(|checkpoint| checkpoint.kind)
                .expect("expected checkpoint"),
            bridge_checkpoint::Kind::Utxo(UtxoId {
                tx_hash: second.tx_hash.to_vec(),
                index: second.output_index.into(),
            })
        );
    }

    #[tokio::test]
    async fn should_return_block_checkpoint_when_bridge_response_is_under_capacity() {
        let bridge_policy = PolicyId::new([2u8; 28]);
        let bridge_asset = AssetName::new(b"").expect("empty asset name");
        let bridge_address =
            Address::from_string("addr_test1wzga9g4tw69twfvpjynyvdyzvpf5f0e88v6hnu9eh9qgdnqaw66xk")
                .expect("bridge address");
        let config = test_bridge_config(bridge_address.clone(), bridge_policy, bridge_asset);

        let mut state = State::new(config);
        let block = test_block_info(12, 1);
        let utxo = UTxOIdentifier::new(TxHash::new([3u8; 32]), 0);

        state
            .handle_address_deltas(
                &block,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_bridge_delta(
                    bridge_address,
                    0,
                    vec![CreatedUTxOExtended {
                        utxo,
                        value: test_value_with_token(bridge_policy, bridge_asset, 15),
                        datum: Some(Datum::Inline(vec![0xCC])),
                    }],
                    vec![],
                )]),
            )
            .expect("bridge delta should be indexed");

        let service = service_with_committed_state(state, block.number);
        let response = service
            .get_bridge_utxos(Request::new(BridgeUtxosRequest {
                checkpoint: Some(bridge_utxos_request::Checkpoint::BlockNumber(0)),
                to_block: block.number,
                utxo_capacity: 10,
            }))
            .await
            .expect("bridge utxos should be returned")
            .into_inner();

        assert_eq!(response.utxos.len(), 1);
        assert_eq!(
            response
                .next_checkpoint
                .and_then(|checkpoint| checkpoint.kind)
                .expect("expected checkpoint"),
            bridge_checkpoint::Kind::BlockNumber(block.number)
        );
    }

    #[tokio::test]
    async fn should_return_not_found_for_unknown_bridge_checkpoint_utxo() {
        let service = service_with_committed_state(State::new(MidnightConfig::default()), 1);
        let result = service
            .get_bridge_utxos(Request::new(BridgeUtxosRequest {
                checkpoint: Some(bridge_utxos_request::Checkpoint::Utxo(UtxoId {
                    tx_hash: vec![9u8; 32],
                    index: 0,
                })),
                to_block: 10,
                utxo_capacity: 1,
            }))
            .await;

        let err = result.expect_err("unknown checkpoint should fail");
        assert_eq!(err.code(), Code::NotFound);
    }

    #[tokio::test]
    async fn should_validate_bridge_request_and_omit_non_inline_datum() {
        let bridge_policy = PolicyId::new([4u8; 28]);
        let bridge_asset = AssetName::new(b"").expect("empty asset name");
        let bridge_address =
            Address::from_string("addr_test1wzga9g4tw69twfvpjynyvdyzvpf5f0e88v6hnu9eh9qgdnqaw66xk")
                .expect("bridge address");
        let config = test_bridge_config(bridge_address.clone(), bridge_policy, bridge_asset);

        let mut state = State::new(config);
        let block = test_block_info(14, 1);
        let utxo = UTxOIdentifier::new(TxHash::new([4u8; 32]), 0);

        state
            .handle_address_deltas(
                &block,
                &AddressDeltasMessage::ExtendedDeltas(vec![test_bridge_delta(
                    bridge_address,
                    0,
                    vec![CreatedUTxOExtended {
                        utxo,
                        value: test_value_with_token(bridge_policy, bridge_asset, 25),
                        datum: Some(Datum::Hash(DatumHash::new([8u8; 32]))),
                    }],
                    vec![],
                )]),
            )
            .expect("bridge delta should be indexed");

        let service = service_with_committed_state(state, block.number);
        let invalid = service
            .get_bridge_utxos(Request::new(BridgeUtxosRequest {
                checkpoint: None,
                to_block: block.number,
                utxo_capacity: 1,
            }))
            .await
            .expect_err("missing checkpoint should fail");
        assert_eq!(invalid.code(), Code::InvalidArgument);

        let invalid = service
            .get_bridge_utxos(Request::new(BridgeUtxosRequest {
                checkpoint: Some(bridge_utxos_request::Checkpoint::BlockNumber(0)),
                to_block: block.number,
                utxo_capacity: 0,
            }))
            .await
            .expect_err("zero capacity should fail");
        assert_eq!(invalid.code(), Code::InvalidArgument);

        let response = service
            .get_bridge_utxos(Request::new(BridgeUtxosRequest {
                checkpoint: Some(bridge_utxos_request::Checkpoint::BlockNumber(0)),
                to_block: block.number,
                utxo_capacity: 1,
            }))
            .await
            .expect("bridge utxo should be returned")
            .into_inner();

        assert_eq!(response.utxos.len(), 1);
        assert!(response.utxos[0].datum.is_none());
    }
}
