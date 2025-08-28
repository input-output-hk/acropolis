use std::sync::Arc;

use acropolis_common::queries::{
    accounts::DEFAULT_ACCOUNTS_QUERY_TOPIC, epochs::DEFAULT_EPOCHS_QUERY_TOPIC,
    parameters::DEFAULT_PARAMETERS_QUERY_TOPIC, pools::DEFAULT_POOLS_QUERY_TOPIC,
};
use config::Config;

#[derive(Clone)]
pub struct QueryTopics {
    pub accounts_query_topic: String,
    pub pools_query_topic: String,
    pub epochs_query_topic: String,
    pub parameters_query_topic: String,
}

impl From<Arc<Config>> for QueryTopics {
    fn from(config: Arc<Config>) -> Self {
        let accounts_query_topic = config
            .get_string(DEFAULT_ACCOUNTS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_ACCOUNTS_QUERY_TOPIC.1.to_string());

        let pools_query_topic = config
            .get_string(DEFAULT_POOLS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_POOLS_QUERY_TOPIC.1.to_string());

        let epochs_query_topic = config
            .get_string(DEFAULT_EPOCHS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_EPOCHS_QUERY_TOPIC.1.to_string());

        let parameters_query_topic = config
            .get_string(DEFAULT_PARAMETERS_QUERY_TOPIC.0)
            .unwrap_or(DEFAULT_PARAMETERS_QUERY_TOPIC.1.to_string());

        Self {
            accounts_query_topic,
            pools_query_topic,
            epochs_query_topic,
            parameters_query_topic,
        }
    }
}
