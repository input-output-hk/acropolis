use std::collections::{HashMap, VecDeque};

use acropolis_common::{
    params::SECURITY_PARAMETER_K, DRepChoice, LovelaceDelta, PoolId, StakeAddress,
};

#[derive(Default)]
pub struct StakeAddressJournal {
    /// First block number represented in the index VecDeque
    first_block: u64,

    deltas: VecDeque<Vec<(StakeAddress, StakeAddressStateDelta)>>,
}

impl StakeAddressJournal {
    pub fn commit(&mut self, block_number: u64, block_deltas: BlockStakeAddressDeltas) {
        if self.deltas.is_empty() {
            self.first_block = block_number;
        }

        self.deltas.push_back(block_deltas.into_vec());

        while block_number.saturating_sub(self.first_block) >= SECURITY_PARAMETER_K {
            self.deltas.pop_front();
            self.first_block += 1;
        }
    }

    pub fn rollback_to(
        &mut self,
        block_number: u64,
    ) -> HashMap<StakeAddress, StakeAddressStateDelta> {
        let last_block = self.first_block + self.deltas.len() as u64;
        let mut blocks_to_pop = last_block - block_number;

        let mut aggregated_deltas: HashMap<StakeAddress, StakeAddressStateDelta> = HashMap::new();
        while blocks_to_pop > 0 {
            if let Some(block_deltas) = self.deltas.pop_back() {
                for (stake_address, delta) in block_deltas {
                    let mut entry =
                        aggregated_deltas.get_mut(&stake_address).cloned().unwrap_or_default();

                    if let Some(drep) = delta.delegated_drep {
                        entry.delegated_drep = Some(drep);
                    }

                    if let Some(pool) = delta.delegated_spo {
                        entry.delegated_spo = Some(pool);
                    }

                    if let Some(registration_status) = delta.registered {
                        entry.registered = Some(registration_status);
                    }

                    if let Some(utxo_delta) = delta.utxo_value {
                        if let Some(existing_delta) = entry.utxo_value {
                            entry.utxo_value = Some(existing_delta - utxo_delta);
                        } else {
                            entry.utxo_value = Some(utxo_delta);
                        }
                    }

                    if let Some(reward_delta) = delta.rewards {
                        if let Some(existing_rewards) = entry.rewards {
                            entry.rewards = Some(existing_rewards - reward_delta);
                        } else {
                            entry.rewards = Some(reward_delta)
                        }
                    }
                }
            } else {
                break;
            }
            blocks_to_pop -= 1;
        }
        aggregated_deltas
    }
}

#[derive(Default, Clone)]
pub struct StakeAddressStateDelta {
    registered: Option<bool>,
    utxo_value: Option<LovelaceDelta>,
    rewards: Option<LovelaceDelta>,
    delegated_spo: Option<Option<PoolId>>,
    delegated_drep: Option<Option<DRepChoice>>,
}

#[derive(Default)]
pub struct BlockStakeAddressDeltas(HashMap<StakeAddress, StakeAddressStateDelta>);

impl BlockStakeAddressDeltas {
    fn entry(&mut self, addr: &StakeAddress) -> &mut StakeAddressStateDelta {
        self.0.entry(addr.clone()).or_default()
    }

    pub fn into_vec(self) -> Vec<(StakeAddress, StakeAddressStateDelta)> {
        self.0.into_iter().collect()
    }

    pub fn record_registration(&mut self, stake_address: &StakeAddress, prior: bool) {
        let delta = self.entry(stake_address);
        if delta.registered.is_none() {
            delta.registered = Some(prior);
        }
    }

    pub fn record_deregistration(&mut self, addr: &StakeAddress, prior: bool) {
        let delta = self.entry(addr);

        if delta.registered.is_none() {
            delta.registered = Some(prior);
        }
    }

    pub fn record_stake_delta(&mut self, addr: &StakeAddress, delta_val: LovelaceDelta) {
        let delta = self.entry(addr);

        match delta.utxo_value {
            Some(current) => delta.utxo_value = Some(current + delta_val),
            None => delta.utxo_value = Some(delta_val),
        }
    }

    pub fn record_reward_delta(&mut self, addr: &StakeAddress, delta_val: LovelaceDelta) {
        let delta = self.entry(addr);

        match delta.rewards {
            Some(current) => delta.rewards = Some(current + delta_val),
            None => delta.rewards = Some(delta_val),
        }
    }

    pub fn record_pool_delegation(&mut self, addr: &StakeAddress, prior: Option<PoolId>) {
        let delta = self.entry(addr);

        if delta.delegated_spo.is_none() {
            delta.delegated_spo = Some(prior);
        }
    }

    pub fn record_drep_delegation(&mut self, addr: &StakeAddress, prior: Option<DRepChoice>) {
        let delta = self.entry(addr);

        if delta.delegated_drep.is_none() {
            delta.delegated_drep = Some(prior);
        }
    }
}
