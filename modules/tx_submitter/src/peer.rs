use std::{collections::VecDeque, sync::Arc, time::Duration};

use anyhow::{Context, Result, bail};
use config::Config;
use pallas::network::{facades::PeerClient, miniprotocols::txsubmission};
use tokio::{select, sync::mpsc};
use tracing::{debug, error, warn};

use crate::{SubmitterConfig, tx::Transaction};

pub struct PeerConfig {
    address: String,
}
impl PeerConfig {
    pub fn parse(config: &Config) -> Result<Self> {
        let address =
            config.get("node-address").unwrap_or("backbone.cardano.iog.io:3001").to_string();
        Ok(Self { address })
    }
}

pub struct PeerConnection {
    tx_sink: mpsc::UnboundedSender<Arc<Transaction>>,
}
impl PeerConnection {
    pub fn open(submitter: &SubmitterConfig, peer: PeerConfig) -> Self {
        let (tx_sink, tx_source) = mpsc::unbounded_channel();
        let worker = PeerWorker {
            tx_source,
            tx_queue: TxQueue::new(),
            address: peer.address,
            magic: submitter.magic,
        };
        tokio::task::spawn(worker.run());
        Self { tx_sink }
    }

    pub fn queue(&self, tx: Arc<Transaction>) -> Result<()> {
        self.tx_sink.send(tx).context("could not queue tx")
    }
}

struct PeerWorker {
    tx_source: mpsc::UnboundedReceiver<Arc<Transaction>>,
    tx_queue: TxQueue,
    address: String,
    magic: u64,
}
impl PeerWorker {
    async fn run(mut self) {
        while !self.tx_source.is_closed() {
            if let Err(error) = self.run_connection().await {
                error!("error connecting to {}: {:#}", self.address, error);
                debug!("reconnecting in 5 seconds");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }

    async fn run_connection(&mut self) -> Result<()> {
        let mut client =
            PeerClient::connect(&self.address, self.magic).await.context("could not connect")?;
        let submission = client.txsubmission();
        submission.send_init().await.context("failed to init")?;
        let mut pending_tx_requests = None;
        self.tx_queue.requeue_sent();
        loop {
            select! {
                new_tx = self.tx_source.recv() => {
                    let Some(tx) = new_tx else {
                        // parent process must have disconnected
                        break;
                    };
                    self.tx_queue.push(tx);
                    if let Some(req) = pending_tx_requests.take() {
                        let ids = self.tx_queue.req(req);
                        submission.reply_tx_ids(ids).await.context("could not send tx ids")?;
                        self.tx_queue.mark_requested(req);
                    }
                }
                request = submission.next_request(), if pending_tx_requests.is_none() => {
                    let req = request.context("could not receive request")?;
                    pending_tx_requests = self.handle_request(submission, req).await?;
                }
            }
        }
        if !matches!(submission.state(), txsubmission::State::Idle) {
            submission.send_done().await?;
        }
        Ok(())
    }

    async fn handle_request(
        &mut self,
        submission: &mut txsubmission::GenericClient<
            txsubmission::EraTxId,
            txsubmission::EraTxBody,
        >,
        req: txsubmission::Request<txsubmission::EraTxId>,
    ) -> Result<Option<u16>> {
        match req {
            txsubmission::Request::TxIds(ack, req) => {
                self.tx_queue.ack(ack)?;

                let ids = self.tx_queue.req(req);
                if ids.is_empty() {
                    Ok(Some(req))
                } else {
                    submission.reply_tx_ids(ids).await.context("could not send tx ids")?;
                    self.tx_queue.mark_requested(req);
                    Ok(None)
                }
            }
            txsubmission::Request::TxIdsNonBlocking(ack, req) => {
                self.tx_queue.ack(ack)?;

                let ids = self.tx_queue.req(req);
                submission.reply_tx_ids(ids).await.context("could not send tx ids")?;
                self.tx_queue.mark_requested(req);
                Ok(None)
            }
            txsubmission::Request::Txs(ids) => {
                let mut txs = vec![];
                for id in ids {
                    match self.tx_queue.tx_body(&id) {
                        Some(body) => {
                            debug!("Sending TX {}", hex::encode(id.1));
                            txs.push(body);
                        }
                        None => {
                            warn!("Server requested unrecognized TX {}", hex::encode(id.1));
                        }
                    }
                }
                submission.reply_txs(txs).await.context("could not send tx bodies")?;
                Ok(None)
            }
        }
    }
}

#[derive(Default)]
struct TxQueue {
    unsent: VecDeque<Arc<Transaction>>,
    sent: VecDeque<Arc<Transaction>>,
}
impl TxQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, tx: Arc<Transaction>) {
        self.unsent.push_back(tx);
    }

    pub fn ack(&mut self, count: u16) -> Result<()> {
        for _ in 0..count {
            match self.sent.pop_front() {
                Some(tx) => {
                    debug!("TX {} has been acknowledged", hex::encode(&tx.id))
                }
                None => bail!("Server acked a TX which we never sent"),
            }
        }
        Ok(())
    }

    pub fn req(&self, count: u16) -> Vec<txsubmission::TxIdAndSize<txsubmission::EraTxId>> {
        self.unsent
            .iter()
            .take(count as usize)
            .map(|tx| {
                txsubmission::TxIdAndSize(
                    txsubmission::EraTxId(tx.era, tx.id.clone()),
                    tx.body.len() as u32,
                )
            })
            .collect()
    }

    pub fn mark_requested(&mut self, count: u16) {
        for _ in 0..count {
            let tx = self.unsent.pop_front().expect("logic error");
            self.sent.push_back(tx);
        }
    }

    pub fn tx_body(&self, id: &txsubmission::EraTxId) -> Option<txsubmission::EraTxBody> {
        self.sent
            .iter()
            .find(|tx| tx.id == id.1)
            .map(|tx| txsubmission::EraTxBody(tx.era, tx.body.clone()))
    }

    pub fn requeue_sent(&mut self) {
        while let Some(tx) = self.sent.pop_back() {
            self.unsent.push_front(tx);
        }
    }
}
