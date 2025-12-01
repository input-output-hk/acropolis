use crate::configuration::{DownloadConfig, Snapshot};
use crate::progress_reader::ProgressReader;
use async_compression::tokio::bufread::GzipDecoder;
use futures_util::TryStreamExt;
use reqwest::Client;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;
use thiserror::Error;
use tokio::fs::File;
use tokio::io::BufReader;
use tracing::info;

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("Failed to initialize HTTP client: {0}")]
    ClientInit(#[from] reqwest::Error),

    #[error("Failed to download snapshot from {0}: {1}")]
    RequestFailed(String, reqwest::Error),

    #[error("Download failed from {0}: HTTP status {1}")]
    InvalidStatusCode(String, reqwest::StatusCode),

    #[error("Cannot create directory {0}: {1}")]
    CreateDirectory(PathBuf, io::Error),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

/// Handles downloading and decompressing snapshot files.
pub struct SnapshotDownloader {
    client: Client,
    network_dir: String,
    cfg: DownloadConfig,
}

impl SnapshotDownloader {
    pub fn new(network_dir: &str, config: &DownloadConfig) -> Result<Self, DownloadError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .connect_timeout(Duration::from_secs(config.connect_timeout_secs))
            .build()?;

        Ok(Self {
            client,
            network_dir: network_dir.to_string(),
            cfg: config.clone(),
        })
    }

    /// Downloads the snapshot file specified by the metadata.
    /// Returns the path to the downloaded file.
    pub async fn download(&self, snapshot: &Snapshot) -> Result<String, DownloadError> {
        let file_path = snapshot.file_path(&self.network_dir);
        self.download_from_url(&snapshot.url, &file_path).await?;
        Ok(file_path)
    }

    /// Downloads a gzip-compressed snapshot from the given URL, decompresses it on-the-fly,
    /// and saves the decompressed CBOR data to the specified output path.
    /// The data is first written to a `.partial` temporary file to ensure atomicity
    /// and then renamed to the final output path upon successful completion.
    async fn download_from_url(&self, url: &str, output_path: &str) -> Result<(), DownloadError> {
        let path = Path::new(output_path);

        if path.exists() {
            info!("Snapshot already exists, skipping: {}", output_path);
            return Ok(());
        }

        info!("Downloading snapshot from {}", url);

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| DownloadError::CreateDirectory(parent.to_path_buf(), e))?;
        }

        let tmp_path = path.with_extension("partial");

        let result = async {
            let response = self
                .client
                .get(url)
                .send()
                .await
                .map_err(|e| DownloadError::RequestFailed(url.to_string(), e))?;

            if !response.status().is_success() {
                return Err(DownloadError::InvalidStatusCode(
                    url.to_string(),
                    response.status(),
                ));
            }

            let content_length = response.content_length();
            let mut file = File::create(&tmp_path).await?;

            let stream = response.bytes_stream().map_err(io::Error::other);
            let async_read = tokio_util::io::StreamReader::new(stream);
            let progress_reader =
                ProgressReader::new(async_read, content_length, self.cfg.progress_log_interval);
            let buffered = BufReader::new(progress_reader);
            let mut decoder = GzipDecoder::new(buffered);

            tokio::io::copy(&mut decoder, &mut file).await?;

            file.sync_all().await?;
            tokio::fs::rename(&tmp_path, output_path).await?;

            info!("Downloaded and decompressed snapshot to {}", output_path);
            Ok(())
        }
        .await;

        if result.is_err() {
            let _ = tokio::fs::remove_file(&tmp_path).await;
        }

        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use flate2::Compression;
    use std::io::Write;
    use tempfile::TempDir;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const TEST_POINT: &str =
        "134956789.6558deef007ba372a414466e49214368c17c1f8428093193fc187d1c4587053c";

    fn gzip_compress(data: &[u8]) -> Vec<u8> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn default_config() -> DownloadConfig {
        DownloadConfig::default()
    }

    fn test_snapshot(url: String) -> Snapshot {
        Snapshot {
            epoch: 509,
            point: TEST_POINT.to_string(),
            url,
        }
    }

    #[tokio::test]
    async fn test_downloader_skips_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        let network_dir = temp_dir.path().to_str().unwrap();
        let snapshot = test_snapshot("https://example.com/snapshot.cbor.gz".to_string());

        // Create file at the exact path the downloader will check
        let expected_path = snapshot.file_path(network_dir);
        std::fs::write(&expected_path, b"existing data").unwrap();

        let downloader = SnapshotDownloader::new(network_dir, &default_config()).unwrap();
        let result = downloader.download(&snapshot).await;

        assert!(result.is_ok());
        assert_eq!(std::fs::read(&expected_path).unwrap(), b"existing data");
    }

    #[tokio::test]
    async fn test_downloader_downloads_and_decompresses() {
        let mock_server = MockServer::start().await;
        let compressed = gzip_compress(b"snapshot content");

        Mock::given(method("GET"))
            .and(path("/snapshot.cbor.gz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(compressed))
            .mount(&mock_server)
            .await;

        let temp_dir = TempDir::new().unwrap();
        let network_dir = temp_dir.path().to_str().unwrap();
        let snapshot = test_snapshot(format!("{}/snapshot.cbor.gz", mock_server.uri()));

        let downloader = SnapshotDownloader::new(network_dir, &default_config()).unwrap();
        let result = downloader.download(&snapshot).await;

        assert!(result.is_ok());
        let downloaded_path = result.unwrap();
        assert_eq!(downloaded_path, snapshot.file_path(network_dir));
        assert_eq!(
            std::fs::read(&downloaded_path).unwrap(),
            b"snapshot content"
        );
    }

    #[tokio::test]
    async fn test_downloader_handles_http_error() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/snapshot.cbor.gz"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&mock_server)
            .await;

        let temp_dir = TempDir::new().unwrap();
        let network_dir = temp_dir.path().to_str().unwrap();
        let snapshot = test_snapshot(format!("{}/snapshot.cbor.gz", mock_server.uri()));

        let downloader = SnapshotDownloader::new(network_dir, &default_config()).unwrap();
        let result = downloader.download(&snapshot).await;

        assert!(matches!(
            result,
            Err(DownloadError::InvalidStatusCode(_, _))
        ));
        assert!(!Path::new(&snapshot.file_path(network_dir)).exists());
    }

    #[tokio::test]
    async fn test_downloader_creates_parent_directories() {
        let mock_server = MockServer::start().await;
        let compressed = gzip_compress(b"data");

        Mock::given(method("GET"))
            .and(path("/snapshot.cbor.gz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(compressed))
            .mount(&mock_server)
            .await;

        let temp_dir = TempDir::new().unwrap();
        let network_dir = temp_dir.path().join("nested").join("dir");
        let network_dir_str = network_dir.to_str().unwrap();
        let snapshot = test_snapshot(format!("{}/snapshot.cbor.gz", mock_server.uri()));

        let downloader = SnapshotDownloader::new(network_dir_str, &default_config()).unwrap();
        let result = downloader.download(&snapshot).await;

        assert!(result.is_ok());
        assert!(Path::new(&snapshot.file_path(network_dir_str)).exists());
    }

    #[tokio::test]
    async fn test_downloader_cleans_up_partial_on_failure() {
        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/snapshot.cbor.gz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"not valid gzip"))
            .mount(&mock_server)
            .await;

        let temp_dir = TempDir::new().unwrap();
        let network_dir = temp_dir.path().to_str().unwrap();
        let snapshot = test_snapshot(format!("{}/snapshot.cbor.gz", mock_server.uri()));

        let downloader = SnapshotDownloader::new(network_dir, &default_config()).unwrap();
        let result = downloader.download(&snapshot).await;

        assert!(result.is_err());

        let file_path = snapshot.file_path(network_dir);
        let partial_path = Path::new(&file_path).with_extension("partial");
        assert!(!Path::new(&file_path).exists());
        assert!(!partial_path.exists());
    }

    #[tokio::test]
    async fn test_downloader_with_custom_config() {
        let mock_server = MockServer::start().await;
        let compressed = gzip_compress(b"data");

        Mock::given(method("GET"))
            .and(path("/snapshot.cbor.gz"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(compressed))
            .mount(&mock_server)
            .await;

        let temp_dir = TempDir::new().unwrap();
        let network_dir = temp_dir.path().to_str().unwrap();
        let config = DownloadConfig {
            timeout_secs: 600,
            connect_timeout_secs: 60,
            progress_log_interval: 100,
        };

        let downloader = SnapshotDownloader::new(network_dir, &config).unwrap();
        let snapshot = test_snapshot(format!("{}/snapshot.cbor.gz", mock_server.uri()));
        let result = downloader.download(&snapshot).await;

        assert!(result.is_ok());
    }
}
