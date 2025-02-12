//! 'main' for the Acropolis omnibus process

use caryatid_process::Process;
use anyhow::Result;
use config::{Config, File, Environment};
use tracing::info;
use tracing_subscriber;
use std::sync::Arc;
use acropolis_messages::Message;

// External modules
use acropolis_module_genesis_bootstrapper::GenesisBootstrapper;
use acropolis_module_miniprotocols::Miniprotocols;
use acropolis_module_block_unpacker::BlockUnpacker;
use acropolis_module_tx_unpacker::TxUnpacker;
use acropolis_module_ledger_state::LedgerState;
use caryatid_module_clock::Clock;

/// Standard main
#[tokio::main]
pub async fn main() -> Result<()> {

    // Initialise tracing
    tracing_subscriber::fmt::init();

    info!("Acropolis omnibus process");

    // Read the config
    let config = Arc::new(Config::builder()
        .add_source(File::with_name("omnibus"))
        .add_source(Environment::with_prefix("ACROPOLIS"))
        .build()
        .unwrap());

    // Create the process
    let mut process = Process::<Message>::create(config).await;

    // Register modules
    GenesisBootstrapper::register(&mut process);
    Miniprotocols::register(&mut process);
    BlockUnpacker::register(&mut process);
    TxUnpacker::register(&mut process);
    LedgerState::register(&mut process);

    Clock::<Message>::register(&mut process);

    // Run it
    process.run().await?;

    // Bye!
    info!("Exiting");
    Ok(())
}

