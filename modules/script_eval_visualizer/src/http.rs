//! Embedded HTTP server: serves the static frontend and the live SSE stream.
//!
//! Endpoints:
//! - `GET /` — the embedded `index.html` page.
//! - `GET /events` — Server-Sent-Events stream, one event per script
//!   evaluation, plus a single `init` event on connect.
//! - `GET /healthz` — liveness check.

use std::convert::Infallible;
use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Result;
use axum::extract::State;
use axum::http::header;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio_stream::wrappers::errors::BroadcastStreamRecvError;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tracing::{info, warn};

use crate::stream::ScriptEvalEvent;

/// Static HTML page served at `GET /`.
const INDEX_HTML: &str = include_str!("assets/index.html");

/// Application state shared with axum handlers.
#[derive(Clone)]
pub struct AppState {
    /// One-end of the broadcast channel that fan-out pushes events into.
    /// Each new SSE connection calls `subscribe()` to obtain a fresh receiver.
    pub events: broadcast::Sender<ScriptEvalEvent>,

    /// Cexplorer.io base URL (network-dependent), sent in the `init` SSE event.
    pub cexplorer_base_url: String,

    /// Lowercase network name, sent in the `init` SSE event.
    pub network: String,
}

/// Build the axum router with all endpoints attached.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/", get(index_handler))
        .route("/events", get(events_handler))
        .route("/healthz", get(healthz_handler))
        .with_state(state)
}

/// Run the HTTP server forever on `addr`.
pub async fn serve(addr: SocketAddr, state: AppState) -> Result<()> {
    let app = router(state);
    let listener = TcpListener::bind(addr).await?;
    info!("script-eval-visualizer HTTP listening on http://{addr}/");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index_handler() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        INDEX_HTML,
    )
}

async fn healthz_handler() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], "ok")
}

/// Build the JSON payload for the `init` SSE event.
///
/// Extracted so tests can assert the exact wire shape without spinning up the
/// HTTP server.
pub fn build_init_payload(cexplorer_base_url: &str, network: &str) -> serde_json::Value {
    json!({
        "cexplorerBaseUrl": cexplorer_base_url,
        "network": network,
    })
}

async fn events_handler(State(state): State<AppState>) -> impl IntoResponse {
    let receiver = state.events.subscribe();

    // Build the eager `init` event that every new client must receive first.
    let init_event = Event::default()
        .id("0")
        .event("init")
        .data(build_init_payload(&state.cexplorer_base_url, &state.network).to_string());
    let init_stream = tokio_stream::iter(std::iter::once(Ok::<Event, Infallible>(init_event)));

    let live = BroadcastStream::new(receiver).filter_map(|item| match item {
        Ok(event) => match serde_json::to_string(&event) {
            Ok(payload) => Some(Ok::<Event, Infallible>(
                Event::default().id(event.id.to_string()).event("script_eval").data(payload),
            )),
            Err(e) => {
                warn!("failed to serialize ScriptEvalEvent {}: {e}", event.id);
                None
            }
        },
        Err(BroadcastStreamRecvError::Lagged(n)) => Some(Ok::<Event, Infallible>(
            Event::default().event("lagged").data(json!({ "skipped": n }).to_string()),
        )),
    });

    let combined = init_stream.chain(live);
    Sse::new(combined).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
