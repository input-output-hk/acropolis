//! Governance recorder module

use anyhow::{anyhow, Result};
use caryatid_sdk::{module, Context, Module, Subscription};
use acropolis_common::{BlockInfo, 
    messages::{
        Message, CardanoMessage, GovernanceProceduresMessage,
        DRepStakeDistributionMessage, SPOStakeDistributionMessage
    }
};
use config::Config;
use std::{fs::File, io::Write, sync::Arc};
use tracing::{error, info};

use crate::replayer_config::ReplayerConfig;

/// Recorder module
#[module(message_type(Message), name = "gov-recorder", description = "Governance messages recorder")]
pub struct Recorder;

struct BlockRecorder {
    cfg: Arc<ReplayerConfig>,
    prefix: String,
    num: usize
}

impl BlockRecorder {
    pub fn new(cfg: Arc<ReplayerConfig>, prefix: &str) -> Self {
        Self { cfg, prefix: prefix.to_string(), num: 0 }
    }

    pub fn write(&mut self, block: &BlockInfo, info: CardanoMessage) {
        let file = format!("{}/{}-{}.json", self.cfg.path, self.prefix, self.num);
        let serialized = serde_json::to_string(&Message::Cardano((block.clone(), info))).unwrap();

        let mut file = File::create(file).unwrap();
        file.write_all(serialized.as_bytes()).unwrap();
        self.num += 1;
    }
}

impl Recorder {
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

    async fn read_drep(
        drep_s: &mut Box<dyn Subscription<Message>>,
    ) -> Result<(BlockInfo, DRepStakeDistributionMessage)> {
        match drep_s.read().await?.1.as_ref() {
            Message::Cardano((blk, CardanoMessage::DRepStakeDistribution(distr))) => {
                Ok((blk.clone(), distr.clone()))
            }
            msg => Err(anyhow!(
                "Unexpected message {msg:?} for DRep distribution topic"
            )),
        }
    }

    async fn read_spo(
        spo_s: &mut Box<dyn Subscription<Message>>,
    ) -> Result<(BlockInfo, SPOStakeDistributionMessage)> {
        match spo_s.read().await?.1.as_ref() {
            Message::Cardano((blk, CardanoMessage::SPOStakeDistribution(distr))) => {
                Ok((blk.clone(), distr.clone()))
            }
            msg => Err(anyhow!(
                "Unexpected message {msg:?} for SPO distribution topic"
            )),
        }
    }


    async fn run(
        cfg: Arc<ReplayerConfig>,
        mut governance_s: Box<dyn Subscription<Message>>,
        mut drep_s: Box<dyn Subscription<Message>>,
        mut spo_s: Box<dyn Subscription<Message>>,
    ) -> Result<()> {
        let mut gov_recorder = BlockRecorder::new(cfg.clone(), "gov");
        let mut spo_recorder = BlockRecorder::new(cfg.clone(), "spo");
        let mut drep_recorder = BlockRecorder::new(cfg.clone(), "drep");

        loop {
            let (blk_g, gov_procs) = Self::read_governance(&mut governance_s).await?;

            let gov_procs_empty =
                gov_procs.proposal_procedures.is_empty() &&
                gov_procs.voting_procedures.is_empty() &&
                !blk_g.new_epoch;

            if !gov_procs_empty {
                gov_recorder.write(&blk_g, CardanoMessage::GovernanceProcedures(gov_procs));
            }

            if blk_g.new_epoch {
                if blk_g.epoch > 0 {
                    info!("Waiting drep...");
                    let (blk_drep, d_drep) = Self::read_drep(&mut drep_s).await?;
                    if blk_g != blk_drep {
                        error!("Governance {blk_g:?} and DRep distribution {blk_drep:?} are out of sync");
                    }

                    info!("Waiting spo...");
                    let (blk_spo, d_spo) = Self::read_spo(&mut spo_s).await?;
                    if blk_g != blk_spo {
                        error!("Governance {blk_g:?} and SPO distribution {blk_spo:?} are out of sync");
                    }

                    drep_recorder.write(&blk_g, CardanoMessage::DRepStakeDistribution(d_drep));
                    spo_recorder.write(&blk_g, CardanoMessage::SPOStakeDistribution(d_spo));
                }
            }
        }
    }

    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = ReplayerConfig::new(&config);
        let gt = context.clone().subscribe(&cfg.subscribe_topic).await?;
        let dt = context.clone().subscribe(&cfg.drep_distribution_topic).await?;
        let st = context.clone().subscribe(&cfg.spo_distribution_topic).await?;

        tokio::spawn(async move {
            Self::run(cfg, gt, dt, st).await.unwrap_or_else(|e| error!("Failed: {e}"));
        });

        Ok(())
    }
}
