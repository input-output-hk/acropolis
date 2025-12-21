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
/// # Example with TCP
///
/// ```ignore
/// use tokio::net::TcpStream;
/// use caryatid_doctor::StreamSource;
///
/// let stream = TcpStream::connect("localhost:9090").await?;
/// let source = StreamSource::spawn(stream, "tcp://localhost:9090");
/// let app = App::new(Box::new(source), Thresholds::default());
/// ```
///
/// # Example with message bus (bridging)
///
/// ```ignore
/// use caryatid_doctor::{ChannelSource, MonitorSnapshot};
///
/// // Create channel source
/// let (tx, source) = ChannelSource::create("rabbitmq");
///
/// // Bridge from your message bus subscription
/// tokio::spawn(async move {
///     let subscription = bus.subscribe("caryatid.monitor.snapshot").await?;
///     loop {
///         let (_, msg) = subscription.read().await?;
///         // Assuming your message type can be converted to JSON bytes
///         let snapshot: MonitorSnapshot = serde_json::from_slice(&msg.to_json())?;
///         let _ = tx.send(snapshot);
///     }
/// });
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
