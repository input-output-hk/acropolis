#![allow(dead_code, unused)]
use acropolis_common::hash::Hash;
use acropolis_common::Point;
use pallas_traverse::Era::Conway;
use pallas_traverse::{MultiEraBlock, MultiEraHeader};
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HeaderContextError {
    #[error("Failed to read header file {0}: {1}")]
    ReadFile(PathBuf, std::io::Error),

    #[error("Failed to decode header at slot {0}: {1}")]
    Decode(u64, String),

    #[error("Origin point has no hash")]
    OriginPoint,

    #[error("Failed to convert hash: {0}")]
    HashConversion(String),
}

#[derive(Debug)]
pub struct HeaderContext {
    pub point: Point,
    pub block_number: u64,
}

impl HeaderContext {
    /// Generate the path for a header file.
    /// Returns an error if the point is Origin (has no hash).
    pub fn path(network_dir: &Path, point: &Point) -> Result<PathBuf, HeaderContextError> {
        let hash = point.hash().ok_or(HeaderContextError::OriginPoint)?;
        let filename = format!("header.{}.{}.cbor", point.slot(), hash);
        Ok(network_dir.join("headers").join(filename))
    }

    /// Convert raw hash bytes to our Hash type.
    pub fn convert_hash(block_body_hash: &[u8]) -> Result<Hash<32>, HeaderContextError> {
        block_body_hash
            .try_into()
            .map_err(|_| HeaderContextError::HashConversion(format!("{:02x?}", block_body_hash)))
    }

    /// Load and decode header from `headers/header.{slot}.{hash}.cbor`
    pub fn load(network_dir: &Path, point: &Point) -> Result<Self, HeaderContextError> {
        let path = Self::path(network_dir, point)?;
        let cbor = fs::read(&path).map_err(|e| HeaderContextError::ReadFile(path, e))?;
        let header = MultiEraBlock::decode(&cbor)
            .map_err(|e| HeaderContextError::Decode(point.slot(), e.to_string()))?;
        Ok(Self {
            point: point.clone(),
            block_number: header.number(),
        })
    }
}

#[cfg(test)]
mod header_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn specific_point(slot: u64, hash_str: &str) -> Point {
        Point::Specific {
            slot,
            hash: hash_str.parse().expect("valid hash"),
        }
    }

    fn setup_headers_dir() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir_all(temp_dir.path().join("headers")).unwrap();
        temp_dir
    }

    const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    #[test]
    fn path_fails_for_origin_point() {
        let result = HeaderContext::path(Path::new("/test"), &Point::Origin);

        let err = result.unwrap_err();
        assert!(matches!(err, HeaderContextError::OriginPoint));
        assert_eq!(err.to_string(), "Origin point has no hash");
    }

    #[test]
    fn path_succeeds_for_specific_point() {
        let point = specific_point(42, ZERO_HASH);

        let path = HeaderContext::path(Path::new("/test"), &point).unwrap();

        assert!(path.ends_with(format!("headers/header.42.{}.cbor", ZERO_HASH)));
    }

    #[test]
    fn convert_hash_fails_for_wrong_length() {
        // Too short
        assert!(matches!(
            HeaderContext::convert_hash(&[0u8; 16]),
            Err(HeaderContextError::HashConversion(_))
        ));

        // Too long
        assert!(matches!(
            HeaderContext::convert_hash(&[0u8; 64]),
            Err(HeaderContextError::HashConversion(_))
        ));
    }

    #[test]
    fn convert_hash_succeeds_for_32_bytes() {
        let bytes = [0xab; 32];
        assert!(HeaderContext::convert_hash(&bytes).is_ok());
    }

    #[test]
    fn hash_conversion_error_includes_hex_representation() {
        let err = HeaderContext::convert_hash(&[0xde, 0xad, 0xbe, 0xef]).unwrap_err();
        let msg = err.to_string().to_lowercase();

        assert!(msg.contains("de") && msg.contains("ad"));
    }

    #[test]
    fn load_fails_for_origin_point() {
        let temp_dir = setup_headers_dir();

        let err = HeaderContext::load(temp_dir.path(), &Point::Origin).unwrap_err();

        assert!(matches!(err, HeaderContextError::OriginPoint));
    }

    #[test]
    fn load_fails_when_file_missing() {
        let temp_dir = setup_headers_dir();
        let point = specific_point(12345, ZERO_HASH);

        let err = HeaderContext::load(temp_dir.path(), &point).unwrap_err();

        assert!(matches!(err, HeaderContextError::ReadFile(_, _)));
        assert!(err.to_string().contains("header.12345"));
    }

    #[test]
    fn load_fails_for_invalid_cbor() {
        let temp_dir = setup_headers_dir();
        let point = specific_point(12345, ZERO_HASH);
        let path = HeaderContext::path(temp_dir.path(), &point).unwrap();
        fs::write(&path, b"not valid cbor").unwrap();

        let err = HeaderContext::load(temp_dir.path(), &point).unwrap_err();

        assert!(matches!(err, HeaderContextError::Decode(12345, _)));
    }

    #[test]
    fn load_fails_for_wrong_cbor_structure() {
        let temp_dir = setup_headers_dir();
        let point = specific_point(555, ZERO_HASH);
        let path = HeaderContext::path(temp_dir.path(), &point).unwrap();
        fs::write(&path, minicbor::to_vec(42u64).unwrap()).unwrap();

        let err = HeaderContext::load(temp_dir.path(), &point).unwrap_err();

        assert!(matches!(err, HeaderContextError::Decode(555, _)));
    }
}
