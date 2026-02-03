use std::time::Duration;

use acropolis_common::{rest_error::RESTError, AssetName, DataHash, PolicyId};
use anyhow::Result;
use blake2::digest::{Update, VariableOutput};
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
    expected_hash: &DataHash,
) -> Result<(), String> {
    // hash the serialized metadata
    let mut hasher = blake2::Blake2bVar::new(32).map_err(invalid_size_desc)?;
    hasher.update(pool_metadata);

    let mut hash = vec![0; 32];
    hasher.finalize_variable(&mut hash).map_err(invalid_size_desc)?;

    if hash == expected_hash.to_vec() {
        return Ok(());
    }

    Err("pool metadata hash does not match to expected".into())
}

fn invalid_size_desc<T: std::fmt::Display>(e: T) -> String {
    format!("Invalid size for hashing pool metadata json {e}")
}

pub fn split_policy_and_asset(hex_str: &str) -> Result<(PolicyId, AssetName), RESTError> {
    let decoded = hex::decode(hex_str)?;

    if decoded.len() < 28 {
        return Err(RESTError::BadRequest(
            "Asset identifier must be at least 28 bytes".to_string(),
        ));
    }

    let (policy_part, asset_part) = decoded.split_at(28);

    let policy_id: PolicyId = policy_part
        .try_into()
        .map_err(|_| RESTError::BadRequest("Policy id must be 28 bytes".to_string()))?;

    let asset_name = AssetName::new(asset_part).ok_or_else(|| {
        RESTError::BadRequest("Asset name must be less than 32 bytes".to_string())
    })?;

    Ok((policy_id, asset_name))
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    // XXX: it’s best to leave Internet interactions to integration tests, not unit tests, so let’s provide:
    async fn offline_fetch_pool_metadata_as_bytes(
        url: String,
        timeout: Duration,
    ) -> Result<Vec<u8>> {
        if let Ok(path) = std::env::var("ACROPOLIS_OFFLINE_MIRROR") {
            if let Ok(file) = std::fs::File::open(&path) {
                if let Ok(map) =
                    serde_json::from_reader::<_, std::collections::HashMap<String, String>>(file)
                {
                    if let Some(path_str) = map.get(url.trim()) {
                        if let Ok(bytes) = std::fs::read(path_str) {
                            return Ok(bytes);
                        }
                    }
                }
            }
        }
        // Fallback to network:
        fetch_pool_metadata_as_bytes(url, timeout).await
    }

    #[tokio::test]
    async fn test_fetch_pool_metadata() {
        let url = "https://raw.githubusercontent.com/Octalus/cardano/master/p.json";
        let pool_metadata =
            offline_fetch_pool_metadata_as_bytes(url.to_string(), Duration::from_secs(3))
                .await
                .unwrap();

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
        let expected_hash_as_arr =
            DataHash::from_str(expected_hash).expect("should be able to decode {}");

        let pool_metadata =
            offline_fetch_pool_metadata_as_bytes(url.to_string(), Duration::from_secs(3))
                .await
                .unwrap();

        assert_eq!(
            verify_pool_metadata_hash(&pool_metadata, &expected_hash_as_arr),
            Ok(())
        );
    }

    fn policy_id() -> PolicyId {
        PolicyId::from([0u8; 28])
    }

    #[test]
    fn invalid_hex_string() {
        let result = split_policy_and_asset("zzzz");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert_eq!(
            err.message(),
            "Invalid hex string: Invalid character 'z' at position 0"
        );
    }

    #[test]
    fn too_short_input() {
        let hex_str = hex::encode([1u8, 2, 3]);
        let result = split_policy_and_asset(&hex_str);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert_eq!(err.message(), "Asset identifier must be at least 28 bytes");
    }

    #[test]
    fn invalid_asset_name_too_long() {
        let mut bytes = policy_id().to_vec();
        bytes.extend(vec![0u8; 33]);
        let hex_str = hex::encode(bytes);
        let result = split_policy_and_asset(&hex_str);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.status_code(), 400);
        assert_eq!(err.message(), "Asset name must be less than 32 bytes");
    }

    #[test]
    fn valid_policy_and_asset() {
        let mut bytes = policy_id().to_vec();
        bytes.extend_from_slice(b"MyToken");
        let hex_str = hex::encode(bytes);
        let result = split_policy_and_asset(&hex_str);
        assert!(result.is_ok());
        let (policy, name) = result.unwrap();
        assert_eq!(policy, policy_id());
        assert_eq!(name.as_slice(), b"MyToken");
    }
}
