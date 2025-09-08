use std::time::Duration;

use anyhow::Result;
use blake2::{
    digest::{Update, VariableOutput},
    Digest,
};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct PoolMetadataJson {
    pub name: String,
    pub description: String,
    pub ticker: String,
    pub homepage: String,
}

impl TryFrom<&[u8]> for PoolMetadataJson {
    type Error = serde_json::Error;

    /// Returns `PoolMetadataJson`
    ///
    /// # Arguments
    ///
    /// * `value` - Pool metadata (in json) as slice
    fn try_from(value: &[u8]) -> std::result::Result<Self, Self::Error> {
        serde_json::from_slice::<Self>(value)
    }
}

impl TryFrom<Vec<u8>> for PoolMetadataJson {
    type Error = serde_json::Error;

    /// Returns `PoolMetadataJson`
    ///
    /// # Arguments
    ///
    /// * `value` - Pool metadata (in json) as bytes
    fn try_from(value: Vec<u8>) -> std::result::Result<Self, Self::Error> {
        PoolMetadataJson::try_from(value.as_ref())
    }
}

/// Fetches pool metadata
///
/// # Returns
///
/// * `Ok<Vec<u8>>` - pool metadata in bytes format
pub async fn fetch_pool_metadata_as_bytes(url: String, timeout: Duration) -> Result<Vec<u8>> {
    let client = Client::new();
    let response = client.get(url).timeout(timeout).send().await?;
    let body = response.bytes().await?;
    Ok(body.to_vec())
}

/// Verifies the calculated pool metadata hash, is similar to the expected hash
///
/// # Arguments
///
/// * `pool_metadata` - The pool metadata as bytes
/// * `expected_hash` - The expected hash of the `pool_metadata`
///
/// # Returns
///
/// * `Ok(())` - for successful verification
/// * `Err(<error description>)` - for failed verifaction
pub fn verify_pool_metadata_hash(
    pool_metadata: &[u8],
    expected_hash: &acropolis_common::types::DataHash,
) -> Result<(), String> {
    // hash the serialized metadata
    let mut hasher = blake2::Blake2bVar::new(32).map_err(invalid_size_desc)?;
    hasher.update(pool_metadata);

    let mut hash = vec![0; 32];
    hasher.finalize_variable(&mut hash).map_err(invalid_size_desc)?;

    if &hash == expected_hash {
        return Ok(());
    }

    Err("pool metadata hash does not match to expected".into())
}

fn invalid_size_desc<T: std::fmt::Display>(e: T) -> String {
    format!("Invalid size for hashing pool metadata json {e}")
}
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_pool_metadata() {
        let url = "https://raw.githubusercontent.com/Octalus/cardano/master/p.json";
        let pool_metadata =
            fetch_pool_metadata_as_bytes(url.to_string(), Duration::from_secs(3)).await.unwrap();

        let pool_metadata = PoolMetadataJson::try_from(pool_metadata).expect("failed to convert");

        assert_eq!(pool_metadata.ticker, "OCTAS");
        assert_eq!(pool_metadata.name, "OctasPool");
        assert_eq!(pool_metadata.description, "Octa's Performance Pool");
        assert_eq!(pool_metadata.homepage, "https://octaluso.dyndns.org");
    }

    #[tokio::test]
    async fn test_pool_metadata_hash_verify() {
        let url = " https://880w.short.gy/clrsp.json ";

        let expected_hash = "3c914463aa1cddb425fba48b21c4db31958ea7a30e077f756a82903f30e04905";
        let expected_hash_as_arr = hex::decode(expected_hash).expect("should be able to decode {}");

        let pool_metadata =
            fetch_pool_metadata_as_bytes(url.to_string(), Duration::from_secs(3)).await.unwrap();

        assert_eq!(
            verify_pool_metadata_hash(&pool_metadata, &expected_hash_as_arr),
            Ok(())
        );
    }
}
