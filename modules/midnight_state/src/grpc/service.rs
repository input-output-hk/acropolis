use std::sync::Arc;

use crate::{
    grpc::midnight_state_proto::{
        midnight_state_server::MidnightState, AriadneParametersRequest, AriadneParametersResponse,
        AssetCreatesRequest, AssetCreatesResponse, AssetSpendsRequest, AssetSpendsResponse,
        CouncilDatumRequest, CouncilDatumResponse, DeregistrationsRequest, DeregistrationsResponse,
        RegistrationsRequest, RegistrationsResponse, TechnicalCommitteeDatumRequest,
        TechnicalCommitteeDatumResponse,
    },
    state::State,
};
use acropolis_common::state_history::StateHistory;
use tokio::sync::Mutex;
use tonic::{Request, Response, Status};

pub struct MidnightStateService {
    _history: Arc<Mutex<StateHistory<State>>>,
}

impl MidnightStateService {
    pub fn new(history: Arc<Mutex<StateHistory<State>>>) -> Self {
        Self { _history: history }
    }
}

#[tonic::async_trait]
impl MidnightState for MidnightStateService {
    async fn get_asset_creates(
        &self,
        _request: Request<AssetCreatesRequest>,
    ) -> Result<Response<AssetCreatesResponse>, Status> {
        Ok(Response::new(AssetCreatesResponse {}))
    }

    async fn get_asset_spends(
        &self,
        _request: Request<AssetSpendsRequest>,
    ) -> Result<Response<AssetSpendsResponse>, Status> {
        Ok(Response::new(AssetSpendsResponse {}))
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
