use acropolis_common::messages::Message;
use acropolis_module_indexer::Indexer;
use anyhow::Result;
use caryatid_process::Process;
use clap::Parser;
use config::{Config, Environment, File};
use std::sync::Arc;

use acropolis_module_block_unpacker::BlockUnpacker;
use acropolis_module_genesis_bootstrapper::GenesisBootstrapper;
use acropolis_module_peer_network_interface::PeerNetworkInterface;

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(long, value_name = "PATH", default_value = "indexer.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::fmt().with_env_filter("info").init();

    let config = Arc::new(
        Config::builder()
            .add_source(File::with_name(&args.config))
            .add_source(Environment::with_prefix("ACROPOLIS"))
            .build()
            .unwrap(),
    );

    let mut process = Process::<Message>::create(config).await;

    GenesisBootstrapper::register(&mut process);
    BlockUnpacker::register(&mut process);
    PeerNetworkInterface::register(&mut process);
    Indexer::register(&mut process);

    process.run().await?;
    Ok(())
}
