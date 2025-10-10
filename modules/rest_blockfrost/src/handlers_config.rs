use std::sync::Arc;

use acropolis_common::queries::{
    accounts::DEFAULT_ACCOUNTS_QUERY_TOPIC,
    assets::{DEFAULT_ASSETS_QUERY_TOPIC, DEFAULT_OFFCHAIN_TOKEN_REGISTRY_URL},
    blocks::DEFAULT_BLOCKS_QUERY_TOPIC,
    epochs::DEFAULT_EPOCHS_QUERY_TOPIC,
    governance::{DEFAULT_DREPS_QUERY_TOPIC, DEFAULT_GOVERNANCE_QUERY_TOPIC},
    parameters::DEFAULT_PARAMETERS_QUERY_TOPIC,
    pools::DEFAULT_POOLS_QUERY_TOPIC,
    spdd::DEFAULT_SPDD_QUERY_TOPIC,
};
use config::Config;

const DEFAULT_EXTERNAL_API_TIMEOUT: (&str, i64) = ("external_api_timeout", 3); // 3 seconds

#[derive(Clone)]
pub struct HandlersConfig {
    pub accounts_query_topic: String,
    pub assets_query_topic: String,
    pub blocks_query_topic: String,
    pub pools_query_topic: String,
    pub dreps_query_topic: String,
    pub governance_query_topic: String,
    pub epochs_query_topic: String,
    pub spdd_query_topic: String,
    pub parameters_query_topic: String,
    pub external_api_timeout: u64,
    pub offchain_token_registry_url: String,
}

impl From<Arc<Config>> for HandlersConfig {
    fn from(config: Arc<Config>) -> Self {
        let accounts_query_topic = config
            .get_string(DEFAULT_ACCOUNTS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_ACCOUNTS_QUERY_TOPIC.1.to_string());

        let assets_query_topic = config
            .get_string(DEFAULT_ASSETS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_ASSETS_QUERY_TOPIC.1.to_string());

        let blocks_query_topic = config
            .get_string(DEFAULT_BLOCKS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_BLOCKS_QUERY_TOPIC.1.to_string());

        let pools_query_topic = config
            .get_string(DEFAULT_POOLS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_POOLS_QUERY_TOPIC.1.to_string());

        let dreps_query_topic = config
            .get_string(DEFAULT_DREPS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_DREPS_QUERY_TOPIC.1.to_string());

        let governance_query_topic = config
            .get_string(DEFAULT_GOVERNANCE_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_GOVERNANCE_QUERY_TOPIC.1.to_string());

        let epochs_query_topic = config
            .get_string(DEFAULT_EPOCHS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_EPOCHS_QUERY_TOPIC.1.to_string());

        let parameters_query_topic = config
            .get_string(DEFAULT_PARAMETERS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_PARAMETERS_QUERY_TOPIC.1.to_string());

        let spdd_query_topic = config
            .get_string(DEFAULT_SPDD_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_SPDD_QUERY_TOPIC.1.to_string());

        let external_api_timeout = config
            .get_int(DEFAULT_EXTERNAL_API_TIMEOUT.0)
            .unwrap_or(DEFAULT_EXTERNAL_API_TIMEOUT.1) as u64;

        let offchain_token_registry_url = config
            .get_string(DEFAULT_OFFCHAIN_TOKEN_REGISTRY_URL.0)
            .unwrap_or(DEFAULT_OFFCHAIN_TOKEN_REGISTRY_URL.1.to_string());

        Self {
            accounts_query_topic,
            assets_query_topic,
            blocks_query_topic,
            pools_query_topic,
            dreps_query_topic,
            governance_query_topic,
            epochs_query_topic,
            spdd_query_topic,
            parameters_query_topic,
            external_api_timeout,
            offchain_token_registry_url,
        }
    }
}
