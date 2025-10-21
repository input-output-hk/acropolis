//! 'main' for the Acropolis omnibus process

use acropolis_common::messages::Message;
use anyhow::Result;
use caryatid_process::Process;
use config::{Config, Environment, File};
use std::sync::Arc;
use tracing::info;
use tracing_subscriber;

// External modules
use acropolis_module_accounts_state::AccountsState;
use acropolis_module_address_state::AddressState;
use acropolis_module_assets_state::AssetsState;
use acropolis_module_block_unpacker::BlockUnpacker;
use acropolis_module_consensus::Consensus;
use acropolis_module_drdd_state::DRDDState;
use acropolis_module_drep_state::DRepState;
use acropolis_module_epochs_state::EpochsState;
use acropolis_module_genesis_bootstrapper::GenesisBootstrapper;
use acropolis_module_governance_state::GovernanceState;
use acropolis_module_mithril_snapshot_fetcher::MithrilSnapshotFetcher;
use acropolis_module_parameters_state::ParametersState;
use acropolis_module_rest_blockfrost::BlockfrostREST;
use acropolis_module_spdd_state::SPDDState;
use acropolis_module_spo_state::SPOState;
use acropolis_module_stake_delta_filter::StakeDeltaFilter;
use acropolis_module_tx_unpacker::TxUnpacker;
use acropolis_module_upstream_chain_fetcher::UpstreamChainFetcher;
use acropolis_module_utxo_state::UTXOState;

use caryatid_module_clock::Clock;
use caryatid_module_rest_server::RESTServer;
use caryatid_module_spy::Spy;

use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::SpanExporter;
use opentelemetry_sdk::trace::SdkTracerProvider;
use tracing_opentelemetry::OpenTelemetryLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::{filter, fmt, EnvFilter, Registry};

#[cfg(not(target_env = "msvc"))]
use tikv_jemallocator::Jemalloc;
#[cfg(not(target_env = "msvc"))]
#[global_allocator]
static GLOBAL: Jemalloc = Jemalloc;

/// Standard main
#[tokio::main]
pub async fn main() -> Result<()> {
    // Standard logging using RUST_LOG for log levels default to INFO for events only
    let fmt_layer = fmt::layer().with_filter(EnvFilter::from_default_env());

    // TODO disabled this filter because it prevents debugging - investigate
    //.add_directive(filter::LevelFilter::INFO.into()))
    //        .with_filter(filter::filter_fn(|meta| meta.is_event()));

    // Only turn on tracing if some OTEL environment variables exist
    if std::env::vars().any(|(name, _)| name.starts_with("OTEL_")) {
        // Send span tracing to opentelemetry
        // Should pick up standard OTEL_* environment variables
        let otel_exporter = SpanExporter::builder().with_tonic().build()?;
        let otel_tracer = SdkTracerProvider::builder()
            .with_batch_exporter(otel_exporter)
            .build()
            .tracer("rust-otel-otlp");
        let otel_layer = OpenTelemetryLayer::new(otel_tracer)
            .with_filter(
                EnvFilter::from_default_env().add_directive(filter::LevelFilter::INFO.into()),
            )
            .with_filter(filter::filter_fn(|meta| meta.is_span()));
        Registry::default().with(fmt_layer).with(otel_layer).init();
    } else {
        Registry::default().with(fmt_layer).init();
    }

    info!("Acropolis omnibus process");

    // Read the config
    let config = Arc::new(
        Config::builder()
            .add_source(File::with_name("omnibus"))
            .add_source(Environment::with_prefix("ACROPOLIS"))
            .build()
            .unwrap(),
    );

    // Create the process
    let mut process = Process::<Message>::create(config).await;

    // Register modules
    GenesisBootstrapper::register(&mut process);
    MithrilSnapshotFetcher::register(&mut process);
    UpstreamChainFetcher::register(&mut process);
    BlockUnpacker::register(&mut process);
    TxUnpacker::register(&mut process);
    UTXOState::register(&mut process);
    SPOState::register(&mut process);
    DRepState::register(&mut process);
    GovernanceState::register(&mut process);
    ParametersState::register(&mut process);
    StakeDeltaFilter::register(&mut process);
    EpochsState::register(&mut process);
    AccountsState::register(&mut process);
    AddressState::register(&mut process);
    AssetsState::register(&mut process);
    BlockfrostREST::register(&mut process);
    SPDDState::register(&mut process);
    DRDDState::register(&mut process);
    Consensus::register(&mut process);

    Clock::<Message>::register(&mut process);
    RESTServer::<Message>::register(&mut process);
    Spy::<Message>::register(&mut process);

    // Run it
    process.run().await?;

    // Bye!
    info!("Exiting");

    Ok(())
}
