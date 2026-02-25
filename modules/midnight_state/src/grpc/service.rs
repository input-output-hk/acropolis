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
use acropolis_common::state_history::StateHistory;
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
        if req.start_block > req.end_block {
            return Err(Status::invalid_argument("start_block must be <= end_block"));
        }

        // TODO: Add additional request parameter constraints:
        // 1. end_block <= tip
        // 2. (end_block - start_block) < some_max_blocks

        let registrations = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.candidates.get_registrations(req.start_block, req.end_block)
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
        if req.start_block > req.end_block {
            return Err(Status::invalid_argument("start_block must be <= end_block"));
        }

        // TODO: Add additional request parameter constraints:
        // 1. end_block <= tip
        // 2. (end_block - start_block) < some_max_blocks

        let deregistrations = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.candidates.get_deregistrations(req.start_block, req.end_block)
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
        _request: Request<TechnicalCommitteeDatumRequest>,
    ) -> Result<Response<TechnicalCommitteeDatumResponse>, Status> {
        Ok(Response::new(TechnicalCommitteeDatumResponse {}))
    }

    async fn get_council_datum(
        &self,
        _request: Request<CouncilDatumRequest>,
    ) -> Result<Response<CouncilDatumResponse>, Status> {
        Ok(Response::new(CouncilDatumResponse {}))
    }

    async fn get_ariadne_parameters(
        &self,
        _request: Request<AriadneParametersRequest>,
    ) -> Result<Response<AriadneParametersResponse>, Status> {
        Ok(Response::new(AriadneParametersResponse {}))
    }
}
