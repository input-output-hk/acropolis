use std::time::Duration;

use anyhow::Result;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use sha2::Digest;

#[derive(Serialize, Deserialize)]
pub struct PoolMetadataJson {
    pub ticker: String,
    pub name: String,
    pub description: String,
    pub homepage: String,
}

impl PoolMetadataJson {
    /// verifies the calculated pool metadata hash, is similar to the expected hash
    pub fn verify(&self, expected_hash: &acropolis_common::types::DataHash) -> Result<(), String> {
        // convert to serialized cbor
        let serialized_metadata = serde_cbor::to_vec(&self)
            .map_err(|e| format!("Cannot serialize pool metadata json: {e:?}"))?;

        // hash the serialized metadata
        let hasher = sha2::Sha256::digest(serialized_metadata);
        let actual_hash = hasher.as_slice();

        if actual_hash == expected_hash {
            return Ok(());
        }

        Err("pool metadata hash does not match to expected".into())
    }
}

pub async fn fetch_pool_metadata(url: String, timeout: Duration) -> Result<PoolMetadataJson> {
    let client = Client::new();
    let response = client.get(url).timeout(timeout).send().await?;
    let body = response.json::<PoolMetadataJson>().await?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_pool_metadata() {
        let url = "https://raw.githubusercontent.com/Octalus/cardano/master/p.json";
        let pool_metadata =
            fetch_pool_metadata(url.to_string(), Duration::from_secs(3)).await.unwrap();
        assert_eq!(pool_metadata.ticker, "OCTAS");
        assert_eq!(pool_metadata.name, "OctasPool");
        assert_eq!(pool_metadata.description, "Octa's Performance Pool");
        assert_eq!(pool_metadata.homepage, "https://octaluso.dyndns.org");
    }
}
