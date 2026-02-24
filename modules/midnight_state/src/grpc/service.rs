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

        let creates = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state
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

        let spends = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state
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
        request: Request<RegistrationsRequest>,
    ) -> Result<Response<RegistrationsResponse>, Status> {
        let req = request.into_inner();
        if req.start_block > req.end_block {
            return Err(Status::invalid_argument("start_block must be <= end_block"));
        }

        let registrations = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.get_registrations(req.start_block, req.end_block)
        };

        let proto_registrations = registrations
            .into_iter()
            .map(|r| midnight_state_proto::Registration {
                full_datum: Some(datum_to_proto(r.full_datum)),
                block_number: r.block_number,
                block_hash: r.block_hash.to_vec(),
                tx_index: r.tx_index_in_block,
                tx_hash: r.tx_hash.to_vec(),
                utxo_index: r.utxo_index as u32,
                block_timestamp_unix: r.block_timestamp.and_utc().timestamp(),
            })
            .collect();

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

        let deregistrations = {
            let history = self.history.lock().await;
            let state =
                history.current().ok_or_else(|| Status::internal("state not initialized"))?;

            state.get_deregistrations(req.start_block, req.end_block)
        };

        let proto_deregistrations = deregistrations
            .into_iter()
            .map(|r| midnight_state_proto::Deregistration {
                full_datum: Some(datum_to_proto(r.full_datum)),
                block_number: r.block_number,
                block_hash: r.block_hash.to_vec(),
                tx_index: r.tx_index_in_block,
                tx_hash: r.tx_hash.to_vec(),
                utxo_tx_hash: r.utxo_tx_hash.to_vec(),
                utxo_index: r.utxo_index as u32,
                block_timestamp_unix: r.block_timestamp.and_utc().timestamp(),
            })
            .collect();

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

        if let Some((source_block_number, datum)) = technical_committee {
            Ok(Response::new(TechnicalCommitteeDatumResponse {
                found: true,
                source_block_number,
                datum: Some(datum_to_proto(datum)),
            }))
        } else {
            Ok(Response::new(TechnicalCommitteeDatumResponse {
                found: false,
                source_block_number: 0,
                datum: None,
            }))
        }
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

        if let Some((source_block_number, datum)) = council {
            Ok(Response::new(CouncilDatumResponse {
                found: true,
                source_block_number,
                datum: Some(datum_to_proto(datum)),
            }))
        } else {
            Ok(Response::new(CouncilDatumResponse {
                found: false,
                source_block_number: 0,
                datum: None,
            }))
        }
    }

    async fn get_ariadne_parameters(
        &self,
        _request: Request<AriadneParametersRequest>,
    ) -> Result<Response<AriadneParametersResponse>, Status> {
        Ok(Response::new(AriadneParametersResponse {}))
    }
}
