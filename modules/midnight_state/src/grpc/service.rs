use std::sync::Arc;

use crate::{
    grpc::midnight_state_proto::{
        self, midnight_state_server::MidnightState, AriadneParametersRequest,
        AriadneParametersResponse, AssetCreatesRequest, AssetCreatesResponse, AssetSpendsRequest,
        AssetSpendsResponse, CouncilDatumRequest, CouncilDatumResponse, DeregistrationsRequest,
        DeregistrationsResponse, RegistrationsRequest, RegistrationsResponse,
        TechnicalCommitteeDatumRequest, TechnicalCommitteeDatumResponse,
    },
    state::State,
};
use acropolis_common::{state_history::StateHistory, Datum};
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

pub struct MidnightStateService {
    history: Arc<Mutex<StateHistory<State>>>,
}

impl MidnightStateService {
    pub fn new(history: Arc<Mutex<StateHistory<State>>>) -> Self {
        Self { history }
    }
}

fn datum_to_proto(datum: Datum) -> midnight_state_proto::Datum {
    let value = match datum {
        Datum::Inline(bytes) => midnight_state_proto::datum::Value::Inline(bytes),
        Datum::Hash(hash) => midnight_state_proto::datum::Value::Hash(hash.to_vec()),
    };

    midnight_state_proto::Datum { value: Some(value) }
}

#[tonic::async_trait]
impl MidnightState for MidnightStateService {
    async fn get_asset_creates(
        &self,
        request: Request<AssetCreatesRequest>,
    ) -> Result<Response<AssetCreatesResponse>, Status> {
        let req = request.into_inner();
        if req.start_block > req.end_block {
            return Err(Status::invalid_argument("start_block must be <= end_block"));
        }

        // TODO: Add additional request parameter constraints:
        // 1. end_block <= tip
        // 2. (end_block - start_block) < some_max_blocks

        let creates = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state
                .utxos
                .get_asset_creates(req.start_block, req.end_block)
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
                    output_index: c.utxo_index as u32,
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
        if req.start_block > req.end_block {
            return Err(Status::invalid_argument("start_block must be <= end_block"));
        }

        // TODO: Add additional request parameter constraints:
        // 1. end_block <= tip
        // 2. (end_block - start_block) < some_max_blocks

        let spends = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state
                .utxos
                .get_asset_spends(req.start_block, req.end_block)
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
                    utxo_index: c.utxo_index as u32,
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
        _request: Request<RegistrationsRequest>,
    ) -> Result<Response<RegistrationsResponse>, Status> {
        Ok(Response::new(RegistrationsResponse {}))
    }

    async fn get_deregistrations(
        &self,
        _request: Request<DeregistrationsRequest>,
    ) -> Result<Response<DeregistrationsResponse>, Status> {
        Ok(Response::new(DeregistrationsResponse {}))
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

        Ok(Response::new(TechnicalCommitteeDatumResponse {
            source_block_number,
            datum: Some(datum_to_proto(datum)),
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

        Ok(Response::new(CouncilDatumResponse {
            source_block_number,
            datum: Some(datum_to_proto(datum)),
        }))
    }

    async fn get_ariadne_parameters(
        &self,
        _request: Request<AriadneParametersRequest>,
    ) -> Result<Response<AriadneParametersResponse>, Status> {
        Ok(Response::new(AriadneParametersResponse {}))
    }
}
