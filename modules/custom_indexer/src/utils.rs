use std::sync::Arc;

use acropolis_common::commands::chain_sync::ChainSyncCommand;
use acropolis_common::messages::{Command, Message};
use acropolis_common::{BlockInfo, Point};
use anyhow::Result;
use caryatid_sdk::Context;
use futures::stream::FuturesUnordered;
use tokio::sync::oneshot;
use tracing::info;

use crate::index_actor::{IndexCommand, IndexResult};
use crate::SharedSenders;

pub async fn change_sync_point(
    point: Point,
    context: Arc<Context<Message>>,
    topic: &String,
) -> Result<()> {
    let msg = Message::Command(Command::ChainSync(ChainSyncCommand::FindIntersect(
        point.clone(),
    )));
    context.publish(topic, Arc::new(msg)).await?;
    info!(
        "Publishing sync command on {} for slot {}",
        topic,
        point.slot()
    );

    Ok(())
}

pub async fn send_txs_to_indexers(
    senders: &SharedSenders,
    block: &BlockInfo,
    txs: &[Arc<[u8]>],
) -> FuturesUnordered<
    impl futures::Future<
        Output = (
            String,
            Result<IndexResult, tokio::sync::oneshot::error::RecvError>,
        ),
    >,
> {
    let senders_snapshot: Vec<_> = {
        let map = senders.lock().await;
        map.iter().map(|(n, tx)| (n.clone(), tx.clone())).collect()
    };

    let futs = FuturesUnordered::new();
    for (name, sender) in senders_snapshot {
        let (tx_resp, rx_resp) = oneshot::channel();

        let cmd = IndexCommand::ApplyTxs {
            block: block.clone(),
            txs: txs.to_vec(),
            response_tx: tx_resp,
        };

        let _ = sender.send(cmd).await;

        futs.push(async move { (name, rx_resp.await) });
    }
    futs
}

pub async fn send_rollback_to_indexers(
    senders: &SharedSenders,
    point: &Point,
) -> FuturesUnordered<
    impl futures::Future<
        Output = (
            String,
            Result<IndexResult, tokio::sync::oneshot::error::RecvError>,
        ),
    >,
> {
    let senders_snapshot: Vec<_> = {
        let map = senders.lock().await;
        map.iter().map(|(n, tx)| (n.clone(), tx.clone())).collect()
    };

    let futs = FuturesUnordered::new();
    for (name, sender) in senders_snapshot {
        let (tx_resp, rx_resp) = oneshot::channel();

        let cmd = IndexCommand::Rollback {
            point: point.clone(),
            response_tx: tx_resp,
        };

        let _ = sender.send(cmd).await;

        futs.push(async move { (name, rx_resp.await) });
    }
    futs
}
