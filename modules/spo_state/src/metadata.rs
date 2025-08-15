use acropolis_common::{KeyHash, PoolMetadata, PoolMetadataExtended};
use anyhow::Result;
use futures::future::join_all;
use std::collections::HashMap;
use tokio::time::{timeout, Duration};
use tracing::info;

#[derive(Debug, serde::Serialize, serde::Deserialize, Clone)]
pub struct PoolMetadataJson {
    name: String,
    description: String,
    ticker: String,
    homepage: String,
}

const METADATA_FETCH_TIMEOUT: u64 = 5;

pub async fn fetch_pools_metadata(
    pools_metadata_extended: &mut HashMap<KeyHash, PoolMetadataExtended>,
    pools_metadata: HashMap<KeyHash, PoolMetadata>,
) -> Result<()> {
    let futures: Vec<_> = pools_metadata
        .iter()
        .map(|(spo, metadata)| async move {
            let client = reqwest::Client::new();
            let response_result = timeout(
                Duration::from_secs(METADATA_FETCH_TIMEOUT),
                client.get(&metadata.url).send(),
            )
            .await;

            match response_result {
                Ok(Ok(response)) => match response.json::<PoolMetadataJson>().await {
                    Ok(metadata_json) => {
                        info!("Fetched metadata for Pool ID {}", hex::encode(&spo));
                        Ok((
                            spo,
                            PoolMetadataExtended {
                                url: metadata.url.clone(),
                                hash: metadata.hash.clone(),
                                name: Some(metadata_json.name),
                                description: Some(metadata_json.description),
                                ticker: Some(metadata_json.ticker),
                                homepage: Some(metadata_json.homepage),
                            },
                        ))
                    }
                    Err(e) => {
                        info!(
                            "Failed to parse JSON for Pool ID: {} from {}: {}",
                            hex::encode(&spo),
                            metadata.url,
                            e
                        );
                        Err(anyhow::anyhow!("Failed to parse JSON: {}", e))
                    }
                },
                Ok(Err(e)) => {
                    info!(
                        "Failed to fetch metadata for Pool ID: {} from {}: {}",
                        hex::encode(&spo),
                        metadata.url,
                        e
                    );
                    Err(anyhow::anyhow!("Failed to fetch metadata: {}", e))
                }
                Err(_) => {
                    info!(
                        "Timeout fetching metadata for Pool ID: {} from {}",
                        hex::encode(&spo),
                        metadata.url
                    );
                    Err(anyhow::anyhow!("Timeout fetching metadata"))
                }
            }
        })
        .collect();

    let results = join_all(futures).await;

    // Process results and update the map
    for result in results {
        if let Ok((spo, metadata_extended)) = result {
            pools_metadata_extended.insert(spo.clone(), metadata_extended);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_pools_metadata() {
        let mut pools_metadata_extended = HashMap::new();
        let mut pools_metadata = HashMap::new();

        pools_metadata.insert(
            "pool19uynx6nxcdksmaqdcshjg487fap3rs3axyhrdqa7gdqgzgxss4y".to_string().into(),
            PoolMetadata {
                url: "https://tokyostaker.com/metadata/japan.json".to_string(),
                hash: "371c1a13a2eee3037efcce30825e95b1a3524bc5ea165627a2699f0b2cbdf413"
                    .to_string()
                    .into(),
            },
        );

        fetch_pools_metadata(&mut pools_metadata_extended, pools_metadata).await.unwrap();

        let fetched_metadata = pools_metadata_extended
            .get(&"pool19uynx6nxcdksmaqdcshjg487fap3rs3axyhrdqa7gdqgzgxss4y".as_bytes().to_vec())
            .unwrap();
        assert_eq!(
            fetched_metadata.url,
            "https://tokyostaker.com/metadata/japan.json".to_string()
        );

        assert_eq!(
            fetched_metadata.hash,
            "371c1a13a2eee3037efcce30825e95b1a3524bc5ea165627a2699f0b2cbdf413".as_bytes().to_vec()
        );
        assert!(fetched_metadata.name.is_some());
        assert!(fetched_metadata.description.is_some());
        assert!(fetched_metadata.ticker.is_some());
        assert!(fetched_metadata.homepage.is_some());
    }
}
