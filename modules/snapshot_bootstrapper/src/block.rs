use acropolis_common::Point;
use pallas_traverse::MultiEraBlock;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum BlockContextError {
    #[error("Failed to read block cbor file {0}: {1}")]
    ReadFile(PathBuf, std::io::Error),

    #[error("Failed to decode block cbor at slot {0}: {1}")]
    Decode(u64, String),

    #[error("Origin point has no hash")]
    OriginPoint,
}

#[derive(Debug)]
pub struct BlockContext {
    pub point: Point,
    pub block_number: u64,
}

impl BlockContext {
    /// Generate the path for a block file.
    /// Returns an error if the point is Origin (has no hash).
    pub fn path(network_dir: &Path, point: &Point) -> Result<PathBuf, BlockContextError> {
        let hash = point.hash().ok_or(BlockContextError::OriginPoint)?;
        let filename = format!("block.{}.{}.cbor", point.slot(), hash);
        Ok(network_dir.join("blocks").join(filename))
    }

    /// Load and decode block from `blocks/block.{slot}.{hash}.cbor`
    pub fn load(network_dir: &Path, point: &Point) -> Result<Self, BlockContextError> {
        let path = Self::path(network_dir, point)?;
        let cbor = fs::read(&path).map_err(|e| BlockContextError::ReadFile(path, e))?;
        let block = MultiEraBlock::decode(&cbor)
            .map_err(|e| BlockContextError::Decode(point.slot(), e.to_string()))?;

        Ok(Self {
            point: point.clone(),
            block_number: block.number(),
        })
    }
}

#[cfg(test)]
mod block_tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn specific_point(slot: u64, hash_str: &str) -> Point {
        Point::Specific {
            slot,
            hash: hash_str.parse().expect("valid hash"),
        }
    }

    fn setup_blocks_dir() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        fs::create_dir_all(temp_dir.path().join("blocks")).unwrap();
        temp_dir
    }

    const ZERO_HASH: &str = "0000000000000000000000000000000000000000000000000000000000000000";

    #[test]
    fn path_fails_for_origin_point() {
        let result = BlockContext::path(Path::new("/test"), &Point::Origin);

        let err = result.unwrap_err();
        assert!(matches!(err, BlockContextError::OriginPoint));
        assert_eq!(err.to_string(), "Origin point has no hash");
    }

    #[test]
    fn path_succeeds_for_specific_point() {
        let point = specific_point(42, ZERO_HASH);

        let path = BlockContext::path(Path::new("/test"), &point).unwrap();

        assert!(path.ends_with(format!("blocks/block.42.{}.cbor", ZERO_HASH)));
    }

    #[test]
    fn load_fails_for_origin_point() {
        let temp_dir = setup_blocks_dir();

        let err = BlockContext::load(temp_dir.path(), &Point::Origin).unwrap_err();

        assert!(matches!(err, BlockContextError::OriginPoint));
    }

    #[test]
    fn load_fails_when_file_missing() {
        let temp_dir = setup_blocks_dir();
        let point = specific_point(12345, ZERO_HASH);

        let err = BlockContext::load(temp_dir.path(), &point).unwrap_err();

        assert!(matches!(err, BlockContextError::ReadFile(_, _)));
        assert!(err.to_string().contains("block.12345"));
    }

    #[test]
    fn load_fails_for_invalid_cbor() {
        let temp_dir = setup_blocks_dir();
        let point = specific_point(12345, ZERO_HASH);
        let path = BlockContext::path(temp_dir.path(), &point).unwrap();
        fs::write(&path, b"not valid cbor").unwrap();

        let err = BlockContext::load(temp_dir.path(), &point).unwrap_err();

        assert!(matches!(err, BlockContextError::Decode(12345, _)));
    }

    #[test]
    fn load_fails_for_wrong_cbor_structure() {
        let temp_dir = setup_blocks_dir();
        let point = specific_point(555, ZERO_HASH);
        let path = BlockContext::path(temp_dir.path(), &point).unwrap();
        fs::write(&path, minicbor::to_vec(42u64).unwrap()).unwrap();

        let err = BlockContext::load(temp_dir.path(), &point).unwrap_err();

        assert!(matches!(err, BlockContextError::Decode(555, _)));
    }
}
