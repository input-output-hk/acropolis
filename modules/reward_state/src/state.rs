//! Acropolis RewardState: State storage
use acropolis_common::{
    messages::{EpochActivityMessage, SPOStateMessage, TxCertificatesMessage},
    BlockInfo, PoolRegistration, TxCertificate,
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
    /// Block this state is for
    block: u64,

    /// Epoch this state is for
    epoch: u64,

    /// Map of active SPOs by VRF vkey
    #[serde_as(as = "HashMapSerial<Hex, _>")]
    spos_by_vrf_key: HashMap<Vec::<u8>, PoolRegistration>,

    /// Map of reward values by staking address
    #[serde_as(as = "HashMapSerial<Hex, _>")]
    rewards: HashMap<Vec::<u8>, u64>,
}

impl BlockState {
    pub fn new() -> Self {
        Self {
            block: 0,
            epoch: 0,
            spos_by_vrf_key: HashMap::new(),
            rewards: HashMap::new(),
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
            BlockState::new()
        }
    }

    /// Handle an EpochActivityMessage giving total fees and block counts by VRF key for
    /// the just-ended epoch
    pub fn handle_epoch_activity(&mut self, block: &BlockInfo,
                                 ea_msg: &EpochActivityMessage) -> Result<()> {
        let mut current = self.get_previous_state(block.number);
        // !TODO how do we manage rollbacks from two different sources!
        //current.block = block.number;
        current.epoch = ea_msg.epoch;

        // Look up every VRF key in the SPO map
        for (vrf_vkey_hash, count) in ea_msg.vrf_vkey_hashes.iter() {
            match current.spos_by_vrf_key.get(vrf_vkey_hash) {
                Some(spo) => {
                    // !TODO count rewards for this block
                }

                None => error!("VRF vkey {} not found in SPO map", hex::encode(vrf_vkey_hash))
            }
        }

        if self.history.len() >= SECURITY_PARAMETER_K as usize {
            self.history.pop_front();
        }
        self.history.push_back(current);

        Ok(())
    }

    /// Handle an SPOStateMessage with the full set of SPOs valid at the end of the last
    /// epoch
    pub fn handle_spo_state(&mut self, block: &BlockInfo,
                            spo_msg: &SPOStateMessage) -> Result<()> {
        let mut current = self.get_previous_state(block.number);
        // !TODO current.block = block.number;
        current.epoch = spo_msg.epoch;

        // Capture current SPOs, mapped by VRF vkey hash
        current.spos_by_vrf_key = spo_msg.spos.iter()
            .cloned()
            .map(|spo| (spo.vrf_key_hash.clone(), spo))
            .collect();

        if self.history.len() >= SECURITY_PARAMETER_K as usize {
            self.history.pop_front();
        }
        self.history.push_back(current);

        Ok(())
    }

    /// Handle TxCertificates with stake delegations
    pub fn handle_tx_certificates(&mut self, block: &BlockInfo,
                                  tx_certs_msg: &TxCertificatesMessage) -> Result<()> {
        let mut current = self.get_previous_state(block.number);
        current.block = block.number;

        // Handle certificates
        for tx_cert in tx_certs_msg.certificates.iter() {
            match tx_cert {
                TxCertificate::StakeDelegation(delegation) => {
                    // !TODO record delegation
                }

                // !TODO Conway delegation varieties

                _ => ()
            }
        }

        // Prune and add to state history
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
            vrf_vkey_hashes: Vec::new(),
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
