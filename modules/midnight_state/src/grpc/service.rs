use std::sync::Arc;

use crate::{
    grpc::midnight_state_proto::{
        self, midnight_state_server::MidnightState, AriadneParametersRequest,
        AriadneParametersResponse, AssetCreatesRequest, AssetCreatesResponse, AssetSpendsRequest,
        AssetSpendsResponse, BlockByHashRequest, BlockByHashResponse, CouncilDatumRequest,
        CouncilDatumResponse, DeregistrationsRequest, DeregistrationsResponse,
        RegistrationsRequest, RegistrationsResponse, TechnicalCommitteeDatumRequest,
        TechnicalCommitteeDatumResponse,
    },
    state::State,
};
use acropolis_common::{
    messages::{Message, StateQuery, StateQueryResponse},
    queries::{
        blocks::{BlocksStateQuery, BlocksStateQueryResponse},
        errors::QueryError,
        utils::query_state,
    },
    state_history::StateHistory,
    BlockHash,
};
use caryatid_sdk::Context;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

pub struct MidnightStateService {
    history: Arc<Mutex<StateHistory<State>>>,
    context: Arc<Context<Message>>,
}

impl MidnightStateService {
    pub fn new(history: Arc<Mutex<StateHistory<State>>>, context: Arc<Context<Message>>) -> Self {
        Self { history, context }
    }
}

#[tonic::async_trait]
impl MidnightState for MidnightStateService {
    async fn get_asset_creates(
        &self,
        request: Request<AssetCreatesRequest>,
    ) -> Result<Response<AssetCreatesResponse>, Status> {
        let req = request.into_inner();

        // TODO: Add additional request parameter constraints:
        // 1. start_block <= tip
        // 2. utxo_capacity <= MAX_CAPACITY

        let utxo_capacity = usize::try_from(req.utxo_capacity)
            .map_err(|_| Status::invalid_argument("utxo_capacity too large"))?;

        let creates = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state
                .get_asset_creates(req.start_block, req.start_tx_index, utxo_capacity)
                .map_err(|e| Status::internal(e.to_string()))?
        };

        let proto_creates = creates
            .into_iter()
            .map(|c| {
                let address =
                    c.holder_address.to_bytes_key().map_err(|e| Status::internal(e.to_string()))?;

                Ok(midnight_state_proto::AssetCreate {
                    address,
                    quantity: c.quantity,
                    tx_hash: c.tx_hash.to_vec(),
                    output_index: c.utxo_index.into(),
                    block_number: c.block_number,
                    block_hash: c.block_hash.to_vec(),
                    tx_index: c.tx_index_in_block,
                    block_timestamp_unix: c.block_timestamp,
                })
            })
            .collect::<Result<Vec<_>, Status>>()?;

        Ok(Response::new(AssetCreatesResponse {
            creates: proto_creates,
        }))
    }

    async fn get_asset_spends(
        &self,
        request: Request<AssetSpendsRequest>,
    ) -> Result<Response<AssetSpendsResponse>, Status> {
        let req = request.into_inner();

        // TODO: Add additional request parameter constraints:
        // 1. start_block <= tip
        // 2. utxo_capacity <= MAX_CAPACITY

        let utxo_capacity = usize::try_from(req.utxo_capacity)
            .map_err(|_| Status::invalid_argument("utxo_capacity too large"))?;

        let spends = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state
                .get_asset_spends(req.start_block, req.start_tx_index, utxo_capacity)
                .map_err(|e| Status::internal(e.to_string()))?
        };

        let proto_spends = spends
            .into_iter()
            .map(|c| {
                let address =
                    c.holder_address.to_bytes_key().map_err(|e| Status::internal(e.to_string()))?;

                Ok(midnight_state_proto::AssetSpend {
                    address,
                    quantity: c.quantity,
                    spending_tx_hash: c.spending_tx_hash.to_vec(),
                    block_number: c.block_number,
                    block_hash: c.block_hash.to_vec(),
                    tx_index: c.tx_index_in_block,
                    utxo_tx_hash: c.utxo_tx_hash.to_vec(),
                    utxo_index: c.utxo_index.into(),
                    block_timestamp_unix: c.block_timestamp,
                })
            })
            .collect::<Result<Vec<_>, Status>>()?;

        Ok(Response::new(AssetSpendsResponse {
            spends: proto_spends,
        }))
    }

    async fn get_registrations(
        &self,
        request: Request<RegistrationsRequest>,
    ) -> Result<Response<RegistrationsResponse>, Status> {
        let req = request.into_inner();

        // TODO: Add additional request parameter constraints:
        // 1. start_block <= tip
        // 2. utxo_capacity <= MAX_CAPACITY

        let utxo_capacity = usize::try_from(req.utxo_capacity)
            .map_err(|_| Status::invalid_argument("utxo_capacity too large"))?;

        let registrations = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.get_registrations(req.start_block, req.start_tx_index, utxo_capacity)
        };

        let proto_registrations = registrations
            .into_iter()
            .map(|c| {
                let full_datum = c
                    .full_datum
                    .to_bytes()
                    .ok_or_else(|| Status::internal("full_datum is not inline"))?;

                Ok(midnight_state_proto::Registration {
                    full_datum,
                    tx_hash: c.tx_hash.to_vec(),
                    output_index: c.utxo_index.into(),
                    block_number: c.block_number,
                    block_hash: c.block_hash.to_vec(),
                    tx_index: c.tx_index_in_block,
                    block_timestamp_unix: c.block_timestamp,
                })
            })
            .collect::<Result<Vec<_>, Status>>()?;

        Ok(Response::new(RegistrationsResponse {
            registrations: proto_registrations,
        }))
    }

    async fn get_deregistrations(
        &self,
        request: Request<DeregistrationsRequest>,
    ) -> Result<Response<DeregistrationsResponse>, Status> {
        let req = request.into_inner();

        // TODO: Add additional request parameter constraints:
        // 1. start_block <= tip
        // 2. utxo_capacity <= MAX_CAPACITY

        let utxo_capacity = usize::try_from(req.utxo_capacity)
            .map_err(|_| Status::invalid_argument("utxo_capacity too large"))?;

        let deregistrations = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.get_deregistrations(req.start_block, req.start_tx_index, utxo_capacity)
        };

        let proto_deregistrations = deregistrations
            .into_iter()
            .map(|c| {
                let full_datum = c
                    .full_datum
                    .to_bytes()
                    .ok_or_else(|| Status::internal("full_datum is not inline"))?;

                Ok(midnight_state_proto::Deregistration {
                    full_datum,
                    tx_hash: c.tx_hash.to_vec(),
                    block_number: c.block_number,
                    block_hash: c.block_hash.to_vec(),
                    tx_index: c.tx_index_in_block,
                    utxo_tx_hash: c.utxo_tx_hash.to_vec(),
                    utxo_index: c.utxo_index.into(),
                    block_timestamp_unix: c.block_timestamp,
                })
            })
            .collect::<Result<Vec<_>, Status>>()?;
        Ok(Response::new(DeregistrationsResponse {
            deregistrations: proto_deregistrations,
        }))
    }

    async fn get_technical_committee_datum(
        &self,
        request: Request<TechnicalCommitteeDatumRequest>,
    ) -> Result<Response<TechnicalCommitteeDatumResponse>, Status> {
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
            block_number: block_info.number,
            block_timestamp_unix: i64::try_from(block_info.timestamp)
                .map_err(|_| Status::internal("timestamp overflow"))?,
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
        Datum, DatumHash, Era, ExtendedAddressDelta, PolicyId, TxHash, TxIdentifier,
        UTxOIdentifier, ValueMap,
    };
    use caryatid_sdk::{async_trait, Context, MessageBounds, MessageBus, Subscription};
    use config::Config;
    use tokio::sync::Mutex;
    use tonic::{Code, Request};

    use crate::{
        configuration::MidnightConfig, grpc::midnight_state_proto::AriadneParametersRequest,
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
}
