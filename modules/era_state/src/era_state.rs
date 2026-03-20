//! Acropolis era state module for Caryatid.
//!
//! Loads era history data from bundled JSON (per network) or a custom file,
//! subscribes to the genesis completion message to obtain the system start time,
//! and publishes the combined [`EraHistory`] on `cardano.era.history`.

use acropolis_common::{
    era_history::EraHistory,
    messages::{CardanoMessage, EraHistoryMessage, GenesisCompleteMessage, Message},
};
use anyhow::Result;
use caryatid_sdk::{module, Context};
use config::Config;
use std::{borrow::Cow, time::SystemTime};
use std::{sync::Arc, time::Duration};
use tracing::{error, info, info_span, Instrument};

const DEFAULT_STARTUP_TOPIC: &str = "cardano.sequence.start";
const DEFAULT_GENESIS_COMPLETION_TOPIC: &str = "cardano.sequence.bootstrapped";
const DEFAULT_PUBLISH_TOPIC: &str = "cardano.era.history";
const DEFAULT_NETWORK_NAME: &str = "mainnet";

const MAINNET_ERA_HISTORY: &str = include_str!("../data/mainnet-era-history.json");
const PREPROD_ERA_HISTORY: &str = include_str!("../data/preprod-era-history.json");
const PREVIEW_ERA_HISTORY: &str = include_str!("../data/preview-era-history.json");
const SANCHONET_ERA_HISTORY: &str = include_str!("../data/sanchonet-era-history.json");

/// Era state module — publishes era history at startup.
#[module(
    message_type(Message),
    name = "era-state",
    description = "Era history publisher"
)]
pub struct EraState;

/// Load era history JSON for the given network name, or from a custom file path.
fn load_era_history(network_name: &str, config: &Config) -> Result<EraHistory, String> {
    let json_str: Cow<'static, str> = match network_name {
        "mainnet" => Cow::Borrowed(MAINNET_ERA_HISTORY),
        "preprod" => Cow::Borrowed(PREPROD_ERA_HISTORY),
        "preview" => Cow::Borrowed(PREVIEW_ERA_HISTORY),
        "sanchonet" => Cow::Borrowed(SANCHONET_ERA_HISTORY),
        _ => {
            let path = config
                .get_string("era-history-file")
                .map_err(|_| format!("no bundled era history for network '{network_name}'; set era-history-file for custom networks"))?;
            let content = std::fs::read_to_string(&path)
                .map_err(|e| format!("cannot read era history file {path}: {e}"))?;
            Cow::Owned(content)
        }
    };

    let history: EraHistory =
        serde_json::from_str(&json_str).map_err(|e| format!("invalid era history JSON: {e}"))?;

    history.validate().map_err(|e| format!("era history validation failed: {e}"))?;

    Ok(history)
}

impl EraState {
    /// Main init function.
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let startup_topic =
            config.get_string("startup-topic").unwrap_or(DEFAULT_STARTUP_TOPIC.to_string());
        let genesis_completion_topic = config
            .get_string("genesis-completion-topic")
            .unwrap_or(DEFAULT_GENESIS_COMPLETION_TOPIC.to_string());
        let publish_topic =
            config.get_string("publish-topic").unwrap_or(DEFAULT_PUBLISH_TOPIC.to_string());
        let network_name =
            config.get_string("startup.network-name").unwrap_or(DEFAULT_NETWORK_NAME.to_string());

        info!("Creating startup subscriber on '{startup_topic}'");
        info!("Will publish era history on '{publish_topic}'");

        let mut startup_sub = context.subscribe(&startup_topic).await?;
        let mut genesis_sub = context.subscribe(&genesis_completion_topic).await?;

        context.clone().run(async move {
            let Ok(_) = startup_sub.read().await else {
                return;
            };
            let span = info_span!("era_state");
            async {
                info!("Received startup message, loading era history for '{network_name}'");

                let mut era_history = match load_era_history(&network_name, &config) {
                    Ok(h) => {
                        info!(
                            eras = h.eras.len(),
                            stability_window = h.stability_window,
                            "Era history loaded"
                        );
                        h
                    }
                    Err(e) => {
                        error!("Failed to load era history: {e}");
                        return;
                    }
                };

                info!("Waiting for genesis completion on '{genesis_completion_topic}'");
                let Ok((_, msg)) = genesis_sub.read().await else {
                    error!("Failed to read genesis completion message");
                    return;
                };

                let (system_start, block_info) = match msg.as_ref() {
                    Message::Cardano((
                        block_info,
                        CardanoMessage::GenesisComplete(GenesisCompleteMessage { values }),
                    )) => {
                        info!(
                            byron_timestamp = values.byron_timestamp,
                            "Received genesis values"
                        );
                        (values.byron_timestamp, block_info.clone())
                    }
                    _ => {
                        error!("Unexpected message on genesis completion topic");
                        return;
                    }
                };

                era_history.system_start =
                    SystemTime::UNIX_EPOCH + Duration::from_secs(system_start);
                println!("era_history: {:?}", era_history);
                let message = Message::Cardano((
                    block_info,
                    CardanoMessage::EraHistory(EraHistoryMessage { era_history }),
                ));

                context
                    .publish(&publish_topic, Arc::new(message))
                    .await
                    .unwrap_or_else(|e| error!("Failed to publish era history: {e}"));

                info!("Published era history on '{publish_topic}'");
            }
            .instrument(span)
            .await;
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use acropolis_common::Era;

    use super::*;

    #[test]
    fn load_mainnet() {
        let config = Config::default();
        let h = load_era_history("mainnet", &config).expect("mainnet loads");
        assert_eq!(h.eras.len(), 7);
        assert_eq!(h.stability_window, 129_600);
    }

    #[test]
    fn load_preprod() {
        let config = Config::default();
        let h = load_era_history("preprod", &config).expect("preprod loads");
        assert_eq!(h.eras.len(), 7);
    }

    #[test]
    fn load_preview() {
        let config = Config::default();
        let h = load_era_history("preview", &config).expect("preview loads");
        assert_eq!(h.eras.len(), 7);
    }

    #[test]
    fn load_sanchonet() {
        let config = Config::default();
        let h = load_era_history("sanchonet", &config).expect("sanchonet loads");
        assert_eq!(h.eras.len(), 1);
        assert_eq!(h.eras[0].params.era_name, Era::Conway);
    }

    #[test]
    fn load_unknown_network_without_config_fails() {
        let config = Config::default();
        let result = load_era_history("unknown-network", &config);
        assert!(result.is_err());
    }
}
