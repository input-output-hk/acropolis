//! gRPC server for the custom indexer PoC.
//!
//! The server has enabled reflection. Use the following for
//! command line requests via grpcurl:
//!
//! with reflection:
//!
//! grpcurl -plaintext 127.0.0.1:50051 list
//!
//! grpcurl -plaintext \
//!   127.0.0.1:50051 \
//!   acropolis.indexer.v1.ChainSyncService/GetTip
//!
//! grpcurl -plaintext \
//!   127.0.0.1:50051 \
//!   acropolis.indexer.v1.ChainSyncService/FollowTip
//!
//! grpcurl -plaintext \
//!   -d '{"hash": "{BASE_64_HASH}' \
//!   localhost:50051 \
//!   acropolis.indexer.v1.ChainSyncService/GetBlockByHash
//!
use std::net::SocketAddr;
use std::pin::Pin;

use acropolis_common::Point;
use anyhow::Result;
use tokio_stream::{wrappers::BroadcastStream, Stream, StreamExt};
use tonic::{Request, Response, Status};
use tracing::info;

use crate::{BlockRecord, ChainEvent, IndexerHandle};

pub mod proto {
    tonic::include_proto!("acropolis.indexer.v1");
    pub const FILE_DESCRIPTOR_SET: &[u8] =
        tonic::include_file_descriptor_set!("indexer_descriptor");
}

use proto::{
    chain_sync_service_server::{ChainSyncService, ChainSyncServiceServer},
    BlockEvent, ChainPoint, FollowTipRequest, GetBlockByHashRequest, GetBlockByHashResponse,
    GetTipRequest, GetTipResponse, RollbackEvent, TipEvent,
};


// Adapters

fn point_to_proto(point: &Point) -> ChainPoint {
    match point {
        Point::Origin => ChainPoint {
            slot: 0,
            block_hash: vec![],
        },
        Point::Specific { hash, slot } => ChainPoint {
            slot: *slot,
            block_hash: hash.to_vec(),
        },
    }
}

fn block_record_to_proto(record: &BlockRecord) -> proto::BlockInfo {
    proto::BlockInfo {
        block_number: record.block_number,
        hash: record.hash.to_vec(),
        epoch: record.epoch,
        slot: record.slot,
        timestamp: record.timestamp,
        tx_count: record.tx_count,
    }
}

fn event_to_proto(event: ChainEvent) -> TipEvent {
    match event {
        ChainEvent::RollForward { block, tx_count } => TipEvent {
            event: Some(proto::tip_event::Event::RollForward(BlockEvent {
                point: Some(ChainPoint {
                    slot: block.slot,
                    block_hash: block.hash.to_vec(),
                }),
                block_number: block.number,
                tx_count,
            })),
        },
        ChainEvent::RollBack(point) => TipEvent {
            event: Some(proto::tip_event::Event::RollBackward(RollbackEvent {
                point: Some(point_to_proto(&point)),
            })),
        },
    }
}

// Service

struct Service {
    handle: IndexerHandle,
}

#[tonic::async_trait]
impl ChainSyncService for Service {
    async fn get_tip(
        &self,
        _request: Request<GetTipRequest>,
    ) -> Result<Response<GetTipResponse>, Status> {
        let tip = self.handle.tip().await.map(|p| point_to_proto(&p));
        Ok(Response::new(GetTipResponse { tip }))
    }

    type FollowTipStream = Pin<Box<dyn Stream<Item = Result<TipEvent, Status>> + Send>>;

    async fn follow_tip(
        &self,
        _request: Request<FollowTipRequest>,
    ) -> Result<Response<Self::FollowTipStream>, Status> {
        let rx = self.handle.subscribe();
        let stream = BroadcastStream::new(rx)
            .filter_map(|r| r.ok())
            .map(|event| Ok(event_to_proto(event)));
        Ok(Response::new(Box::pin(stream)))
    }

    async fn get_block_by_hash(
        &self,
        request: Request<GetBlockByHashRequest>,
    ) -> Result<Response<GetBlockByHashResponse>, Status> {
        let hash_bytes = &request.get_ref().hash;
        let hash: [u8; 32] = hash_bytes
            .as_slice()
            .try_into()
            .map_err(|_| Status::invalid_argument("hash must be exactly 32 bytes"))?;
        let block_hash = acropolis_common::BlockHash::from(hash);

        match self.handle.get_block_by_hash(&block_hash).await {
            Some(record) => Ok(Response::new(GetBlockByHashResponse {
                block: Some(block_record_to_proto(&record)),
            })),
            None => Err(Status::not_found("block not found")),
        }
    }
}

/// Start the gRPC server.
///
/// Reads the indexer, blocks until the server is shut down.
pub async fn start_grpc_server(addr: SocketAddr, handle: IndexerHandle) -> Result<()> {
    let reflection = tonic_reflection::server::Builder::configure()
        .register_encoded_file_descriptor_set(proto::FILE_DESCRIPTOR_SET)
        .build_v1()?;

    info!("gRPC custom indexer server listening on {addr}");
    tonic::transport::Server::builder()
        .add_service(reflection)
        .add_service(ChainSyncServiceServer::new(Service { handle }))
        .serve(addr)
        .await?;
    Ok(())
}
