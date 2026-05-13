# Quickstart: MCP Server for Acropolis

This walks an operator from a clean checkout to a working MCP session against the running omnibus.

## Prerequisites

- An Acropolis checkout that builds cleanly (`make build`).
- One of the following MCP clients:
  - Claude Code (uses `.vscode/mcp.json` automatically when present).
  - VS Code with the MCP extension.
  - `npx @anthropics/mcp-inspector` for ad-hoc testing.

## 1. Enable the module

Open `processes/omnibus/omnibus.toml` and confirm the `[module.mcp-server]` section is present and enabled:

```toml
[module.mcp-server]
enabled = true
address = "127.0.0.1"   # use 0.0.0.0 only if you understand the risk
port = 4341
```

The default is `enabled = false` on a fresh deploy. The module logs `MCP server is disabled in configuration` and binds no port when disabled.

## 2. Run the omnibus

```bash
make run
```

You should see, in order:

```
Initializing MCP server on 127.0.0.1:4341
Starting Acropolis MCP server on http://127.0.0.1:4341/mcp
MCP server listening on http://127.0.0.1:4341/mcp
MCP server started
```

If the port is busy you will see a `tracing::error!` line; the rest of the node still runs.

## 3. Confirm the server with MCP Inspector

```bash
npx @anthropics/mcp-inspector
```

Configure the Inspector with:

- Transport: **HTTP (Streamable HTTP)**
- URL: `http://127.0.0.1:4341/mcp`

Verify:

- `resources/list` returns ~63 entries, every URI starting with `blockfrost://`.
- `tools/list` returns the same count, each tool name in `get_*` form.
- `resources/read` on `blockfrost://epochs/latest` returns a JSON object describing the current epoch.
- `tools/call` on `get_epoch_information` with `{ "number": "latest" }` returns the same payload.

## 4. Connect Claude Code

Make sure `.vscode/mcp.json` exists at the repo root with:

```json
{
  "servers": {
    "acropolis": {
      "type": "http",
      "url": "http://localhost:4341/mcp"
    }
  }
}
```

Open Claude Code in this workspace and confirm the `acropolis` MCP server appears as connected. Ask:

> "What's the current epoch on this node, and which stake pool produced the most blocks in it?"

Claude should call `get_epoch_information` and one of the pool tools, then synthesize an answer.

## 5. Run the smoke-test script

```bash
python3 scripts/test_mcp_server.py
```

The script drives the protocol directly and prints PASS/FAIL for each step. Use this to confirm the server is up before debugging client-side problems.

## Troubleshooting

| Symptom | Likely cause |
|---|---|
| `Address already in use` in the omnibus logs | Another process owns port 4341. Change `port` or stop the other process. |
| Client connects but `tools/list` is empty | The `mcp_server` crate did not link against the `rest_blockfrost` routes — check `Cargo.toml` shows the path dependency. |
| `resources/read` returns "no matching route" | The URI in the request does not match any `mcp_uri_template`. Compare against `tools/list`. |
| Server logs every call as info and is noisy | Lower `RUST_LOG` to `warn` or filter on `acropolis_module_mcp_server=warn`. |
| Need to expose to another machine | Set `address = "0.0.0.0"` *and* add a firewall rule. The server has no authentication; do not put it on a public interface. |
