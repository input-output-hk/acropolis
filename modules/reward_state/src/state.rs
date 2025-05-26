//! Acropolis RewardState: State storage
use acropolis_common::{
    messages::EpochActivityMessage,
    BlockInfo,
    params::SECURITY_PARAMETER_K,
};
use anyhow::Result;
use imbl::HashMap;
use tracing::{error, info};
use serde::{Serializer, ser::SerializeMap};
use serde_with::{serde_as, hex::Hex, SerializeAs, ser::SerializeAsWrap};
use std::collections::VecDeque;

struct HashMapSerial<KAs, VAs>(std::marker::PhantomData<(KAs, VAs)>);

// TODO!  This should move to common - AJW may have already done so
impl<K, V, KAs, VAs> SerializeAs<HashMap<K, V>> for HashMapSerial<KAs, VAs>
where
    KAs: SerializeAs<K>,
    VAs: SerializeAs<V>,
{
    fn serialize_as<S>(source: &HashMap<K, V>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map_ser = serializer.serialize_map(Some(source.len()))?;
        for (k, v) in source {
            map_ser.serialize_entry(
                &SerializeAsWrap::<K, KAs>::new(k),
                &SerializeAsWrap::<V, VAs>::new(v),
            )?;
        }
        map_ser.end()
    }
}

#[serde_as]
#[derive(Debug, Clone, serde::Serialize)]
pub struct BlockState {
    block: u64,

    epoch: u64,

    #[serde_as(as = "HashMapSerial<Hex, _>")]
    rewards: HashMap<Vec::<u8>, u64>,
}

impl BlockState {
    pub fn new(block: u64, epoch: u64, rewards: HashMap<Vec::<u8>, u64>) -> Self {
        Self {
            block,
            epoch,
            rewards,
        }
    }
}

pub struct State {
    history: VecDeque<BlockState>,
}

impl State {
    pub fn new() -> Self {
        Self {
            history: VecDeque::<BlockState>::new(),
        }
    }

    pub fn current(&self) -> Option<&BlockState> {
        self.history.back()
    }

    pub fn get(&self, stake_key: &Vec<u8>) -> Option<&u64> {
        if let Some(current) = self.current() {
            current.rewards.get(stake_key)
        } else {
            None
        }
    }

    async fn log_stats(&self) {
        if let Some(current) = self.current() {
            info!(
                num_rewards = current.rewards.keys().len(),
            );
        } else {
            info!(num_rewards = 0);
        }
    }

    pub async fn tick(&self) -> Result<()> {
        self.log_stats().await;
        Ok(())
    }

    fn get_previous_state(&mut self, block_number: u64) -> BlockState {
        loop {
            match self.history.back() {
                Some(state) => if state.block >= block_number {
                    info!("Rolling back state for block {}", state.block);
                    self.history.pop_back();
                } else {
                    break
                },
                _ => break
            }
        }
        if let Some(current) = self.history.back() {
            current.clone()
        } else {
            BlockState::new(0, 0, HashMap::new())
        }
    }

    pub fn handle_epoch_activity(&mut self, block: &BlockInfo,
                                ea_msg: &EpochActivityMessage) -> Result<()> {
        let mut current = self.get_previous_state(block.number);
        current.block = block.number;
        if block.epoch > current.epoch {
            current.epoch = block.epoch;
        }

        if self.history.len() >= SECURITY_PARAMETER_K as usize {
            self.history.pop_front();
        }
        self.history.push_back(current);

        Ok(())
    }
}

// -- Tests --
#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{
        BlockInfo,
        Era,
        BlockStatus,
    };

    #[tokio::test]
    async fn new_state_is_empty() {
        let state = State::new();
        assert_eq!(0, state.history.len());
    }

    #[tokio::test]
    async fn current_on_new_state_returns_none() {
        let state = State::new();
        assert!(state.current().is_none());
    }

    fn new_msg() -> EpochActivityMessage {
        EpochActivityMessage {
            epoch: 0,
            total_blocks: 0,
            total_fees: 0,
            vrf_vkeys: Vec::new(),
        }
    }

    fn new_block() -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Immutable,
            slot: 0,
            number: 0,
            hash: Vec::<u8>::new(),
            epoch: 0,
            new_epoch: true,
            era: Era::Byron,
        }
    }
}
