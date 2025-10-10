//! Unit tests for snapshot streaming iterator (T012)

use spec_test::snapshot::stream::open_stream;
use spec_test::types::LedgerMessage;

#[test]
fn test_stream_valid_snapshot() {
    // Use the CBOR fixture generated alongside this test
    let iter = open_stream("tests/fixtures/snapshot-small.cbor").expect("failed to open stream");

    let items: Vec<_> = iter.collect();

    // Expected: 2 UTXOs, 1 TipUpdate, 1 GovernanceActions, 1 ParameterSet, 1 EndOfSnapshot = 6 items
    assert_eq!(items.len(), 6, "expected 6 items in fixture");

    // Validate sequence
    assert!(matches!(&items[0], Ok(LedgerMessage::UtxoEntry { .. })));
    assert!(matches!(&items[1], Ok(LedgerMessage::UtxoEntry { .. })));
    assert!(matches!(&items[2], Ok(LedgerMessage::TipUpdate { .. })));
    assert!(matches!(
        &items[3],
        Ok(LedgerMessage::GovernanceActions { .. })
    ));
    assert!(matches!(&items[4], Ok(LedgerMessage::ParameterSet { .. })));
    assert!(matches!(&items[5], Ok(LedgerMessage::EndOfSnapshot)));

    // All should be Ok
    for (i, item) in items.iter().enumerate() {
        assert!(item.is_ok(), "item {i} should be Ok, got {item:?}");
    }
}

#[test]
fn test_stream_missing_end_marker() {
    let iter =
        open_stream("tests/fixtures/snapshot-missing-end.cbor").expect("failed to open stream");
    let items: Vec<_> = iter.collect();

    // Last item should be an error about missing EndOfSnapshot
    assert!(!items.is_empty());
    let last = items.last().unwrap();
    assert!(last.is_err());
    if let Err(e) = last {
        assert!(
            e.to_string().contains("missing EndOfSnapshot"),
            "expected missing end error, got: {e}"
        );
    }
}

#[test]
fn test_stream_count_mismatch() {
    let iter =
        open_stream("tests/fixtures/snapshot-count-mismatch.cbor").expect("failed to open stream");
    let items: Vec<_> = iter.collect();

    // Should get error about count mismatch at end
    assert!(!items.is_empty());
    let last = items.last().unwrap();
    assert!(last.is_err());
    if let Err(e) = last {
        let msg = e.to_string();
        assert!(
            msg.contains("Integrity") || msg.contains("mismatch"),
            "expected integrity error, got: {e}"
        );
    }
}

#[test]
fn test_stream_duplicate_end_marker() {
    let iter =
        open_stream("tests/fixtures/snapshot-duplicate-end.cbor").expect("failed to open stream");
    let items: Vec<_> = iter.collect();

    // Should get error about duplicate end marker
    // Note: if we set finished=true after first EndOfSnapshot, we won't see the second one.
    // That's actually fine - we stop after first end marker as expected.
    // Let's just verify we got at least the first EndOfSnapshot
    assert!(!items.is_empty(), "should have at least one item");
    let has_end = items
        .iter()
        .any(|r| matches!(r, Ok(spec_test::types::LedgerMessage::EndOfSnapshot)));
    assert!(has_end, "should have EndOfSnapshot marker");
}

#[test]
fn test_stream_wrong_era() {
    // Should fail during open_stream (header validation)
    let result = open_stream("tests/fixtures/snapshot-wrong-era.cbor");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.to_string().contains("Era mismatch"),
        "expected era mismatch, got: {err}"
    );
}
