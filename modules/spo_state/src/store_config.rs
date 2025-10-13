use std::sync::Arc;

use config::Config;
use serde::Serialize;

const DEFAULT_STORE_EPOCHS_HISTORY: (&str, bool) = ("store-epochs-history", false);
const DEFAULT_STORE_RETIRED_POOLS: (&str, bool) = ("store-retired-pools", false);
const DEFAULT_STORE_REGISTRATION: (&str, bool) = ("store-registration", false);
const DEFAULT_STORE_UPDATES: (&str, bool) = ("store-updates", false);
const DEFAULT_STORE_DELEGATORS: (&str, bool) = ("store-delegators", false);
const DEFAULT_STORE_VOTES: (&str, bool) = ("store-votes", false);
const DEFAULT_STORE_BLOCKS: (&str, bool) = ("store-blocks", false);
const DEFAULT_STORE_STAKE_ADDRESSES: (&str, bool) = ("store-stake-addresses", false);

#[derive(Default, Debug, Clone, Serialize)]
pub struct StoreConfig {
    pub store_epochs_history: bool,
    pub store_retired_pools: bool,
    pub store_registration: bool,
    pub store_updates: bool,
    pub store_delegators: bool,
    pub store_votes: bool,
    pub store_blocks: bool,
    pub store_stake_addresses: bool,
}

impl StoreConfig {
    pub fn new(
        store_epochs_history: bool,
        store_retired_pools: bool,
        store_registration: bool,
        store_updates: bool,
        store_delegators: bool,
        store_votes: bool,
        store_blocks: bool,
        store_stake_addresses: bool,
    ) -> Self {
        Self {
            store_epochs_history,
            store_retired_pools,
            store_registration,
            store_updates,
            store_delegators,
            store_votes,
            store_blocks,
            store_stake_addresses,
        }
    }

    pub fn store_historical_state(&self) -> bool {
        self.store_registration
            || self.store_updates
            || self.store_delegators
            || self.store_votes
            || self.store_blocks
    }
}

impl From<Arc<Config>> for StoreConfig {
    fn from(config: Arc<Config>) -> Self {
        Self {
            store_epochs_history: config
                .get_bool(DEFAULT_STORE_EPOCHS_HISTORY.0)
                .unwrap_or(DEFAULT_STORE_EPOCHS_HISTORY.1),
            store_retired_pools: config
                .get_bool(DEFAULT_STORE_RETIRED_POOLS.0)
                .unwrap_or(DEFAULT_STORE_RETIRED_POOLS.1),
            store_registration: config
                .get_bool(DEFAULT_STORE_REGISTRATION.0)
                .unwrap_or(DEFAULT_STORE_REGISTRATION.1),
            store_updates: config
                .get_bool(DEFAULT_STORE_UPDATES.0)
                .unwrap_or(DEFAULT_STORE_UPDATES.1),
            store_delegators: config
                .get_bool(DEFAULT_STORE_DELEGATORS.0)
                .unwrap_or(DEFAULT_STORE_DELEGATORS.1),
            store_votes: config.get_bool(DEFAULT_STORE_VOTES.0).unwrap_or(DEFAULT_STORE_VOTES.1),
            store_blocks: config.get_bool(DEFAULT_STORE_BLOCKS.0).unwrap_or(DEFAULT_STORE_BLOCKS.1),
            store_stake_addresses: config
                .get_bool(DEFAULT_STORE_STAKE_ADDRESSES.0)
                .unwrap_or(DEFAULT_STORE_STAKE_ADDRESSES.1),
        }
    }
}
