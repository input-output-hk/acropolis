use futures::future::join_all;
use reqwest;

use crate::types::PoolMetadataResponse;

pub async fn fetch_pools_metadata(urls: Vec<Option<String>>) -> Vec<Option<PoolMetadataResponse>> {
    let futures = urls.iter().map(|url| async move {
        let Some(url) = url else {
            return None;
        };
        let client = reqwest::Client::new();
        let Ok(response) = client.get(url).send().await else {
            return None;
        };
        let Ok(body) = response.json::<PoolMetadataResponse>().await else {
            return None;
        };
        Some(body)
    });

    join_all(futures).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_fetch_pools_metadata() {
        let urls = vec![Some(
            "https://raw.githubusercontent.com/Octalus/cardano/master/p.json".to_string(),
        )];
        let metadatas = fetch_pools_metadata(urls).await;
        assert_eq!(metadatas.len(), 1);
        let metadata = metadatas[0].as_ref().unwrap();
        assert_eq!(metadata.name, "OctasPool");
        assert_eq!(metadata.description, "Octa's Performance Pool");
        assert_eq!(metadata.ticker, "OCTAS");
        assert_eq!(metadata.homepage, "https://octaluso.dyndns.org");
    }
}
