//! Acropolis epoch activity counter: state storage

use std::collections::BTreeMap;
use acropolis_common::BlockInfo;
use tracing::info;

pub struct State {

    // Map of block numbers to VRF vkeys (identifying SPOs)
    vrf_vkeys: BTreeMap::<u64, Vec<u8>>,

    // Total fees seen this epoch
    total_fees: u64,
}

impl State {
    // Constructor
    pub fn new() -> Self {
        Self {
            vrf_vkeys: BTreeMap::new(),
            total_fees: 0,
        }
    }

    // Handle a block minting, taking the SPO's VRF vkey
    pub fn handle_mint(&mut self, block: &BlockInfo, vrf_vkey: &[u8]) {
        self.vrf_vkeys.insert(block.number, vrf_vkey.to_vec());
    }

    // Handle block fees
    pub fn handle_fees(&mut self, _block: &BlockInfo, total_fees: u64) {
        self.total_fees += total_fees;
    }

    // Handle end of epoch
    pub fn end_epoch(&mut self, block: &BlockInfo) {
        info!("End of epoch {} - {} blocks captured, total fees {}",
              block.epoch-1, self.vrf_vkeys.len(), self.total_fees);
        self.vrf_vkeys.clear();
        self.total_fees = 0;
    }
}
