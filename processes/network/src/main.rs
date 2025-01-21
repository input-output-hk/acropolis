//! 'main' for the Acropolis network process

use caryatid_process::Process;
use anyhow::Result;
use config::{Config, File, Environment};
use tracing::info;
use tracing_subscriber;
use std::sync::Arc;
use acropolis_messages::Message;

// External modules
extern crate miniprotocols;
use miniprotocols::Miniprotocols;

use caryatid_module_clock::Clock;

/// Standard main
#[tokio::main]
pub async fn main() -> Result<()> {

    // Initialise tracing
    tracing_subscriber::fmt::init();

    info!("Acropolis network process");

    // Read the config
    let config = Arc::new(Config::builder()
        .add_source(File::with_name("network"))
        .add_source(Environment::with_prefix("ACROPOLIS"))
        .build()
        .unwrap());

    // Create the process
    let mut process = Process::<Message>::create(config).await;

    // Register modules
    Miniprotocols::<Message>::register(&mut process);
    Clock::<Message>::register(&mut process);

    // Run it
    process.run().await?;

    // Bye!
    info!("Exiting");
    Ok(())
}

