//! Integration test: send a Loc over the Caryatid bus and resolve it.
//! Everything in this process is used for testing, don't accidentally include in production builds
//! TODO: this could be broken into three parts: subscriber module, publisher module, and the test itself.
#![cfg(test)]
use std::{
    sync::{Arc, Mutex, OnceLock},
    time::Duration,
};

use anyhow::Result;
use caryatid_sdk::{module, Context, Module};
use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use tokio::{sync::watch, time::timeout};
use tracing::info;

use acropolis_common::resolver::{Loc, ObjectId, Region, Registry, Resolver, StoreId};
use caryatid_process::Process;
use config::{Config, Environment, File};

// --------- shared test completion signaling ---------
static TEST_COMPLETION_TX: Mutex<Option<watch::Sender<bool>>> = Mutex::new(None);
pub fn signal_test_completion() {
    if let Ok(tx) = TEST_COMPLETION_TX.lock() {
        if let Some(sender) = tx.as_ref() {
            let _ = sender.send(true);
        }
    }
}

// --------- shared registry (test process local) ----------
static REGISTRY: OnceLock<Arc<Registry>> = OnceLock::new();
fn registry() -> Arc<Registry> {
    REGISTRY.get().cloned().expect("registry not set")
}

// ---------- typed bus message carrying our Loc ----------
#[derive(Clone, Debug, Serialize, Default, Deserialize, PartialEq)]
enum BusMsg {
    #[default]
    None, // Just so we have a simple default

    Loc(Loc),
    Ack(String), // response back to publisher
}

/// Typed subscriber module
#[module(
    message_type(BusMsg),
    name = "subscriber",
    description = "Typed subscriber module"
)]
struct Subscriber;

impl Subscriber {
    // Implement the single initialisation function, with application
    async fn init(&self, context: Arc<Context<BusMsg>>, config: Arc<Config>) -> Result<()> {
        let subscribe_topic = config.get_string("topic").unwrap_or("sample".to_string());
        let ack_topic = format!("{}.ack", subscribe_topic);
        let mut sub = context.subscribe(&subscribe_topic).await?;
        info!("Creating subscriber on '{}'", subscribe_topic);
        // Let this run async
        let ctx = context.clone();
        ctx.run(async move {
            while let Ok((_, msg)) = sub.read().await {
                match &*msg {
                    BusMsg::Loc(loc) => {
                        let res = Resolver::new(&registry()).resolve(loc);
                        match res {
                            Ok(view) => {
                                // touch the bytes so we know mapping worked
                                let slice = view.as_slice();
                                // trivial check: non-empty
                                if !slice.is_empty() {
                                    context
                                        .publish(
                                            &ack_topic,
                                            Arc::new(BusMsg::Ack("ok".to_string())),
                                        )
                                        .await
                                        .expect("Failed to publish ACK");
                                } else {
                                    context
                                        .publish(
                                            &ack_topic,
                                            Arc::new(BusMsg::Ack("empty".to_string())),
                                        )
                                        .await
                                        .expect("Failed to publish ACK");
                                }
                                break; // test done
                            }
                            Err(_) => {
                                context
                                    .publish(
                                        &ack_topic,
                                        Arc::new(BusMsg::Ack("resolve_err".to_string())),
                                    )
                                    .await
                                    .expect("Failed to publish ACK");
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
        });
        Ok(())
    }
}

/// Typed publisher module
#[module(
    message_type(BusMsg),
    name = "publisher",
    description = "Typed publisher module"
)]
pub struct Publisher;

impl Publisher {
    // super::signal_test_completion();
    // Implement the single initialisation function, with application
    async fn init(&self, context: Arc<Context<BusMsg>>, config: Arc<Config>) -> Result<()> {
        let message_bus = context.message_bus.clone();

        // Get configuration
        let topic = config.get_string("topic").unwrap_or("sample".to_string());

        // Subscribe for the ACK *before* publishing to avoid races.
        let mut ack_sub = context.subscribe(&format!("{}.ack", topic)).await?;

        info!("Creating publisher on '{}'", topic);

        // Send test messages to the message bus on 'sample_topic'
        // Let this run async
        context.run(async move {
            // Custom struct
            let message = BusMsg::Loc(Loc {
                store: StoreId(1),
                object: ObjectId(0xFEED_CAFE_BEEF),
                region: Region {
                    offset: 100,
                    len: 40,
                },
                inline: None,
            });
            info!("Sending {:?}", message);
            message_bus
                .publish(&topic, Arc::new(message))
                .await
                .expect("Failed to publish message");
            // Wait for ACK from Publisher and then signal completion of test.
            while let Ok((_, message)) = ack_sub.read().await {
                if let BusMsg::Ack(ref s) = *message {
                    if s == "ok" {
                        // we're done!
                        signal_test_completion();
                        break;
                    } else {
                        panic!("Unexpected ACK message: {}", s);
                    }
                }
            }
        });
        Ok(())
    }
}

// -------------- the test --------------
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn loc_round_trip_over_caryatid() -> Result<()> {
    // 0) Prepare backing bytes on disk and register them.
    let tmp = NamedTempFile::new()?;
    let bytes: Vec<u8> = (0u8..=255).collect(); // 256 bytes
    std::fs::write(tmp.path(), &bytes)?;
    // reopen read-only for mmap stability
    let file = std::fs::File::open(tmp.path())?;

    let reg = Arc::new(Registry::default());
    reg.register_file(StoreId(1), ObjectId(0xFEED_CAFE_BEEF), &file)?;
    REGISTRY.set(reg.clone()).ok();

    // Read the config
    let config = Arc::new(
        Config::builder()
            .add_source(File::with_name("test"))
            .add_source(Environment::with_prefix("CARYATID"))
            .build()
            .unwrap(),
    );

    let (completion_tx, mut completion_rx) = watch::channel(false);

    {
        let mut tx = TEST_COMPLETION_TX.lock().unwrap();
        *tx = Some(completion_tx);
    }

    // Create the process
    let mut process = Process::<BusMsg>::create(config).await;

    // Register modules
    Subscriber::register(&mut process);
    Publisher::register(&mut process);

    // Run the process (this will run until we signal completion)
    // We wrap this in a timeout to avoid hanging the test indefinitely

    match timeout(Duration::from_secs(5), async {
        tokio::select! {
            // run everything
            result = process.run() => {
                result
            }
            _ = completion_rx.changed() => {
                Ok(())
            }
        }
    })
    .await
    {
        Ok(result) => result?,
        Err(_) => {
            panic!("Test timed out after 5 seconds");
        }
    }
    Ok(())
}
