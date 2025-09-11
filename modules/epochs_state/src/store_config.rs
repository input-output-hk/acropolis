use std::sync::Arc;

use config::Config;

const DEFAULT_STORE_HISTORY: (&str, bool) = ("store-history", false);
const DEFAULT_STORE_BLOCK_HAHSES: (&str, bool) = ("store-block-hashes", false);

#[derive(Default, Debug, Clone)]
pub struct StoreConfig {
    pub store_history: bool,
    pub store_block_hashes: bool,
}

impl StoreConfig {
    #[allow(dead_code)]
    pub fn new(store_history: bool, store_block_hashes: bool) -> Self {
        Self {
            store_history,
            store_block_hashes,
        }
    }
}

impl From<Arc<Config>> for StoreConfig {
    fn from(config: Arc<Config>) -> Self {
        Self {
            store_history: config
                .get_bool(DEFAULT_STORE_HISTORY.0)
                .unwrap_or(DEFAULT_STORE_HISTORY.1),
            store_block_hashes: config
                .get_bool(DEFAULT_STORE_BLOCK_HAHSES.0)
                .unwrap_or(DEFAULT_STORE_BLOCK_HAHSES.1),
        }
    }
}
