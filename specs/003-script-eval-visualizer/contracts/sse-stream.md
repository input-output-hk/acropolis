# Contract: `GET /events` Server-Sent-Events stream

## Endpoint

| Property | Value |
|---|---|
| Method + path | `GET /events` |
| Response status | `200 OK` |
| `Content-Type` | `text/event-stream; charset=utf-8` |
| `Cache-Control` | `no-cache, no-transform` |
| `Connection` | `keep-alive` |
| `X-Accel-Buffering` | `no` (defensive, in case behind nginx) |
| Heartbeat | One SSE comment line `:heartbeat\n\n` every 15 s. |

## Event types

### `event: init` — sent exactly once, immediately after connection

```text
id: 0
event: init
data: {"cexplorerBaseUrl":"https://cexplorer.io","network":"mainnet"}

```

Fields:
- `cexplorerBaseUrl`: base URL the frontend prefixes to `/block/{hash}` and `/tx/{hash}` for cexplorer links. Derived from the node's network at module init time. (See research.md Q5.)
- `network`: lowercase network name (`"mainnet"`, `"preprod"`, `"preview"`, etc.).

### `event: script_eval` — one per Plutus script evaluation

```text
id: <u64, monotonically increasing>
event: script_eval
data: {<JSON object — schema below>}

```

JSON shape:

```json
{
  "id": 12345,
  "epoch": 502,
  "slot": 134567890,
  "blockNumber": 11234567,
  "blockHash": "abc123...",
  "txHash": "def456...",
  "scriptHash": "0123ab...",
  "purpose": "spend",
  "plutusVersion": "v3",
  "mem": 12345678,
  "cpu": 9876543210,
  "memBudget": 14000000,
  "cpuBudget": 10000000000,
  "success": true,
  "error": null
}
```

- `purpose`: one of `"spend"`, `"mint"`, `"cert"`, `"reward"`, `"vote"`, `"propose"`.
- `plutusVersion`: one of `"v1"`, `"v2"`, `"v3"`.
- `slot`: absolute slot number of the block containing the transaction (`BlockInfo.slot`).
- `mem`, `cpu`, `memBudget`, `cpuBudget`: u64 numbers (JSON numbers; values may exceed 2^53 in theory, but in practice mainnet ex-units fit comfortably in JS doubles — documented limitation).
- `error`: omitted (or `null`) on success; on failure, a short string (≤ 512 chars).

### `event: lagged` — sent when the broadcast stream falls behind for this client

```text
event: lagged
data: {"skipped": <n>}

```

The browser does not need to act on this; it is informational.

## Connection lifecycle

- The server holds the connection open indefinitely.
- The server MUST send an `init` event before any `script_eval` event.
- If the underlying `tokio::sync::broadcast` channel signals `RecvError::Lagged(n)`, the server emits one `lagged` event with `skipped = n` and continues.
- If the underlying channel signals `RecvError::Closed` (module shutting down), the server gracefully closes the response.
- Reconnection: the browser uses `EventSource`, which auto-reconnects. The server does not honor `Last-Event-ID` (no replay; see research.md Q6).

## Failure modes

- Serialization of an event fails → server logs at `WARN`, drops *that* event only, continues serving.
- Client disconnects → server detects via the broken stream, drops its receiver, continues.
- Module shutting down → in-flight connections are closed; new connections refused.
