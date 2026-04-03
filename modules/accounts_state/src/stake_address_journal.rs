use std::collections::{HashMap, VecDeque};

use acropolis_common::{
    params::SECURITY_PARAMETER_K,
    stake_addresses::{BlockStakeAddressDeltas, StakeAddressStateDelta},
    StakeAddress,
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

    #[allow(dead_code)]
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
