use pallas_primitives::babbage::MintedHeader;
use pallas_primitives::conway::Header as ConwayHeader;
use std::fs;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HeaderError {
    #[error("Failed to read header file {0}: {1}")]
    ReadFile(String, std::io::Error),

    #[error("Failed to decode header at slot {0}: {1}")]
    Decode(u64, String),

    #[error("Invalid point format: {0}")]
    InvalidPoint(String),

    #[error("Invalid hex hash: {0}")]
    InvalidHex(String, #[source] hex::FromHexError),
}

#[derive(Debug, Clone)]
pub struct Header {
    cbor: Vec<u8>,
    pub slot: u64,
    pub hash: [u8; 32],
}

impl Header {
    /// Load header from `headers/header.{point}.cbor`
    pub fn load(dir: &str, point: &str) -> Result<Self, HeaderError> {
        let (slot, hash_hex) = parse_point(point)?;
        let hash = decode_hash32(hash_hex)?;

        let path = format!("{dir}/headers/header.{point}.cbor");
        let cbor = fs::read(&path).map_err(|e| HeaderError::ReadFile(path, e))?;

        Ok(Self { cbor, slot, hash })
    }

    /// Decode CBOR and extract the block number.
    pub fn block_number(&self) -> Result<u64, HeaderError> {
        let header = self.decode()?;
        Ok(header.header_body.block_number)
    }

    fn decode(&self) -> Result<ConwayHeader, HeaderError> {
        let minted: MintedHeader<'_> = minicbor::decode(&self.cbor)
            .map_err(|e| HeaderError::Decode(self.slot, e.to_string()))?;
        Ok(ConwayHeader::from(minted))
    }
}

fn parse_point(s: &str) -> Result<(u64, &str), HeaderError> {
    let (slot, hash) = s.split_once('.').ok_or_else(|| HeaderError::InvalidPoint(s.to_string()))?;
    let slot = slot.parse().map_err(|_| HeaderError::InvalidPoint(s.to_string()))?;
    Ok((slot, hash))
}

fn decode_hash32(hex_str: &str) -> Result<[u8; 32], HeaderError> {
    let bytes =
        hex::decode(hex_str).map_err(|e| HeaderError::InvalidHex(hex_str.to_string(), e))?;
    bytes.try_into().map_err(|_| HeaderError::InvalidPoint(hex_str.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_point() {
        let (slot, hash) = parse_point("134956789.abc123").unwrap();
        assert_eq!(slot, 134956789);
        assert_eq!(hash, "abc123");

        assert!(parse_point("invalid").is_err());
        assert!(parse_point("not_number.abc").is_err());
    }
}
