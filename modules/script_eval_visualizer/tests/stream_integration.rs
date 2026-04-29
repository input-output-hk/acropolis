//! Integration test: feed a synthetic `Phase2EvaluationResultsMessage`
//! through the public fan-out + broadcast path that the module uses
//! internally and assert the receiver yields one `ScriptEvalEvent` per
//! outcome, in order.

use std::sync::atomic::AtomicU64;

use acropolis_common::messages::Phase2EvaluationResultsMessage;
use acropolis_common::validation::ScriptEvaluationOutcome;
use acropolis_common::{
    BlockHash, BlockInfo, BlockIntent, BlockStatus, Era, ExUnits, PlutusVersion, RedeemerTag,
    ScriptHash, TxHash,
};
use acropolis_module_script_eval_visualizer::http::build_init_payload;
use acropolis_module_script_eval_visualizer::stream::{fan_out, ScriptEvalEvent};
use tokio::sync::broadcast;

fn fixture_block() -> BlockInfo {
    BlockInfo {
        status: BlockStatus::Volatile,
        intent: BlockIntent::ValidateAndApply,
        slot: 999,
        number: 12_345_678,
        hash: BlockHash::from([0x11; 32]),
        epoch: 503,
        epoch_slot: 4444,
        new_epoch: false,
        is_new_era: false,
        tip_slot: None,
        timestamp: 1_700_000_000,
        era: Era::Conway,
    }
}

fn outcome(
    seed: u8,
    purpose: RedeemerTag,
    version: PlutusVersion,
    success: bool,
) -> ScriptEvaluationOutcome {
    ScriptEvaluationOutcome {
        script_hash: ScriptHash::from([seed; 28]),
        purpose,
        plutus_version: version,
        ex_units: ExUnits {
            mem: u64::from(seed) * 1_000,
            steps: u64::from(seed) * 1_000_000,
        },
        is_success: success,
        error_message: if success {
            None
        } else {
            Some(format!("err-{seed}"))
        },
    }
}

#[tokio::test]
async fn fan_out_through_broadcast_yields_one_event_per_outcome() {
    let (tx, mut rx) = broadcast::channel::<ScriptEvalEvent>(64);
    let counter = AtomicU64::new(1);

    let block = fixture_block();
    let msg = Phase2EvaluationResultsMessage {
        tx_hash: TxHash::from([0x22; 32]),
        tx_index_in_block: 4,
        is_valid: true,
        outcomes: vec![
            outcome(0x01, RedeemerTag::Spend, PlutusVersion::V1, true),
            outcome(0x02, RedeemerTag::Mint, PlutusVersion::V2, true),
            outcome(0x03, RedeemerTag::Cert, PlutusVersion::V3, false),
        ],
    };

    let events = fan_out(&block, &msg, &counter);
    assert_eq!(events.len(), 3);

    for event in events {
        tx.send(event).expect("send must succeed with one live receiver");
    }

    // Drain three events back out, in order.
    let e1 = rx.recv().await.expect("event 1");
    let e2 = rx.recv().await.expect("event 2");
    let e3 = rx.recv().await.expect("event 3");

    assert_eq!(e1.id, 1);
    assert_eq!(e1.purpose, "spend");
    assert_eq!(e1.plutus_version, "v1");
    assert!(e1.success);

    assert_eq!(e2.id, 2);
    assert_eq!(e2.purpose, "mint");
    assert_eq!(e2.plutus_version, "v2");

    assert_eq!(e3.id, 3);
    assert_eq!(e3.purpose, "cert");
    assert_eq!(e3.plutus_version, "v3");
    assert!(!e3.success);
    assert_eq!(e3.error.as_deref(), Some("err-3"));

    // All events agree on block context.
    assert_eq!(e1.epoch, 503);
    assert_eq!(e1.slot, 999);
    assert_eq!(e1.block_number, 12_345_678);
    assert_eq!(e1.block_hash, "11".repeat(32));
    assert_eq!(e1.tx_hash, "22".repeat(32));
}

#[test]
fn init_event_carries_cexplorer_base_url_and_network() {
    // Mainnet: canonical URL.
    let payload = build_init_payload("https://cexplorer.io", "mainnet");
    assert_eq!(
        payload["cexplorerBaseUrl"].as_str(),
        Some("https://cexplorer.io")
    );
    assert_eq!(payload["network"].as_str(), Some("mainnet"));

    // Preprod: subdomain.
    let payload = build_init_payload("https://preprod.cexplorer.io", "preprod");
    assert_eq!(
        payload["cexplorerBaseUrl"].as_str(),
        Some("https://preprod.cexplorer.io")
    );
    assert_eq!(payload["network"].as_str(), Some("preprod"));
}

#[tokio::test]
async fn fan_out_skips_when_outcomes_empty() {
    let (tx, mut rx) = broadcast::channel::<ScriptEvalEvent>(8);
    let counter = AtomicU64::new(0);

    let block = fixture_block();
    let msg = Phase2EvaluationResultsMessage {
        tx_hash: TxHash::from([0x33; 32]),
        tx_index_in_block: 0,
        is_valid: true,
        outcomes: vec![],
    };

    let events = fan_out(&block, &msg, &counter);
    assert!(events.is_empty(), "no events for empty outcomes");
    for event in events {
        tx.send(event).expect("unreachable");
    }

    // Receiver must time out / report no message.
    let res = tokio::time::timeout(std::time::Duration::from_millis(100), rx.recv()).await;
    assert!(res.is_err(), "no events should arrive");
}
