use std::{net::SocketAddr, sync::Arc};

use acropolis_common::state_history::StateHistory;
use anyhow::Result;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tonic::transport::Server;

use crate::grpc::midnight_state_proto::midnight_state_server::MidnightStateServer;
use crate::grpc::midnight_state_proto::FILE_DESCRIPTOR_SET;
use crate::grpc::service::MidnightStateService;
use crate::state::State;

pub async fn run(history: Arc<Mutex<StateHistory<State>>>, addr: SocketAddr) -> Result<()> {
    tracing::info!("Starting gRPC server on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("gRPC server listening on {}", addr);

    let service = MidnightStateService::new(history);
    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(FILE_DESCRIPTOR_SET)
        .build_v1()?;

    Server::builder()
        .add_service(reflection)
        .add_service(MidnightStateServer::new(service))
        .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
        .await?;

    Ok(())
}
