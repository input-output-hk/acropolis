//! Caryatid Playback module

use acropolis_common::{
    messages::{
        CardanoMessage, DRepStakeDistributionMessage, GovernanceProceduresMessage, Message,
        SPOStakeDistributionMessage,
    },
    BlockHash, BlockInfo,
};
use anyhow::{anyhow, bail, ensure, Result};
use caryatid_sdk::{module, Context, Module};
use config::Config;
use std::collections::HashMap;
use std::fs::read_to_string;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

use crate::replayer_config::ReplayerConfig;

/// Playback module
/// Parameterised by the outer message enum used on the bus
#[module(
    message_type(Message),
    name = "gov-playback",
    description = "Governance messages playback"
)]
pub struct Playback;

struct PlaybackRunner {
    context: Arc<Context<Message>>,
    cfg: Arc<ReplayerConfig>,

    /// List of (topic, file prefix, epoch bound, skip epoch 0)
    topics: Arc<Vec<(String, String, bool, bool)>>,

    current_file: HashMap<String, u64>,
    empty_message: HashMap<String, Arc<CardanoMessage>>,
    next: HashMap<String, (BlockInfo, Arc<CardanoMessage>)>,
}

impl PlaybackRunner {
    fn new(context: Arc<Context<Message>>, cfg: Arc<ReplayerConfig>) -> Self {
        Self {
            context,
            cfg: cfg.clone(),
            topics: cfg.get_topics_vec(),
            current_file: HashMap::new(),
            next: HashMap::new(),
            empty_message: HashMap::new(),
        }
    }
}

impl Playback {
    async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let cfg = ReplayerConfig::new(&config);
        let mut playback_runner = PlaybackRunner::new(context.clone(), cfg);

        context.run(async move { playback_runner.run().await });

        Ok(())
    }
}

impl PlaybackRunner {
    fn empty_message(msg: &CardanoMessage) -> Result<Arc<CardanoMessage>> {
        match msg {
            CardanoMessage::GovernanceProcedures(_) => Ok(Arc::new(
                CardanoMessage::GovernanceProcedures(GovernanceProceduresMessage::default()),
            )),
            CardanoMessage::DRepStakeDistribution(_) => Ok(Arc::new(
                CardanoMessage::DRepStakeDistribution(DRepStakeDistributionMessage::default()),
            )),
            CardanoMessage::SPOStakeDistribution(_) => Ok(Arc::new(
                CardanoMessage::SPOStakeDistribution(SPOStakeDistributionMessage::default()),
            )),
            m => bail!("Cannot empty message {m:?}"),
        }
    }

    fn take_message(
        &mut self,
        topic: String,
        prefix: String,
    ) -> Result<Option<Arc<CardanoMessage>>> {
        let num = self.current_file.get(&topic).unwrap_or(&0);

        let path = PathBuf::from(self.cfg.path.clone());
        let filename = path.join(format!("{prefix}-{num}.json"));

        if !filename.exists() {
            self.next.remove(&topic);
            return Ok(None);
        }

        let (id, message) = match read_to_string(&filename) {
            Ok(file) => match serde_json::from_str::<Message>(&file) {
                Ok(Message::Cardano((id, cardano_message))) => (id, Arc::new(cardano_message)),
                Ok(m) => bail!("Expected CardanoMessage, found {m:?}"),
                Err(error) => bail!("Failed to parse message from file {filename:?}: {error}"),
            },

            Err(error) => bail!("Failed to read file {filename:?}: {error}. Aborting playback"),
        };

        self.current_file.insert(topic.clone(), num + 1);
        self.next.insert(topic, (id, message.clone()));
        Ok(Some(message))
    }

    fn get_earliest_available_block(&self) -> Option<BlockInfo> {
        self.next.values().map(|(blk, _msg)| blk).min().map(|x| (*x).clone())
    }

    fn gen_block_info(
        curr_block_num: u64,
        prev_blk: &BlockInfo,
        pending_blk: &BlockInfo,
    ) -> Result<BlockInfo> {
        let mut curr_blk = prev_blk.clone();
        curr_blk.slot += curr_block_num - prev_blk.number;
        curr_blk.number = curr_block_num;
        curr_blk.hash = BlockHash::default();
        curr_blk.new_epoch = false;

        ensure!(curr_blk.slot < pending_blk.slot);
        ensure!(curr_blk.number < pending_blk.number);
        ensure!(
            curr_blk.epoch == pending_blk.epoch
                || (curr_blk.epoch + 1 == pending_blk.epoch && pending_blk.new_epoch)
        );

        Ok(curr_blk)
    }

    async fn send_message(&self, topic: &str, blk: &BlockInfo, msg: &CardanoMessage) -> Result<()> {
        let msg = Arc::new(Message::Cardano((blk.clone(), msg.clone())));
        //info!("Publishing {msg:?} to {topic}");
        self.context.message_bus.publish(topic, msg).await
    }

    async fn send_messages_to_all(&self, curr_blk: &BlockInfo) -> Result<()> {
        for (topic, _prefix, epoch_bound, skip_zero) in self.topics.iter() {
            if (!skip_zero || curr_blk.epoch != 0) && (!epoch_bound || curr_blk.new_epoch) {
                let msg = match self.next.get(topic) {
                    Some((blk, msg)) if blk == curr_blk => msg.clone(),

                    Some((blk, _)) if blk.number == curr_blk.number => {
                        bail!("{blk:?} != {curr_blk:?}")
                    }

                    Some((blk, _)) if blk.number < curr_blk.number => {
                        bail!("{blk:?} < {curr_blk:?}")
                    }

                    Some(_) | None => self
                        .empty_message
                        .get(topic)
                        .ok_or_else(|| anyhow!("No empty message for {topic}"))?
                        .clone(),
                };
                self.send_message(topic, curr_blk, &msg).await?;
            }
        }
        Ok(())
    }

    fn step_forward(&mut self, current_step: &BlockInfo) -> Result<()> {
        for (topic, prefix, _epoch_bound, _skip_zero) in self.topics.clone().iter() {
            if let Some((blk, _msg)) = self.next.get(topic) {
                if blk == current_step {
                    self.take_message(topic.to_string(), prefix.to_string())?;
                } else if blk.number <= current_step.number {
                    bail!("Impossible next block info for {topic}: {blk:?} < {current_step:?}");
                }
            }
        }
        Ok(())
    }

    fn dump_state(&self) {
        let stats = self
            .next
            .iter()
            .map(|(topic, (blk, _msg))| format!("{topic}: {}:{}  ", blk.epoch, blk.number))
            .collect::<String>();
        info!("Current replay state: {stats}");
    }

    async fn run(&mut self) -> Result<()> {
        // Initializing message status
        for (topic, prefix, _epoch_bound, _skip_zero) in self.topics.clone().iter() {
            let msg = self
                .take_message(topic.to_string(), prefix.to_string())?
                .ok_or_else(|| anyhow!("Topic {topic} may not be empty"))?;

            self.empty_message.insert(topic.to_string(), Self::empty_message(&msg)?);
        }

        let mut prev_blk = match self.get_earliest_available_block() {
            Some(minimal_blk) => minimal_blk,
            None => bail!("At least one real block is required for replay"),
        };

        if prev_blk.number != 1 {
            bail!("First replay block should be with number 1 instead of {prev_blk:?}")
        }

        let mut granularity = 0;
        while let Some(pending_blk) = self.get_earliest_available_block() {
            if granularity % 100 == 0 {
                self.dump_state();
                granularity += 1;
            }

            for curr_block_num in prev_blk.number..pending_blk.number {
                let cur_blk = Self::gen_block_info(curr_block_num, &prev_blk, &pending_blk)?;
                self.send_messages_to_all(&cur_blk).await?;
            }

            self.send_messages_to_all(&pending_blk).await?;
            self.step_forward(&pending_blk)?;

            prev_blk = pending_blk;
        }

        info!("All messages replayed, stopping");
        Ok(())
    }
}
