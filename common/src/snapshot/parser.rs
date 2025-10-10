// Snapshot parser implementation - validates and streams Conway snapshot data.

use super::SnapshotError;
use crate::types::SnapshotMeta;
use std::fs;
use std::io::{BufReader, Read};
use std::path::Path;

/// Parse snapshot manifest JSON file into SnapshotMeta.
///
/// Validates all required fields are present and non-empty.
pub fn parse_manifest<P: AsRef<Path>>(manifest_path: P) -> Result<SnapshotMeta, SnapshotError> {
    let path = manifest_path.as_ref();

    // Check file exists and is not a directory
    if !path.exists() {
        return Err(SnapshotError::FileNotFound(path.display().to_string()));
    }

    if path.is_dir() {
        return Err(SnapshotError::FileNotFound(format!(
            "{} is a directory, not a file",
            path.display()
        )));
    }

    // Read and parse JSON
    let content = fs::read_to_string(path)?;
    let meta: SnapshotMeta = serde_json::from_str(&content)?;

    // Validate required fields
    if meta.magic.is_empty() {
        return Err(SnapshotError::StructuralDecode(
            "magic field is empty".to_string(),
        ));
    }

    if meta.version.is_empty() {
        return Err(SnapshotError::StructuralDecode(
            "version field is empty".to_string(),
        ));
    }

    if meta.era.is_empty() {
        return Err(SnapshotError::StructuralDecode(
            "era field is empty".to_string(),
        ));
    }

    if meta.block_height == 0 {
        return Err(SnapshotError::StructuralDecode(
            "block_height must be > 0".to_string(),
        ));
    }

    if meta.block_hash.is_empty() {
        return Err(SnapshotError::StructuralDecode(
            "block_hash field is empty".to_string(),
        ));
    }

    if meta.sha256.len() != 64 {
        return Err(SnapshotError::StructuralDecode(format!(
            "sha256 must be 64 hex chars, got {}",
            meta.sha256.len()
        )));
    }

    if meta.size_bytes == 0 {
        return Err(SnapshotError::StructuralDecode(
            "size_bytes must be > 0".to_string(),
        ));
    }

    Ok(meta)
}

/// Validate Conway era in snapshot metadata.
///
/// Returns error if era is not "conway".
pub fn validate_era(meta: &SnapshotMeta) -> Result<(), SnapshotError> {
    if meta.era != "conway" {
        return Err(SnapshotError::EraMismatch {
            expected: "conway".to_string(),
            actual: meta.era.clone(),
        });
    }
    Ok(())
}

/// Compute SHA256 checksum of snapshot payload file.
///
/// Returns hex-encoded hash string (64 chars).
pub fn compute_sha256<P: AsRef<Path>>(snapshot_path: P) -> Result<String, SnapshotError> {
    use sha2::{Digest, Sha256};

    let path = snapshot_path.as_ref();

    if !path.exists() {
        return Err(SnapshotError::FileNotFound(path.display().to_string()));
    }

    if path.is_dir() {
        return Err(SnapshotError::FileNotFound(format!(
            "{} is a directory, not a file",
            path.display()
        )));
    }

    let file = fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(16 * 1024 * 1024, file);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let result = hasher.finalize();
    Ok(format!("{result:x}"))
}

/// Validate snapshot integrity by comparing computed hash against manifest.
///
/// Returns error if hashes don't match or if file size differs from manifest.
pub fn validate_integrity<P: AsRef<Path>>(
    snapshot_path: P,
    meta: &SnapshotMeta,
) -> Result<(), SnapshotError> {
    let path = snapshot_path.as_ref();

    // Check file size matches manifest
    let file_meta = fs::metadata(path)?;
    let actual_size = file_meta.len();

    if actual_size != meta.size_bytes {
        return Err(SnapshotError::StructuralDecode(format!(
            "File size mismatch: manifest says {} bytes, file is {} bytes (truncated?)",
            meta.size_bytes, actual_size
        )));
    }

    // Compute and compare SHA256
    let computed_hash = compute_sha256(path)?;

    if computed_hash != meta.sha256 {
        return Err(SnapshotError::IntegrityMismatch {
            expected: meta.sha256.clone(),
            actual: computed_hash,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_manifest_validates_fields() {
        // Create a temporary test manifest
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_parser_manifest.json");
        
        // Valid manifest
        let valid_json = r#"{
            "magic": "CARDANO_SNAPSHOT",
            "version": "1.0",
            "era": "conway",
            "block_height": 100,
            "block_hash": "abc123",
            "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "size_bytes": 1024
        }"#;
        
        std::fs::write(&test_file, valid_json).unwrap();
        let result = parse_manifest(&test_file);
        assert!(result.is_ok());
        
        // Invalid: empty magic
        let invalid_json = r#"{
            "magic": "",
            "version": "1.0",
            "era": "conway",
            "block_height": 100,
            "block_hash": "abc123",
            "sha256": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
            "size_bytes": 1024
        }"#;
        
        std::fs::write(&test_file, invalid_json).unwrap();
        let result = parse_manifest(&test_file);
        assert!(result.is_err());
        
        // Cleanup
        let _ = std::fs::remove_file(&test_file);
    }

    #[test]
    fn test_validate_era() {
        let meta = SnapshotMeta {
            magic: "CARDANO_SNAPSHOT".to_string(),
            version: "1.0".to_string(),
            era: "conway".to_string(),
            block_height: 100,
            block_hash: "abc123".to_string(),
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            size_bytes: 1024,
        };
        
        assert!(validate_era(&meta).is_ok());
        
        let mut wrong_era = meta.clone();
        wrong_era.era = "byron".to_string();
        assert!(validate_era(&wrong_era).is_err());
    }

    #[test]
    fn test_compute_sha256() {
        // Create a temporary test file
        let temp_dir = std::env::temp_dir();
        let test_file = temp_dir.join("test_parser_snapshot.dat");
        
        std::fs::write(&test_file, b"test data").unwrap();
        
        let hash = compute_sha256(&test_file).unwrap();
        assert_eq!(hash.len(), 64); // SHA256 hex is 64 chars
        
        // Verify it's consistent
        let hash2 = compute_sha256(&test_file).unwrap();
        assert_eq!(hash, hash2);
        
        // Cleanup
        let _ = std::fs::remove_file(&test_file);
    }

    #[test]
    #[ignore] // Requires fixtures directory
    fn test_parse_real_manifest() {
        // Test with real fixture file if available
        let manifest_path = "tests/fixtures/test-manifest.json";
        if std::path::Path::new(manifest_path).exists() {
            let result = parse_manifest(manifest_path);
            assert!(result.is_ok());
            
            let meta = result.unwrap();
            assert_eq!(meta.era, "conway");
            assert_eq!(meta.block_height, 1000000);
            assert_eq!(meta.size_bytes, 245);
        }
    }

    #[test]
    #[ignore] // Requires fixtures directory
    fn test_validate_real_integrity() {
        // Test with real fixture files if available
        let manifest_path = "tests/fixtures/test-manifest.json";
        let snapshot_path = "tests/fixtures/snapshot-small.cbor";
        
        if std::path::Path::new(manifest_path).exists() 
            && std::path::Path::new(snapshot_path).exists() {
            let meta = parse_manifest(manifest_path).unwrap();
            let result = validate_integrity(snapshot_path, &meta);
            assert!(result.is_ok());
        }
    }
}
