use std::sync::Arc;

use config::Config;

const DEFAULT_STORE_HISTORY: (&str, bool) = ("store-history", false);
const DEFAULT_STORE_RETIRED_POOLS: (&str, bool) = ("store-retired-pools", false);

#[derive(Debug, Clone)]
pub struct StoreConfig {
    pub store_history: bool,
    pub store_retired_pools: bool,
}

impl StoreConfig {
    #[allow(dead_code)]
    pub fn new(store_history: bool, store_retired_pools: bool) -> Self {
        Self {
            store_history,
            store_retired_pools,
        }
    }
}

impl From<Arc<Config>> for StoreConfig {
    fn from(config: Arc<Config>) -> Self {
        Self {
            store_history: config
                .get_bool(DEFAULT_STORE_HISTORY.0)
                .unwrap_or(DEFAULT_STORE_HISTORY.1),
            store_retired_pools: config
                .get_bool(DEFAULT_STORE_RETIRED_POOLS.0)
                .unwrap_or(DEFAULT_STORE_RETIRED_POOLS.1),
        }
    }
}
