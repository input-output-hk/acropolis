// Unit tests for snapshot parser

use spec_test::snapshot::parser::{parse_manifest, validate_era, validate_integrity};

#[test]
fn test_parse_valid_manifest() {
    let result = parse_manifest("tests/fixtures/test-manifest.json");
    assert!(result.is_ok());

    let meta = result.unwrap();
    assert_eq!(meta.era, "conway");
    assert_eq!(meta.block_height, 1000000);
    assert_eq!(meta.size_bytes, 245);
}

#[test]
fn test_parse_missing_file() {
    let result = parse_manifest("tests/fixtures/nonexistent.json");
    assert!(result.is_err());
}

#[test]
fn test_validate_conway_era() {
    let meta = parse_manifest("tests/fixtures/test-manifest.json").unwrap();
    assert!(validate_era(&meta).is_ok());
}

#[test]
fn test_validate_wrong_era() {
    let meta = parse_manifest("tests/fixtures/wrong-era-manifest.json").unwrap();
    let result = validate_era(&meta);
    assert!(result.is_err());
}

#[test]
fn test_validate_integrity_success() {
    let meta = parse_manifest("tests/fixtures/test-manifest.json").unwrap();
    let result = validate_integrity("tests/fixtures/snapshot-small.cbor", &meta);
    assert!(result.is_ok());
}

#[test]
fn test_validate_integrity_hash_mismatch() {
    // Use wrong-era manifest which has wrong sha256 for the snapshot
    let mut meta = parse_manifest("tests/fixtures/test-manifest.json").unwrap();
    meta.sha256 = "0000000000000000000000000000000000000000000000000000000000000000".to_string();

    let result = validate_integrity("tests/fixtures/snapshot-small.cbor", &meta);
    assert!(result.is_err());
}
