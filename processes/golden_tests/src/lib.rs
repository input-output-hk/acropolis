// Everything in this process is used for testing, don't accidentally include in production builds
#![cfg(test)]
use std::sync::Arc;

use anyhow::Result;

use acropolis_common::messages::Message;
use acropolis_module_snapshot_bootstrapper::SnapshotBootstrapper;
use acropolis_module_spo_state::SPOState;
use acropolis_module_tx_unpacker::TxUnpacker;
use caryatid_process::Process;
use config::{Config, Environment, File};
use test_module::TestModule;

mod test_module;

#[tokio::test]
async fn test() -> Result<()> {
    let config = Arc::new(
        Config::builder()
            .add_source(File::with_name("golden"))
            .add_source(Environment::with_prefix("TEST_ACROPOLIS"))
            // TODO: we should use set_override to provide test information in the configuration
            .build()
            .unwrap(),
    );

    let mut process = Process::<Message>::create(config).await;

    SnapshotBootstrapper::register(&mut process);
    TxUnpacker::register(&mut process);
    TestModule::register(&mut process);
    SPOState::register(&mut process);

    process.run().await?;

    Ok(())
}
