use std::collections::{HashMap, VecDeque};

use acropolis_common::Address;

use crate::state::AddressEntry;

#[derive(Debug, Clone)]
pub struct VolatileAddresses {
    pub window: VecDeque<HashMap<Address, AddressEntry>>,
    pub start_block: u64,
    pub epoch_start_block: u64,
    pub last_persisted_epoch: Option<u64>,
    pub security_param_k: u64,
}

impl Default for VolatileAddresses {
    fn default() -> Self {
        Self::new()
    }
}

impl VolatileAddresses {
    pub fn new() -> Self {
        let mut window = VecDeque::new();
        window.push_back(HashMap::new());

        VolatileAddresses {
            window,
            start_block: 0,
            epoch_start_block: 0,
            last_persisted_epoch: None,
            security_param_k: 0,
        }
    }

    pub fn update_k(&mut self, k: u32) {
        self.security_param_k = k as u64;
    }

    pub fn next_block(&mut self) {
        self.window.push_back(HashMap::new());
    }

    pub fn start_new_epoch(&mut self, block_number: u64) {
        self.epoch_start_block = block_number;
    }

    pub fn rollback_before(&mut self, block: u64) -> Vec<(Address, AddressEntry)> {
        let mut out = Vec::new();

        while self.start_block + self.window.len() as u64 >= block {
            if let Some(map) = self.window.pop_back() {
                out.extend(map.into_iter());
            } else {
                break;
            }
        }
        out
    }

    pub fn prune_volatile(&mut self) -> Vec<HashMap<Address, AddressEntry>> {
        let epoch = self.last_persisted_epoch.map(|e| e + 1).unwrap_or(0);
        let blocks_to_drain = (self.epoch_start_block.saturating_sub(self.start_block) as usize)
            .min(self.window.len());

        self.start_block += blocks_to_drain as u64;
        self.last_persisted_epoch = Some(epoch);

        self.window.drain(..blocks_to_drain).collect()
    }
}
