use acropolis_common::{
    messages::{
        EpochActivityMessage, SPORewardsMessage, SPOStakeDistributionMessage, TxCertificatesMessage,
    },
    BlockInfo, BlockStatus, Era, TxCertificate,
};

use crate::state_config::StateConfig;

pub fn default_state_config() -> StateConfig {
    StateConfig {
        store_history: false,
        store_retired_pools: false,
    }
}

pub fn save_history_state_config() -> StateConfig {
    StateConfig {
        store_history: true,
        store_retired_pools: false,
    }
}

pub fn save_retired_pools_state_config() -> StateConfig {
    StateConfig {
        store_history: false,
        store_retired_pools: true,
    }
}

pub fn save_all_state_config() -> StateConfig {
    StateConfig {
        store_history: true,
        store_retired_pools: true,
    }
}

pub fn new_block(epoch: u64) -> BlockInfo {
    BlockInfo {
        status: BlockStatus::Immutable,
        slot: 0,
        number: 10 * epoch,
        hash: Vec::<u8>::new(),
        epoch,
        new_epoch: true,
        era: Era::Byron,
    }
}

pub fn new_certs_msg() -> TxCertificatesMessage {
    TxCertificatesMessage {
        certificates: Vec::<TxCertificate>::new(),
    }
}

pub fn new_spdd_message(epoch: u64) -> SPOStakeDistributionMessage {
    SPOStakeDistributionMessage {
        spos: Vec::new(),
        epoch,
    }
}

pub fn new_epoch_activity_message(epoch: u64) -> EpochActivityMessage {
    EpochActivityMessage {
        epoch,
        total_blocks: 0,
        total_fees: 0,
        vrf_vkey_hashes: Vec::new(),
    }
}

pub fn new_spo_rewards_message(epoch: u64) -> SPORewardsMessage {
    SPORewardsMessage {
        spos: Vec::new(),
        epoch,
    }
}
