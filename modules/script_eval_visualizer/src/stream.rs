//! Per-script `ScriptEvalEvent` and the fan-out from a per-tx
//! [`Phase2EvaluationResultsMessage`].
//!
//! One incoming `Phase2EvaluationResultsMessage` (per-transaction; carries N
//! script outcomes) is fanned out to N `ScriptEvalEvent`s — one per script —
//! and broadcast to all currently-connected SSE clients.

use std::sync::atomic::{AtomicU64, Ordering};

use acropolis_common::messages::Phase2EvaluationResultsMessage;
use acropolis_common::validation::ScriptEvaluationOutcome;
use acropolis_common::{BlockInfo, PlutusVersion, RedeemerTag};
use serde::Serialize;

/// One row of the visualizer table, broadcast to every connected SSE client.
///
/// Field naming follows the SSE wire contract — JSON keys are camelCase.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScriptEvalEvent {
    /// Monotonic per-process id; used as the SSE `id:` field.
    pub id: u64,

    /// Cardano epoch number.
    pub epoch: u64,

    /// Absolute slot of the block containing the transaction.
    pub slot: u64,

    /// Block number containing the transaction.
    pub block_number: u64,

    /// Block hash (lowercase hex, 64 chars).
    pub block_hash: String,

    /// Transaction hash (lowercase hex, 64 chars).
    pub tx_hash: String,

    /// Script hash (lowercase hex, 56 chars).
    pub script_hash: String,

    /// Script purpose (`spend`/`mint`/`cert`/`reward`/`vote`/`propose`).
    pub purpose: &'static str,

    /// Plutus language version (`v1`/`v2`/`v3`).
    pub plutus_version: &'static str,

    /// Memory units declared by the redeemer.
    pub mem: u64,

    /// CPU/steps units declared by the redeemer.
    pub cpu: u64,

    /// `true` iff the script evaluated successfully.
    pub success: bool,

    /// On failure: a short rendered error message; omitted on success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Lowercase hex helper that does not allocate twice.
fn to_hex<B: AsRef<[u8]>>(bytes: B) -> String {
    hex::encode(bytes)
}

/// Convert a [`RedeemerTag`] into its wire-format string.
fn purpose_str(tag: &RedeemerTag) -> &'static str {
    match tag {
        RedeemerTag::Spend => "spend",
        RedeemerTag::Mint => "mint",
        RedeemerTag::Cert => "cert",
        RedeemerTag::Reward => "reward",
        RedeemerTag::Vote => "vote",
        RedeemerTag::Propose => "propose",
    }
}

/// Convert a [`PlutusVersion`] into its wire-format string.
fn plutus_version_str(v: PlutusVersion) -> &'static str {
    match v {
        PlutusVersion::V1 => "v1",
        PlutusVersion::V2 => "v2",
        PlutusVersion::V3 => "v3",
    }
}

/// Build one [`ScriptEvalEvent`] from a single outcome plus block/tx context.
fn event_from_outcome(
    id: u64,
    block: &BlockInfo,
    tx_hash_hex: &str,
    outcome: &ScriptEvaluationOutcome,
) -> ScriptEvalEvent {
    ScriptEvalEvent {
        id,
        epoch: block.epoch,
        slot: block.slot,
        block_number: block.number,
        block_hash: to_hex(block.hash.as_ref()),
        tx_hash: tx_hash_hex.to_owned(),
        script_hash: to_hex(outcome.script_hash.as_ref()),
        purpose: purpose_str(&outcome.purpose),
        plutus_version: plutus_version_str(outcome.plutus_version),
        mem: outcome.ex_units.mem,
        cpu: outcome.ex_units.steps,
        success: outcome.is_success,
        error: outcome.error_message.clone(),
    }
}

/// Fan a per-transaction phase-2 message out into per-script events.
///
/// Returns one [`ScriptEvalEvent`] per outcome in `msg.outcomes`. Each event
/// gets a fresh monotonic `id` from `next_id`. The order matches `msg.outcomes`.
pub fn fan_out(
    block: &BlockInfo,
    msg: &Phase2EvaluationResultsMessage,
    next_id: &AtomicU64,
) -> Vec<ScriptEvalEvent> {
    let tx_hash_hex = to_hex(msg.tx_hash.as_ref());
    msg.outcomes
        .iter()
        .map(|outcome| {
            let id = next_id.fetch_add(1, Ordering::Relaxed);
            event_from_outcome(id, block, &tx_hash_hex, outcome)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use acropolis_common::{
        BlockHash, BlockInfo, BlockIntent, BlockStatus, Era, ExUnits, PlutusVersion, RedeemerTag,
        ScriptHash, TxHash,
    };

    fn fixture_block() -> BlockInfo {
        BlockInfo {
            status: BlockStatus::Volatile,
            intent: BlockIntent::ValidateAndApply,
            slot: 12345,
            number: 11_234_567,
            hash: BlockHash::from([0xaa; 32]),
            epoch: 502,
            epoch_slot: 1234,
            new_epoch: false,
            is_new_era: false,
            tip_slot: None,
            timestamp: 0,
            era: Era::Conway,
        }
    }

    fn fixture_outcome(
        seed: u8,
        purpose: RedeemerTag,
        plutus_version: PlutusVersion,
        success: bool,
    ) -> ScriptEvaluationOutcome {
        ScriptEvaluationOutcome {
            script_hash: ScriptHash::from([seed; 28]),
            purpose,
            plutus_version,
            ex_units: ExUnits {
                mem: 1_234_567,
                steps: 9_876_543_210,
            },
            is_success: success,
            error_message: if success { None } else { Some("boom".into()) },
        }
    }

    fn fixture_message(outcomes: Vec<ScriptEvaluationOutcome>) -> Phase2EvaluationResultsMessage {
        Phase2EvaluationResultsMessage {
            tx_hash: TxHash::from([0xbb; 32]),
            tx_index_in_block: 7,
            is_valid: true,
            outcomes,
        }
    }

    #[test]
    fn fan_out_emits_one_event_per_outcome_in_order() {
        let block = fixture_block();
        let msg = fixture_message(vec![
            fixture_outcome(0x10, RedeemerTag::Spend, PlutusVersion::V1, true),
            fixture_outcome(0x20, RedeemerTag::Mint, PlutusVersion::V2, false),
            fixture_outcome(0x30, RedeemerTag::Cert, PlutusVersion::V3, true),
        ]);
        let counter = AtomicU64::new(100);
        let events = fan_out(&block, &msg, &counter);

        assert_eq!(events.len(), 3, "one event per outcome");
        assert_eq!(events[0].id, 100);
        assert_eq!(events[1].id, 101);
        assert_eq!(events[2].id, 102);
        assert_eq!(counter.load(Ordering::Relaxed), 103);

        // Block context propagated identically into every event.
        for ev in &events {
            assert_eq!(ev.epoch, 502);
            assert_eq!(ev.slot, 12345);
            assert_eq!(ev.block_number, 11_234_567);
            assert_eq!(ev.block_hash, "a".repeat(64));
            assert_eq!(ev.tx_hash, "b".repeat(64));
            assert_eq!(ev.mem, 1_234_567);
            assert_eq!(ev.cpu, 9_876_543_210);
        }

        // Per-outcome fields propagated in order.
        assert_eq!(events[0].script_hash, "10".repeat(28));
        assert_eq!(events[0].purpose, "spend");
        assert_eq!(events[0].plutus_version, "v1");
        assert!(events[0].success);
        assert!(events[0].error.is_none());

        assert_eq!(events[1].script_hash, "20".repeat(28));
        assert_eq!(events[1].purpose, "mint");
        assert_eq!(events[1].plutus_version, "v2");
        assert!(!events[1].success);
        assert_eq!(events[1].error.as_deref(), Some("boom"));

        assert_eq!(events[2].script_hash, "30".repeat(28));
        assert_eq!(events[2].purpose, "cert");
        assert_eq!(events[2].plutus_version, "v3");
    }

    #[test]
    fn fan_out_on_empty_outcomes_emits_no_events() {
        let counter = AtomicU64::new(0);
        let events = fan_out(&fixture_block(), &fixture_message(vec![]), &counter);
        assert!(events.is_empty());
        assert_eq!(counter.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn sse_payload_uses_camelcase_and_omits_error_on_success() {
        let block = fixture_block();
        let msg = fixture_message(vec![fixture_outcome(
            0x42,
            RedeemerTag::Vote,
            PlutusVersion::V3,
            true,
        )]);
        let counter = AtomicU64::new(0);
        let events = fan_out(&block, &msg, &counter);
        let json = serde_json::to_value(&events[0]).expect("serializes");
        let obj = json.as_object().expect("is object");

        // Required keys per contracts/sse-stream.md
        for key in [
            "id",
            "epoch",
            "slot",
            "blockNumber",
            "blockHash",
            "txHash",
            "scriptHash",
            "purpose",
            "plutusVersion",
            "mem",
            "cpu",
            "success",
        ] {
            assert!(obj.contains_key(key), "missing key {key}");
        }
        // Error must be omitted on success.
        assert!(
            !obj.contains_key("error"),
            "error key must be omitted on success"
        );
        // No snake_case leaks.
        for k in obj.keys() {
            assert!(!k.contains('_'), "unexpected snake_case key: {k}");
        }
    }

    #[test]
    fn sse_payload_includes_error_on_failure() {
        let block = fixture_block();
        let msg = fixture_message(vec![fixture_outcome(
            0x42,
            RedeemerTag::Spend,
            PlutusVersion::V1,
            false,
        )]);
        let counter = AtomicU64::new(0);
        let events = fan_out(&block, &msg, &counter);
        let json = serde_json::to_value(&events[0]).expect("serializes");
        assert_eq!(json["error"].as_str(), Some("boom"));
        assert_eq!(json["success"].as_bool(), Some(false));
    }
}
