// Everything in this process is used for testing, don't accidentally include in production builds
#![cfg(test)]
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Result;

use acropolis_common::messages::Message;
use acropolis_module_snapshot_bootstrapper::SnapshotBootstrapper;
use acropolis_module_spo_state::SPOState;
use acropolis_module_tx_unpacker::TxUnpacker;
use caryatid_process::Process;
use config::{Config, Environment, File};
use test_module::TestModule;
use tokio::{sync::watch, time::timeout};

mod test_module;

static TEST_COMPLETION_TX: Mutex<Option<watch::Sender<bool>>> = Mutex::new(None);

pub fn signal_test_completion() {
    if let Ok(tx) = TEST_COMPLETION_TX.lock() {
        if let Some(sender) = tx.as_ref() {
            let _ = sender.send(true);
        }
    }
}

#[tokio::test]
#[ignore = "Disabled test pending fix"]
async fn golden_test() -> Result<()> {
    let config = Arc::new(
        Config::builder()
            .add_source(File::with_name("golden"))
            .add_source(Environment::with_prefix("TEST_ACROPOLIS"))
            // TODO: we should use set_override to provide test information in the configuration
            .build()
            .unwrap(),
    );

    let (completion_tx, mut completion_rx) = watch::channel(false);

    {
        let mut tx = TEST_COMPLETION_TX.lock().unwrap();
        *tx = Some(completion_tx);
    }

    let mut process = Process::<Message>::create(config).await;

    SnapshotBootstrapper::register(&mut process);
    TxUnpacker::register(&mut process);
    TestModule::register(&mut process);
    SPOState::register(&mut process);

    match timeout(Duration::from_secs(30), async {
        tokio::select! {
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
            panic!("Test timed out after 30 seconds");
        }
    }

    Ok(())
}
