use std::pin::Pin;
use std::task::{Context, Poll};
use tracing::info;

pub struct ProgressReader<R> {
    inner: R,
    bytes_read: u64,
    last_log: u64,
    log_interval: u64,
    total_size: Option<u64>,
}

impl<R> ProgressReader<R> {
    pub fn new(inner: R, total_size: Option<u64>, log_interval_mb: u64) -> Self {
        Self {
            inner,
            bytes_read: 0,
            last_log: 0,
            log_interval: log_interval_mb * 1024 * 1024,
            total_size,
        }
    }
}

impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for ProgressReader<R> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let result = Pin::new(&mut self.inner).poll_read(cx, buf);
        let after = buf.filled().len();
        let bytes_read = (after - before) as u64;

        self.bytes_read += bytes_read;

        if self.bytes_read - self.last_log >= self.log_interval {
            if let Some(total) = self.total_size {
                let percent = (self.bytes_read as f64 / total as f64) * 100.0;
                info!(
                    "Download progress: {:.1}% ({} MB / {} MB)",
                    percent,
                    self.bytes_read / (1024 * 1024),
                    total / (1024 * 1024)
                );
            } else {
                info!("Downloaded {} MB", self.bytes_read / (1024 * 1024));
            }
            self.last_log = self.bytes_read;
        }

        result
    }
}
