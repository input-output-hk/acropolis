mod peer;
mod tx;

use std::sync::Arc;

use acropolis_common::{
    commands::transactions::{TransactionsCommand, TransactionsCommandResponse},
    messages::{Command, CommandResponse, Message},
};
use anyhow::{Result, bail};
use caryatid_sdk::{Context, Module, module};
use config::Config;
use peer::PeerConfig;
use tokio::sync::RwLock;

use crate::{peer::PeerConnection, tx::Transaction};

#[module(
    message_type(Message),
    name = "tx-submitter",
    description = "TX submission module"
)]
pub struct TxSubmitter;

impl TxSubmitter {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let submitter = Arc::new(SubmitterConfig::parse(&config)?);
        let peer = PeerConfig::parse(&config)?;
        let state = Arc::new(RwLock::new(SubmitterState {
            peers: vec![PeerConnection::open(&submitter, peer)],
        }));
        context.handle(&submitter.subscribe_topic, move |message| {
            let state = state.clone();
            async move {
                let state = state.read().await;
                let res = Self::handle_command(message, &state.peers)
                    .await
                    .unwrap_or_else(|e| TransactionsCommandResponse::Error(e.to_string()));
                Arc::new(Message::CommandResponse(CommandResponse::Transactions(res)))
            }
        });
        Ok(())
    }

    async fn handle_command(
        message: Arc<Message>,
        peers: &Vec<PeerConnection>,
    ) -> Result<TransactionsCommandResponse> {
        let Message::Command(Command::Transactions(TransactionsCommand::Submit { cbor })) =
            message.as_ref()
        else {
            bail!("unexpected tx request")
        };
        let tx = Arc::new(Transaction::from_bytes(cbor)?);
        for peer in peers {
            peer.queue(tx.clone())?;
        }
        Ok(TransactionsCommandResponse::Submitted { id: tx.id })
    }
}

struct SubmitterConfig {
    subscribe_topic: String,
    magic: u64,
}
impl SubmitterConfig {
    pub fn parse(config: &Config) -> Result<Self> {
        let subscribe_topic =
            config.get_string("subscribe-topic").unwrap_or("cardano.txs.submit".to_string());
        let magic = config.get("magic-number").unwrap_or(764824073);
        Ok(Self {
            subscribe_topic,
            magic,
        })
    }
}

struct SubmitterState {
    peers: Vec<PeerConnection>,
}
