use std::{net::SocketAddr, sync::Arc};

use acropolis_common::state_history::StateHistory;
use anyhow::Result;
use tokio::sync::Mutex;
use tonic::transport::Server;

use crate::state::State;

use crate::grpc::midnight_state_proto::midnight_state_server::MidnightStateServer;
use crate::grpc::service::MidnightStateService;

pub async fn run(history: Arc<Mutex<StateHistory<State>>>, addr: SocketAddr) -> Result<()> {
    let service = MidnightStateService::new(history);

    Server::builder().add_service(MidnightStateServer::new(service)).serve(addr).await?;

    Ok(())
}
