use std::sync::Arc;

use config::Config;

const DEFAULT_STORE_HISTORY: (&str, bool) = ("store-history", false);

#[derive(Default, Debug, Clone)]
pub struct StoreConfig {
    pub store_history: bool,
}

impl StoreConfig {
    #[allow(dead_code)]
    pub fn new(store_history: bool) -> Self {
        Self { store_history }
    }
}

impl From<Arc<Config>> for StoreConfig {
    fn from(config: Arc<Config>) -> Self {
        Self {
            store_history: config
                .get_bool(DEFAULT_STORE_HISTORY.0)
                .unwrap_or(DEFAULT_STORE_HISTORY.1),
        }
    }
}
