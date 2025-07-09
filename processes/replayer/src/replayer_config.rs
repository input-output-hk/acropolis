use config::Config;
use std::sync::Arc;
use tracing::info;

const DEFAULT_PATH: (&str, &str) = ("path", "governance-logs");
const DEFAULT_SUBSCRIBE_TOPIC: (&str, &str) = ("subscribe-topic", "cardano.governance");
const DEFAULT_DREP_DISTRIBUTION_TOPIC: (&str, &str) =
    ("stake-drep-distribution-topic", "cardano.drep.distribution");
const DEFAULT_SPO_DISTRIBUTION_TOPIC: (&str, &str) =
    ("stake-spo-distribution-topic", "cardano.spo.distribution");

pub struct ReplayerConfig {
    pub path: String,
    pub subscribe_topic: String,
    pub drep_distribution_topic: String,
    pub spo_distribution_topic: String,
}

impl ReplayerConfig {
    fn conf(config: &Arc<Config>, keydef: (&str, &str)) -> String {
        let actual = config.get_string(keydef.0).unwrap_or(keydef.1.to_string());
        info!("Creating subscriber on '{}' for {}", actual, keydef.0);
        actual
    }

    pub fn new(config: &Arc<Config>) -> Arc<Self> {
        Arc::new(Self {
            path: Self::conf(config, DEFAULT_PATH),
            subscribe_topic: Self::conf(config, DEFAULT_SUBSCRIBE_TOPIC),
            drep_distribution_topic: Self::conf(config, DEFAULT_DREP_DISTRIBUTION_TOPIC),
            spo_distribution_topic: Self::conf(config, DEFAULT_SPO_DISTRIBUTION_TOPIC),
        })
    }

    /// Returns of subscription topics: topic, prefix, is-epoch-wise, do-skip-epoch-0
    pub fn get_topics_vec(&self) -> Arc<Vec<(String,String,bool,bool)>> {
        Arc::new(vec![
            (self.subscribe_topic.to_string(), "gov".to_string(), false, false),
            (self.drep_distribution_topic.to_string(), "drep".to_string(), true, true),
            (self.spo_distribution_topic.to_string(), "spo".to_string(), true, true)
        ])
    }
}
