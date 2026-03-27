use std::{net::SocketAddr, sync::Arc};

use acropolis_common::messages::Message;
use acropolis_common::state_history::StateHistory;
use anyhow::Result;
use caryatid_sdk::Context;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use tonic::transport::Server;

use crate::grpc::midnight_state_proto::midnight_state_server::MidnightStateServer;
use crate::grpc::midnight_state_proto::FILE_DESCRIPTOR_SET;
use crate::grpc::service::MidnightStateService;
use crate::state::State;

pub async fn run(
    history: Arc<Mutex<StateHistory<State>>>,
    context: Arc<Context<Message>>,
    addr: SocketAddr,
) -> Result<()> {
    tracing::info!("Starting gRPC server on {}", addr);
    let listener = TcpListener::bind(addr).await?;
    tracing::info!("gRPC server listening on {}", addr);

    let service = MidnightStateService::new(history, context);

    // background stats logger
    let stats_service = service.clone();
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(60)).await;
            if let Some(stats) = stats_service.stats() {
                tracing::info!("gRPC request stats: {}", stats);
            }
        }
    });

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
