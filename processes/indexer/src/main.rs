use acropolis_common::{hash::Hash, messages::Message, Point};
use anyhow::Result;
use caryatid_process::Process;
use caryatid_sdk::module_registry::ModuleRegistry;
use clap::Parser;
use config::{Config, Environment, File};
use std::{collections::BTreeMap, str::FromStr, sync::Arc};
use tokio::sync::watch;

use acropolis_module_block_unpacker::BlockUnpacker;
use acropolis_module_custom_indexer::chain_indexer::CustomIndexer;
use acropolis_module_genesis_bootstrapper::GenesisBootstrapper;
use acropolis_module_peer_network_interface::PeerNetworkInterface;

mod indices;

#[allow(unused_imports)]
use crate::indices::fjall_pool_cost_index::{FjallPoolCostIndex, FjallPoolCostState};
#[allow(unused_imports)]
use crate::indices::in_memory_pool_cost_index::{InMemoryPoolCostIndex, InMemoryPoolCostState};
#[allow(unused_imports)]
use acropolis_module_custom_indexer::cursor_store::{FjallCursorStore, InMemoryCursorStore};

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

    // watch channel to send latest state to consumer on index change
    let (sender, receiver) = watch::channel(FjallPoolCostState {
        pools: BTreeMap::new(),
    });

    // Example receiver
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
    let shelley_start = Point::Specific {
        hash: Hash::from_str("4e9bbbb67e3ae262133d94c3da5bffce7b1127fc436e7433b87668dba34c354a")?,
        slot: 16588737,
    };

    // Fjall backed example indexer
    let indexer = CustomIndexer::new(
        FjallPoolCostIndex::new("fjall-pool-cost-index", sender)?,
        FjallCursorStore::new("fjall-cursor-store", shelley_start.clone())?,
        shelley_start,
    );

    // In memory example indexer
    /*
    let indexer = CustomIndexer::new(
        InMemoryPoolCostIndex::new(sender),
        InMemoryCursorStore::new(shelley_start.clone()),
        shelley_start,
    );
    */

    process.register(Arc::new(indexer));
    process.run().await?;

    Ok(())
}
