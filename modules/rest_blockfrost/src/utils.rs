use std::time::Duration;

use anyhow::Result;
use reqwest::Client;

use crate::types::PoolMetadataJson;

pub async fn fetch_pool_metadata(url: String) -> Result<PoolMetadataJson> {
    let client = Client::new();
    let response = client.get(url).timeout(Duration::from_secs(3)).send().await?;
    let body = response.json::<PoolMetadataJson>().await?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_pool_metadata() {
        let url = "https://raw.githubusercontent.com/Octalus/cardano/master/p.json";
        let pool_metadata = fetch_pool_metadata(url.to_string()).await.unwrap();
        assert_eq!(pool_metadata.ticker, "OCTAS");
        assert_eq!(pool_metadata.name, "OctasPool");
        assert_eq!(pool_metadata.description, "Octa's Performance Pool");
        assert_eq!(pool_metadata.homepage, "https://octaluso.dyndns.org");
    }
}
