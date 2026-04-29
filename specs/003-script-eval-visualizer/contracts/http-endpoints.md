# Contract: HTTP endpoints served by `script_eval_visualizer`

## Bind

| Setting | Default | Config key |
|---|---|---|
| Bind address | `127.0.0.1` | `bind-address` |
| Bind port | `8030` | `bind-port` |

Both keys live under `[module.script-eval-visualizer]` in `omnibus.toml`.

## Endpoints

### `GET /` → static HTML page

| Property | Value |
|---|---|
| Status | `200 OK` |
| `Content-Type` | `text/html; charset=utf-8` |
| `Cache-Control` | `no-cache` |
| Body | Contents of `modules/script_eval_visualizer/src/assets/index.html` (embedded via `include_str!`). |

The page is a single self-contained HTML document with inline `<style>` and `<script>` blocks. It opens an `EventSource("/events")` on load, maintains an in-memory deque of up to 1000 `script_eval` events (newest first), and re-renders the table on each event arrival.

### `GET /events` → SSE stream

See [sse-stream.md](./sse-stream.md).

### `GET /healthz` → liveness check

| Property | Value |
|---|---|
| Status | `200 OK` |
| `Content-Type` | `text/plain` |
| Body | `ok` |

Used by ops/monitoring to confirm the module is up. Does not depend on the Caryatid bus or any subscription state.

### Other paths

`404 Not Found`. No directory listing, no other endpoints.

## CORS / authentication

- No CORS headers are emitted; the page is served from the same origin as the SSE stream by design.
- No authentication. The default bind address is loopback, so the endpoint is operator-local.
- Operators who bind to a non-loopback address are responsible for fronting it with a reverse proxy that adds auth, per the spec's "Authentication / access control" assumption.

## Lifecycle

- Server starts in the module's `init`/`run`, after the Caryatid subscription is registered.
- Server runs until module shutdown. There is no graceful-drain step; in-flight SSE connections close when the underlying TCP listener drops.
