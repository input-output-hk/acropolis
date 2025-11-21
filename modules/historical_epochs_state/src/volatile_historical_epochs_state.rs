use acropolis_common::{messages::EpochActivityMessage, BlockInfo};

#[derive(Default, Debug, Clone)]
pub struct VolatileHistoricalEpochsState {
    pub block_number: u64,

    pub volatile_ea: Option<EpochActivityMessage>,

    pub last_persisted_epoch: Option<u64>,

    pub security_param_k: u64,
}

impl VolatileHistoricalEpochsState {
    pub fn new() -> Self {
        Self {
            block_number: 0,
            volatile_ea: None,
            last_persisted_epoch: None,
            security_param_k: 0,
        }
    }

    pub fn rollback_before(&mut self, rollbacked_block: u64) -> Option<EpochActivityMessage> {
        if self.block_number >= rollbacked_block {
            std::mem::take(&mut self.volatile_ea)
        } else {
            None
        }
    }

    pub fn handle_new_epoch(&mut self, block_info: &BlockInfo, ea: &EpochActivityMessage) {
        self.block_number = block_info.number;
        self.volatile_ea = Some(ea.clone());
    }

    pub fn prune_volatile(&mut self) -> Option<EpochActivityMessage> {
        if let Some(ea) = self.volatile_ea.as_ref() {
            self.last_persisted_epoch = Some(ea.epoch);
            std::mem::take(&mut self.volatile_ea)
        } else {
            None
        }
    }

    pub fn update_k(&mut self, k: u32) {
        self.security_param_k = k as u64;
    }

    pub fn get_volatile_epoch(&self, epoch: u64) -> Option<EpochActivityMessage> {
        if let Some(ea) = self.volatile_ea.as_ref() {
            if ea.epoch == epoch {
                return Some(ea.clone());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use acropolis_common::{BlockHash, BlockIntent, BlockStatus, Era};

    use super::*;

    #[test]
    fn test_rollback_before() {
        let mut state = VolatileHistoricalEpochsState::new();
        let block_info = BlockInfo {
            number: 1,
            epoch: 1,
            status: BlockStatus::Volatile,
            intent: BlockIntent::Apply,
            slot: 1,
            hash: BlockHash::default(),
            epoch_slot: 1,
            new_epoch: false,
            timestamp: 1,
            era: Era::Shelley,
        };
        let ea = EpochActivityMessage {
            epoch: 1,
            epoch_start_time: 1,
            epoch_end_time: 2,
            total_blocks: 100,
            total_txs: 100,
            total_outputs: 100,
            total_fees: 100,
            spo_blocks: vec![],
            nonce: None,
            first_block_time: 1,
            first_block_height: 1,
            last_block_time: 1,
            last_block_height: 1,
        };
        state.handle_new_epoch(&block_info, &ea);
        assert!(state.get_volatile_epoch(1).unwrap().eq(&ea));
        assert_eq!(state.get_volatile_epoch(2), None);

        let rollbacked = state.rollback_before(2);
        assert_eq!(rollbacked, None);

        let rollbacked = state.rollback_before(1);
        assert!(rollbacked.unwrap().eq(&ea));
        assert_eq!(state.get_volatile_epoch(1), None);
    }
}
