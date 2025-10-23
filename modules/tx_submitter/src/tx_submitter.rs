mod peer;
mod tx;

use std::sync::Arc;

use acropolis_common::messages::Message;
use anyhow::Result;
use caryatid_sdk::Context;
use config::Config;
use peer::PeerConfig;
use tracing::error;

use crate::{peer::PeerConnection, tx::Transaction};

pub struct TxSubmitter;

impl TxSubmitter {
    async fn run_tx_submission(
        config: Arc<SubmitterConfig>,
        peer_config: PeerConfig,
    ) -> Result<()> {
        let mut peers = vec![PeerConnection::open(&config, peer_config)];
        loop {
            let tx_bytes = Self::get_tx().await;
            let tx = Arc::new(Transaction::from_bytes(&tx_bytes)?);
            peers.retain(|peer| peer.queue(tx.clone()).is_ok());
        }
    }

    async fn get_tx() -> Vec<u8> {
        todo!()
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let submitter = Arc::new(SubmitterConfig::parse(&config)?);
        let peer = PeerConfig::parse(&config)?;
        context.run(async move {
            Self::run_tx_submission(submitter, peer)
                .await
                .unwrap_or_else(|e| error!("TX submission failed: {e}"));
        });
        Ok(())
    }
}

struct SubmitterConfig {
    magic: u64,
}
impl SubmitterConfig {
    pub fn parse(config: &Config) -> Result<Self> {
        let magic = config.get("magic-number").unwrap_or(764824073);
        Ok(Self { magic })
    }
}
