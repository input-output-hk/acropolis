use acropolis_common::{commands::chain_sync::Point, hash::Hash, messages::Message};
use anyhow::Result;
use caryatid_process::Process;
use caryatid_sdk::module_registry::ModuleRegistry;
use clap::Parser;
use config::{Config, Environment, File};
use std::{collections::BTreeMap, str::FromStr, sync::Arc};
use tokio::sync::watch;

mod indices;

use acropolis_module_block_unpacker::BlockUnpacker;
use acropolis_module_custom_indexer::{
    chain_indexer::CustomIndexer, cursor_store::InMemoryCursorStore,
};
use acropolis_module_genesis_bootstrapper::GenesisBootstrapper;
use acropolis_module_peer_network_interface::PeerNetworkInterface;

use crate::indices::pool_cost_index::{PoolCostIndex, PoolCostState};

#[derive(Debug, clap::Parser)]
struct Args {
    #[arg(long, value_name = "PATH", default_value = "indexer.toml")]
    config: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Get arguments and config
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

    // Core modules to fetch blocks and publish decoded transactions
    GenesisBootstrapper::register(&mut process);
    BlockUnpacker::register(&mut process);
    PeerNetworkInterface::register(&mut process);

    // watch channel to send latest state to downstream process on index change
    let (sender, receiver) = watch::channel(PoolCostState {
        pools: BTreeMap::new(),
    });

    // Example receiver (This would likely be provided in initialization of a new module)
    {
        let mut rx = receiver.clone();
        tokio::spawn(async move {
            while rx.changed().await.is_ok() {
                let snapshot = rx.borrow().clone();
                tracing::info!("New PoolCostIndex state: {:?}", snapshot.pools);
            }
        });
    }

    // Initialize and register indexer
    let shelley_start = Point::Specific(
        16588737,
        Hash::from_str("4e9bbbb67e3ae262133d94c3da5bffce7b1127fc436e7433b87668dba34c354a")?,
    );
    let indexer = CustomIndexer::new(
        PoolCostIndex::new(sender),
        InMemoryCursorStore::new(shelley_start.clone()),
        shelley_start,
    );
    process.register(Arc::new(indexer));

    process.run().await?;

    Ok(())
}
