use acropolis_common::hash::Hash;
use acropolis_common::Point;
use pallas_primitives::babbage::MintedHeader;
use pallas_primitives::conway::Header as ConwayHeader;
use std::fs;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HeaderError {
    #[error("Failed to read header file {0}: {1}")]
    ReadFile(PathBuf, std::io::Error),

    #[error("Failed to decode header at slot {0}: {1}")]
    Decode(u64, String),

    #[error("Origin point has no hash")]
    OriginPoint,

    #[error("Failed to convert hash: {0}")]
    HashConversion(String),
}

pub struct Header {
    pub point: Point,
    pub block_number: u64,
    pub block_hash: Hash<32>,
}

impl Header {
    pub fn path(network_dir: &Path, point: &Point) -> PathBuf {
        let filename = format!(
            "header.{}.{}.cbor",
            point.slot(),
            point.hash().expect("header point must have hash")
        );
        network_dir.join("headers").join(filename)
    }

    /// Load and decode header from `headers/header.{slot}.{hash}.cbor`
    pub fn load(network_dir: &Path, point: &Point) -> Result<Self, HeaderError> {
        let path = Self::path(network_dir, point);
        let cbor = fs::read(&path).map_err(|e| HeaderError::ReadFile(path, e))?;

        let minted: MintedHeader<'_> = minicbor::decode(&cbor)
            .map_err(|e| HeaderError::Decode(point.slot(), e.to_string()))?;
        let header = ConwayHeader::from(minted);
        let block_body_hash = header.header_body.block_body_hash;
        let hash: Hash<32> = block_body_hash
            .as_ref()
            .try_into()
            .map_err(|_| HeaderError::HashConversion(format!("{:?}", block_body_hash)))?;

        Ok(Self {
            point: point.clone(),
            block_number: header.header_body.block_number,
            block_hash: hash,
        })
    }
}
