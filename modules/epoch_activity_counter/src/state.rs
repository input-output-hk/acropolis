//! Acropolis epoch activity counter: state storage

use acropolis_common::{BlockInfo, messages::{Message, EpochActivityMessage}};
use std::sync::Arc;
use tracing::info;
use std::collections::HashMap;

pub struct State {

    // Map of counts by VRF keys
    vrf_vkeys: HashMap<Vec<u8>, usize>,

    // Total blocks seen this epoch
    total_blocks: usize,

    // Total fees seen this epoch
    total_fees: u64,
}

impl State {
    // Constructor
    pub fn new() -> Self {
        Self {
            vrf_vkeys: HashMap::new(),
            total_blocks: 0,
            total_fees: 0,
        }
    }

    // Handle a block minting, taking the SPO's VRF vkey
    pub fn handle_mint(&mut self, _block: &BlockInfo, vrf_vkey: &[u8]) {
        self.total_blocks += 1;
        *(self.vrf_vkeys.entry(vrf_vkey.to_vec()).or_insert(0)) += 1;
    }

    // Handle block fees
    pub fn handle_fees(&mut self, _block: &BlockInfo, total_fees: u64) {
        self.total_fees += total_fees;
    }

    // Handle end of epoch, returns message to be published
    pub fn end_epoch(&mut self, block: &BlockInfo) -> Arc<Message> {
        info!("End of epoch {} - {} total blocks, {} unique SPOs, total fees {}",
              block.epoch-1, self.total_blocks, self.vrf_vkeys.len(),
              self.total_fees);

        let message = Arc::new(Message::EpochActivity(EpochActivityMessage {
            block: block.clone(),
            total_blocks: self.total_blocks,
            total_fees: self.total_fees,
            vrf_vkeys: self.vrf_vkeys.drain().collect(),
        }));

        self.total_blocks = 0;
        self.total_fees = 0;

        message
    }
}
