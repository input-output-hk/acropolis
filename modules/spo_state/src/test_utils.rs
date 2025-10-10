use acropolis_common::{
    messages::{
        EpochActivityMessage, SPORewardsMessage, SPOStakeDistributionMessage, TxCertificatesMessage,
    },
    BlockHash, BlockInfo, BlockStatus, Era, TxCertificate,
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
        store_block_hashes: false,
        store_stake_addresses: false,
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
        store_block_hashes: false,
        store_stake_addresses: false,
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
        store_block_hashes: false,
        store_stake_addresses: false,
    }
}

pub fn save_block_hashes_store_config() -> StoreConfig {
    StoreConfig {
        store_epochs_history: false,
        store_retired_pools: false,
        store_registration: false,
        store_updates: false,
        store_delegators: false,
        store_votes: false,
        store_block_hashes: true,
        store_stake_addresses: false,
    }
}

pub fn new_block(epoch: u64) -> BlockInfo {
    BlockInfo {
        status: BlockStatus::Immutable,
        slot: 0,
        number: 10 * epoch,
        hash: BlockHash::default(),
        epoch,
        epoch_slot: 0,
        new_epoch: true,
        timestamp: epoch,
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
        epoch_start_time: 0,
        epoch_end_time: 0,
        first_block_time: 0,
        first_block_height: 0,
        last_block_time: 0,
        last_block_height: 0,
        total_blocks: 0,
        total_txs: 0,
        total_outputs: 0,
        total_fees: 0,
        vrf_vkey_hashes: Vec::new(),
        nonce: None,
    }
}

pub fn new_spo_rewards_message(epoch: u64) -> SPORewardsMessage {
    SPORewardsMessage {
        spos: Vec::new(),
        epoch,
    }
}
