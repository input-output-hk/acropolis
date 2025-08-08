//! Alonzo governance recorder module

use anyhow::{anyhow, Result};
use caryatid_sdk::{module, Context, Module, Subscription};
use acropolis_common::{
    BlockInfo, GenesisKeyhash, ProtocolParamUpdate, AlonzoBabbageUpdateProposal,
    messages::{Message, CardanoMessage, GovernanceProceduresMessage}
};
use config::Config;
use std::{fs::File, io::Write, sync::Arc};
use serde_with::{serde_as, base64::Base64};
use tracing::error;

use crate::replayer_config::ReplayerConfig;

/// Recorder module
#[module(message_type(Message),
  name = "gov-alonzo-recorder",
  description = "Alonzo governance messages recorder")]
pub struct RecorderAlonzoGovernance;

#[serde_as]
#[derive(serde::Serialize, serde::Deserialize)]
struct ReplayerGenesisKeyhash(
    #[serde_as(as = "Base64")]
    GenesisKeyhash
);

struct BlockRecorder {
    cfg: Arc<ReplayerConfig>,
    prefix: String,
    // slot, epoch, era (num), new_epoch, [enactment epoch, voting: [key, vote]]
    list: Vec<(u64,u64,u8,u8,Vec<(u64,Vec<(ReplayerGenesisKeyhash, Box<ProtocolParamUpdate>)>)>)>,
}

impl BlockRecorder {
    pub fn new(cfg: Arc<ReplayerConfig>, prefix: &str) -> Self {
        Self { cfg, prefix: prefix.to_string(), list: Vec::new() }
    }

    pub fn write(&mut self, 
        block: &BlockInfo, votes: &Vec<AlonzoBabbageUpdateProposal>
    ) {
        let file = format!("{}/{}.json", self.cfg.path, self.prefix);

        let mut proposals = Vec::new();
        for vote in votes.iter() {
            let mut votes_indexed = Vec::new();
            for (h,u) in &vote.proposals {
                votes_indexed.push((ReplayerGenesisKeyhash(h.clone()),u.clone()));
            }
            proposals.push((vote.enactment_epoch, votes_indexed));
        }

        self.list.push(
            (block.slot, block.epoch, block.era.clone() as u8, block.new_epoch as u8, proposals)
        );

        let mut file = File::create(file).unwrap();
        let mut continuation = "[";
        for list_elem in &self.list {
            let serialized = format!("{}{}", 
                continuation, serde_json::to_string(list_elem).unwrap()
            );
            file.write_all(serialized.as_bytes()).unwrap();
            continuation = ",\n";
        }
        file.write_all("]\n".as_bytes()).unwrap();
    }
}

impl RecorderAlonzoGovernance {
    async fn read_governance(
        governance_s: &mut Box<dyn Subscription<Message>>,
    ) -> Result<(BlockInfo, GovernanceProceduresMessage)> {
        match governance_s.read().await?.1.as_ref() {
            Message::Cardano((blk, CardanoMessage::GovernanceProcedures(msg))) => {
                Ok((blk.clone(), msg.clone()))
            }
            msg => Err(anyhow!(
                "Unexpected message {msg:?} for governance procedures topic"
            )),
        }
    }

    async fn run(
        cfg: Arc<ReplayerConfig>,
        mut governance_s: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let mut gov_recorder = BlockRecorder::new(cfg.clone(), "alonzo-gov");

        loop {
            let (blk_g, procs) = Self::read_governance(&mut governance_s).await?;

            let procs_empty = procs.alonzo_babbage_updates.is_empty() && !blk_g.new_epoch;

            if !procs_empty {
                gov_recorder.write(&blk_g, &procs.alonzo_babbage_updates);
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = ReplayerConfig::new(&config);
        let st = context.clone().subscribe(&cfg.subscribe_topic).await?;

        tokio::spawn(async move {
            Self::run(cfg, st).await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
