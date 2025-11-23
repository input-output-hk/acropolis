use crate::config::SnapshotFileMetadata;
use crate::progress_reader::ProgressReader;
use async_compression::tokio::bufread::GzipDecoder;
use futures_util::TryStreamExt;
use reqwest::Client;
use std::io;
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::fs::File;
use tokio::io::BufReader;
use tracing::info;

#[derive(Debug, Error)]
pub enum DownloadError {
    #[error("Failed to initialize HTTP client: {0}")]
    ClientInit(#[from] reqwest::Error),

    #[error("Failed to download snapshot from {0}: {1}")]
    Download(String, reqwest::Error),

    #[error("Download failed from {0}: HTTP status {1}")]
    InvalidStatusCode(String, reqwest::StatusCode),

    #[error("Cannot create directory {0}: {1}")]
    CreateDirectory(PathBuf, io::Error),

    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

/// Handles downloading and decompressing snapshot files
pub struct SnapshotDownloader {
    client: Client,
    network_dir: String,
}

impl SnapshotDownloader {
    pub fn new(network_dir: String) -> Result<Self, DownloadError> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_mins(5))
            .connect_timeout(std::time::Duration::from_secs(30))
            .build()?;

        Ok(Self {
            client,
            network_dir,
        })
    }

    pub async fn download_all(
        &self,
        snapshots: &[SnapshotFileMetadata],
    ) -> Result<(), DownloadError> {
        for snapshot_meta in snapshots {
            let file_path = snapshot_meta.file_path(&self.network_dir);
            self.download_single(&snapshot_meta.url, &file_path).await?;
        }
        Ok(())
    }

    /// Downloads a gzip-compressed snapshot from the given URL, decompresses it on-the-fly,
    /// and saves the decompressed CBOR data to the specified output path.
    /// The data is first written to a `.partial` temporary file to ensure atomicity
    /// and then renamed to the final output path upon successful completion.
    pub async fn download_single(&self, url: &str, output_path: &str) -> Result<(), DownloadError> {
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
                .map_err(|e| DownloadError::Download(url.to_string(), e))?;

            if !response.status().is_success() {
                return Err(DownloadError::InvalidStatusCode(
                    url.to_string(),
                    response.status(),
                ));
            }

            let content_length = response.content_length();
            let mut file = File::create(&tmp_path).await?;

            let stream = response.bytes_stream();
            let async_read =
                tokio_util::io::StreamReader::new(stream.map_err(|e| std::io::Error::other(e)));

            let progress_reader = ProgressReader::new(async_read, content_length, 200);
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
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_fake_snapshot(dir: &Path, point: &str) {
        let snapshot_path = dir.join(format!("{}.cbor", point));
        let mut file = fs::File::create(&snapshot_path).unwrap();
        file.write_all(b"fake snapshot data").unwrap();
    }

    #[tokio::test]
    async fn test_downloader_skips_existing_file() {
        let temp_dir = TempDir::new().unwrap();
        let point = "point_500";
        create_fake_snapshot(temp_dir.path(), point);

        let file_path = temp_dir.path().join(format!("{}.cbor", point));
        let downloader =
            SnapshotDownloader::new(temp_dir.path().to_str().unwrap().to_string()).unwrap();

        let result = downloader
            .download_single(
                "https://example.com/snapshot.cbor.gz",
                file_path.to_str().unwrap(),
            )
            .await;

        assert!(result.is_ok());
        assert!(file_path.exists());
    }

    #[tokio::test]
    async fn test_downloader_missing_file_fails() {
        let temp_dir = TempDir::new().unwrap();
        let point = "point_500";
        let file_path = temp_dir.path().join(format!("{}.cbor", point));
        let downloader =
            SnapshotDownloader::new(temp_dir.path().to_str().unwrap().to_string()).unwrap();

        let result = downloader
            .download_single(
                "https://invalid-url-that-does-not-exist.com/snapshot.cbor.gz",
                file_path.to_str().unwrap(),
            )
            .await;

        assert!(result.is_err());
        assert!(!file_path.exists());
    }

    #[tokio::test]
    async fn test_downloader_creates_directory() {
        let temp_dir = TempDir::new().unwrap();
        let nested_path = temp_dir.path().join("nested").join("directory").join("snapshot.cbor");
        let downloader =
            SnapshotDownloader::new(temp_dir.path().to_str().unwrap().to_string()).unwrap();

        let _ = downloader
            .download_single(
                "https://invalid-url.com/snapshot.cbor.gz",
                nested_path.to_str().unwrap(),
            )
            .await;

        assert!(nested_path.parent().unwrap().exists());
    }

    #[tokio::test]
    async fn test_downloader_creates_partial_file_then_renames() {
        let temp_dir = TempDir::new().unwrap();
        let output_path = temp_dir.path().join("snapshot.cbor");
        let downloader =
            SnapshotDownloader::new(temp_dir.path().to_str().unwrap().to_string()).unwrap();

        let result = downloader
            .download_single(
                "https://invalid-url.com/snapshot.cbor.gz",
                output_path.to_str().unwrap(),
            )
            .await;

        assert!(result.is_err());
        assert!(!output_path.exists());
    }
}
