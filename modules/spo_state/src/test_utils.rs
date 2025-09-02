use acropolis_common::{
    messages::{
        EpochActivityMessage, SPORewardsMessage, SPOStakeDistributionMessage, TxCertificatesMessage,
    },
    BlockInfo, BlockStatus, Era, TxCertificate,
};

use crate::store_config::StoreConfig;

pub fn default_store_config() -> StoreConfig {
    StoreConfig {
        store_epochs_history: false,
        store_retired_pools: false,
        store_registration: false,
        store_updates: false,
        store_delegators: false,
        store_votes: false,
    }
}

pub fn save_history_store_config() -> StoreConfig {
    StoreConfig {
        store_epochs_history: true,
        store_retired_pools: false,
        store_registration: false,
        store_updates: false,
        store_delegators: false,
        store_votes: false,
    }
}

pub fn save_retired_pools_store_config() -> StoreConfig {
    StoreConfig {
        store_epochs_history: false,
        store_retired_pools: true,
        store_registration: false,
        store_updates: false,
        store_delegators: false,
        store_votes: false,
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
