use acropolis_common::messages::Message;
use anyhow::Result;
use caryatid_sdk::{module, Context};
use config::Config;
use std::sync::Arc;
use tracing::{error, info};

const DEFAULT_CLOCK_TICK_SUBSCRIBE_TOPIC: (&str, &str) =
    ("clock-tick-subscribe-topic", "clock.tick");

#[module(message_type(Message), name = "stats", description = "Logs statistics")]
pub struct Stats;

impl Stats {
    pub async fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        let clock_tick_subscribe_topic = config
            .get_string(DEFAULT_CLOCK_TICK_SUBSCRIBE_TOPIC.0)
            .unwrap_or(DEFAULT_CLOCK_TICK_SUBSCRIBE_TOPIC.1.to_string());
        info!("Creating subscriber on '{clock_tick_subscribe_topic}'");
        let mut clock_tick_subscription = context.subscribe(&clock_tick_subscribe_topic).await?;
        context.run(async move {
            loop {
                let Ok((_, tick_message)) = clock_tick_subscription.read().await else {
                    error!("Failed to run Stats clock tick subscription");
                    continue;
                };
                if let Message::Clock(tick_message) = tick_message.as_ref() {
                    if tick_message.number.is_multiple_of(60) {
                        Self::log_stats().await;
                    }
                }
            }
        });
        Ok(())
    }

    async fn log_stats() {
        #[cfg(not(target_env = "msvc"))]
        {
            // The jemalloc epoch must be advanced to flush any cached stats
            let Ok(j_epoch) = tikv_jemalloc_ctl::epoch::mib() else {
                error!("failed to get jemalloc epoch");
                return;
            };
            if j_epoch.advance().is_err() {
                error!("failed to advance jemalloc epoch");
                return;
            }

            let Ok(allocated) = tikv_jemalloc_ctl::stats::allocated::read() else {
                error!("failed to read allocated");
                return;
            };
            let Ok(active) = tikv_jemalloc_ctl::stats::active::read() else {
                error!("failed to read active");
                return;
            };
            let Ok(resident) = tikv_jemalloc_ctl::stats::resident::read() else {
                error!("failed to read resident");
                return;
            };
            let Ok(mapped) = tikv_jemalloc_ctl::stats::mapped::read() else {
                error!("failed to read mapped");
                return;
            };
            info!(allocated, active, resident, mapped, "Memory usage");
        }
    }
}
