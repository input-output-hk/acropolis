use std::sync::Arc;

use crate::{
    grpc::conversions::{try_asset_create_proto, try_asset_spend_proto, CNightOwnerAddressError},
    grpc::{
        midnight_state_proto::{
            midnight_state_server::MidnightState, utxo_event, AriadneParametersRequest,
            AriadneParametersResponse, AssetCreate as AssetCreateProto, AssetCreatesRequest,
            AssetCreatesResponse, AssetSpend as AssetSpendProto, AssetSpendsRequest,
            AssetSpendsResponse, BlockByHashRequest, BlockByHashResponse, CouncilDatumRequest,
            CouncilDatumResponse, DeregistrationsRequest, DeregistrationsResponse,
            EpochCandidatesRequest, EpochCandidatesResponse, EpochNonceRequest, EpochNonceResponse,
            RegistrationsRequest, RegistrationsResponse, StakePoolEntry,
            TechnicalCommitteeDatumRequest, TechnicalCommitteeDatumResponse, UtxoEvent,
            UtxoEventsRequest, UtxoEventsResponse,
        },
        utxo_events::truncate_by_tx_capacity,
    },
    state::State,
    types::{AssetCreate as CNightAssetCreate, AssetSpend as CNightAssetSpend},
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
    BlockHash,
};
use caryatid_sdk::Context;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};
use tracing::warn;

const MAX_EVENTS_PER_TX: usize = 64;

fn convert_asset_creates(creates: Vec<CNightAssetCreate>) -> Vec<AssetCreateProto> {
    creates
        .into_iter()
        .filter_map(|create| match try_asset_create_proto(&create) {
            Ok(proto) => Some(proto),
            Err(err) => {
                warn_skipped_asset_create(&create, &err);
                None
            }
        })
        .collect()
}

fn convert_asset_spends(spends: Vec<CNightAssetSpend>) -> Vec<AssetSpendProto> {
    spends
        .into_iter()
        .filter_map(|spend| match try_asset_spend_proto(&spend) {
            Ok(proto) => Some(proto),
            Err(err) => {
                warn_skipped_asset_spend(&spend, &err);
                None
            }
        })
        .collect()
}

fn warn_skipped_asset_create(create: &CNightAssetCreate, err: &CNightOwnerAddressError) {
    warn!(
        block_number = create.block_number,
        tx_hash = %create.tx_hash,
        utxo_index = create.utxo_index,
        address_kind = create.holder_address.kind(),
        reason = %err,
        "skipping cNIGHT asset create with unsupported owner address"
    );
}

fn warn_skipped_asset_spend(spend: &CNightAssetSpend, err: &CNightOwnerAddressError) {
    warn!(
        block_number = spend.block_number,
        tx_hash = %spend.spending_tx_hash,
        utxo_tx_hash = %spend.utxo_tx_hash,
        utxo_index = spend.utxo_index,
        address_kind = spend.holder_address.kind(),
        reason = %err,
        "skipping cNIGHT asset spend with unsupported owner address"
    );
}

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

        let proto_creates = convert_asset_creates(creates);

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

        let proto_spends = convert_asset_spends(spends);

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
                    .filter_map(|e| {
                        let proto = match try_asset_create_proto(&e) {
                            Ok(proto) => proto,
                            Err(err) => {
                                warn_skipped_asset_create(&e, &err);
                                return None;
                            }
                        };

                        Some(UtxoEvent {
                            kind: Some(utxo_event::Kind::AssetCreate(proto)),
                        })
                    }),
            );

            events.extend(
                state
                    .utxos
                    .get_asset_spends(start_block.into(), start_tx_index, event_capacity)
                    .map_err(|e| Status::internal(e.to_string()))?
                    .into_iter()
                    .filter_map(|e| {
                        let proto = match try_asset_spend_proto(&e) {
                            Ok(proto) => proto,
                            Err(err) => {
                                warn_skipped_asset_spend(&e, &err);
                                return None;
                            }
                        };

                        Some(UtxoEvent {
                            kind: Some(utxo_event::Kind::AssetSpend(proto)),
                        })
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
            block_number: u32::try_from(block_info.number)
                .map_err(|_| Status::internal("block number overflow"))?,
            block_timestamp_unix: i64::try_from(block_info.timestamp)
                .map_err(|_| Status::internal("timestamp overflow"))?,
            tx_count: u32::try_from(block_info.tx_count)
                .map_err(|_| Status::internal("tx count overflow"))?,
        }))
    }

    async fn get_epoch_nonce(
        &self,
        request: Request<EpochNonceRequest>,
    ) -> Result<Response<EpochNonceResponse>, Status> {
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
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc, time::Duration};

    use acropolis_common::{
        messages::AddressDeltasMessage,
        state_history::{StateHistory, StateHistoryStore},
        Address, AssetName, BlockHash, BlockInfo, BlockIntent, BlockStatus, CreatedUTxOExtended,
        Datum, DatumHash, Era, ExtendedAddressDelta, KeyHash, NetworkId, PolicyId, ShelleyAddress,
        ShelleyAddressDelegationPart, ShelleyAddressPaymentPart, SpentUTxOExtended, StakeAddress,
        StakeCredential, TxHash, TxIdentifier, UTxOIdentifier, ValueMap,
    };
    use caryatid_sdk::{async_trait, Context, MessageBounds, MessageBus, Subscription};
    use config::Config;
    use tokio::sync::Mutex;
    use tonic::{Code, Request};

    use crate::{
        configuration::MidnightConfig,
        grpc::midnight_state_proto::{
            utxo_event, AriadneParametersRequest, AssetCreatesRequest, AssetSpendsRequest,
            UtxoEventsRequest,
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
                start_block: 0,
                start_tx_index: 0,
                tx_capacity: 10,
            }))
            .await
            .expect("utxo events should be returned")
            .into_inner();

        assert_eq!(response.events.len(), 2);

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
}
