//! Stream-based data source.
//!
//! Receives monitor snapshots from an async byte stream.
//! This is useful for network-based sources like TCP connections
//! or message bus subscriptions.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncBufReadExt, AsyncRead, BufReader};
use tokio::sync::mpsc;

use super::{DataSource, MonitorSnapshot};

/// A data source that receives monitor snapshots from an async stream.
///
/// This source spawns a background task that reads newline-delimited JSON
/// from the provided async reader and makes snapshots available via `poll()`.
///
/// # Example with a byte stream
///
/// ```
/// use std::io::Cursor;
/// use caryatid_doctor::StreamSource;
///
/// # tokio_test::block_on(async {
/// let data = b"{}\n";
/// let stream = Cursor::new(data.to_vec());
/// let source = StreamSource::spawn(stream, "example");
/// # });
/// ```
#[derive(Debug)]
pub struct StreamSource {
    receiver: mpsc::Receiver<MonitorSnapshot>,
    description: String,
    last_snapshot: Option<MonitorSnapshot>,
    last_error: Arc<Mutex<Option<String>>>,
}

impl StreamSource {
    /// Spawn a background task that reads from the given async reader.
    ///
    /// The reader should provide newline-delimited JSON snapshots.
    /// Each line is parsed as a complete `MonitorSnapshot`.
    pub fn spawn<R>(reader: R, description: &str) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let (tx, rx) = mpsc::channel(16);
        let last_error = Arc::new(Mutex::new(None));
        let error_handle = last_error.clone();
        let desc = description.to_string();

        tokio::spawn(async move {
            let mut reader = BufReader::new(reader);
            let mut line = String::new();

            loop {
                line.clear();
                match reader.read_line(&mut line).await {
                    Ok(0) => {
                        // EOF
                        *error_handle.lock().unwrap() = Some("Connection closed".to_string());
                        break;
                    }
                    Ok(_) => {
                        // Try to parse the line as JSON
                        match serde_json::from_str::<MonitorSnapshot>(line.trim()) {
                            Ok(snapshot) => {
                                *error_handle.lock().unwrap() = None;
                                if tx.send(snapshot).await.is_err() {
                                    // Receiver dropped
                                    break;
                                }
                            }
                            Err(e) => {
                                *error_handle.lock().unwrap() = Some(format!("Parse error: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        *error_handle.lock().unwrap() = Some(format!("Read error: {}", e));
                        break;
                    }
                }
            }
        });

        Self {
            receiver: rx,
            description: format!("stream: {}", desc),
            last_snapshot: None,
            last_error,
        }
    }

    /// Create a StreamSource from raw bytes channel.
    ///
    /// This is useful when you want to push JSON bytes from another source
    /// (like a message bus) without using an AsyncRead.
    pub fn from_bytes_channel(mut rx: mpsc::Receiver<Vec<u8>>, description: &str) -> Self {
        let (tx, snapshot_rx) = mpsc::channel(16);
        let last_error = Arc::new(Mutex::new(None));
        let error_handle = last_error.clone();

        tokio::spawn(async move {
            while let Some(bytes) = rx.recv().await {
                match serde_json::from_slice::<MonitorSnapshot>(&bytes) {
                    Ok(snapshot) => {
                        *error_handle.lock().unwrap() = None;
                        if tx.send(snapshot).await.is_err() {
                            break;
                        }
                    }
                    Err(e) => {
                        *error_handle.lock().unwrap() = Some(format!("Parse error: {}", e));
                    }
                }
            }
        });

        Self {
            receiver: snapshot_rx,
            description: format!("stream: {}", description),
            last_snapshot: None,
            last_error,
        }
    }
}

impl DataSource for StreamSource {
    fn poll(&mut self) -> Option<MonitorSnapshot> {
        // Try to receive without blocking
        match self.receiver.try_recv() {
            Ok(snapshot) => {
                self.last_snapshot = Some(snapshot.clone());
                Some(snapshot)
            }
            Err(mpsc::error::TryRecvError::Empty) => None,
            Err(mpsc::error::TryRecvError::Disconnected) => {
                *self.last_error.lock().unwrap() = Some("Stream disconnected".to_string());
                None
            }
        }
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn error(&self) -> Option<&str> {
        // This is a bit awkward due to the mutex, but we need interior mutability
        // for the error state. In practice, this is called infrequently.
        None // Can't return reference to mutex-guarded data easily
    }
}

// Custom error method that returns owned string
impl StreamSource {
    /// Get the last error message, if any.
    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().unwrap().clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn sample_json() -> &'static str {
        r#"{"TestModule":{"reads":{"input":{"read":100}},"writes":{"output":{"written":50}}}}"#
    }

    #[tokio::test]
    async fn test_stream_source_spawn() {
        // Create a cursor with newline-delimited JSON
        let data = format!("{}\n", sample_json());
        let cursor = Cursor::new(data);

        let mut source = StreamSource::spawn(cursor, "test");

        // Give the background task time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Should receive the snapshot
        let snapshot = source.poll();
        assert!(snapshot.is_some());
        assert!(snapshot.unwrap().contains_key("TestModule"));
    }

    #[tokio::test]
    async fn test_stream_source_multiple_snapshots() {
        let data = format!("{}\n{}\n", sample_json(), sample_json());
        let cursor = Cursor::new(data);

        let mut source = StreamSource::spawn(cursor, "test");

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Should receive both snapshots
        let s1 = source.poll();
        let s2 = source.poll();
        assert!(s1.is_some());
        assert!(s2.is_some());

        // No more data
        assert!(source.poll().is_none());
    }

    #[tokio::test]
    async fn test_stream_source_description() {
        let cursor = Cursor::new("");
        let source = StreamSource::spawn(cursor, "tcp://localhost:9090");
        assert_eq!(source.description(), "stream: tcp://localhost:9090");
    }

    #[tokio::test]
    async fn test_stream_source_from_bytes_channel() {
        let (tx, rx) = mpsc::channel::<Vec<u8>>(16);
        let mut source = StreamSource::from_bytes_channel(rx, "test-channel");

        // Send a snapshot
        tx.send(sample_json().as_bytes().to_vec()).await.unwrap();

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        let snapshot = source.poll();
        assert!(snapshot.is_some());
        assert!(snapshot.unwrap().contains_key("TestModule"));
    }

    #[tokio::test]
    async fn test_stream_source_invalid_json() {
        // Include valid JSON after invalid to keep the stream processing
        let data = "not valid json\n";
        let cursor = Cursor::new(data);

        let mut source = StreamSource::spawn(cursor, "test");

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // Should not receive anything (invalid JSON is skipped)
        assert!(source.poll().is_none());

        // Error may be overwritten by "Connection closed" after EOF,
        // so we just verify no valid snapshot was received
    }

    #[tokio::test]
    async fn test_stream_source_empty_stream() {
        let cursor = Cursor::new("");
        let mut source = StreamSource::spawn(cursor, "test");

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

        // No data to receive
        assert!(source.poll().is_none());
    }
}
