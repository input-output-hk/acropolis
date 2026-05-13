# Contract: MCP Protocol Coverage

This document is the wire-level contract the MCP server promises to its clients. It lists the MCP methods the server implements, what it returns, and how each one ties back to the shared routes registry.

## Server identity

`get_info` returns:

| Field | Value |
|---|---|
| `protocol_version` | rmcp default (MCP 1.0) |
| `server_info.name` | `"acropolis-mcp"` |
| `server_info.title` | `"Acropolis MCP Server"` |
| `server_info.version` | crate version of `acropolis_module_mcp_server` |
| `capabilities` | `resources` and `tools` enabled |
| `instructions` | One-sentence prompt describing the server's purpose and naming a few tools |

## Methods implemented

### `resources/list`

- **Input**: optional pagination cursor (ignored — single page).
- **Output**: one entry per `RouteDefinition` in `rest_blockfrost::ROUTES`:
  - `uri`: the route's `mcp_uri_template` (e.g. `blockfrost://epochs/{number}`)
  - `name`: the route's `name`
  - `description`: the route's `description`
  - `mime_type`: `"application/json"`
- **Pagination**: `next_cursor = None`. All resources are returned in one response.

### `resources/read`

- **Input**: `{ uri: string }`.
- **Behavior**:
  1. Walk `ROUTES`, find the first entry whose `mcp_uri_template` matches the requested URI, extracting `param_names`.
  2. Call the corresponding handler in `rest_blockfrost` via the existing message bus / handler-table path.
  3. Return the handler's JSON response as `TextResourceContents` with `mime_type = "application/json"` and `text = serde_json::to_string_pretty(value)`.
- **Errors**:
  - No matching template → `INTERNAL_ERROR` with `"Failed to handle resource: <reason>"`.
  - Handler failure → `INTERNAL_ERROR` with the underlying error message.
  - JSON serialization failure → `INTERNAL_ERROR` with the serde error.

### `tools/list`

- **Input**: optional pagination cursor (ignored).
- **Output**: one tool per `RouteDefinition` with:
  - `name`: snake_cased, `get_`-prefixed transformation of `route.name` (e.g. `"Epoch Information"` → `"get_epoch_information"`).
  - `description`: the route's `description`.
  - `inputSchema`: a JSON Schema object whose required `properties` are the entries in `route.param_names`, each typed `string`. If `route.handler_type == WithQuery`, an additional optional `query: string` property is added.

### `tools/call`

- **Input**: `{ name: string, arguments: object }`.
- **Behavior**:
  1. Match `name` against the tool list derived from `ROUTES`.
  2. Read each `param_names[i]` from `arguments` (required, must be a string).
  3. If `WithQuery`, optionally read `query` from `arguments`.
  4. Dispatch to the same handler the equivalent REST request would have hit.
  5. Return the handler's JSON result as the tool's textual content.
- **Errors**:
  - Unknown tool name → `INTERNAL_ERROR` with `"Tool call failed: <reason>"`.
  - Missing or wrongly-typed argument → `INTERNAL_ERROR` with the parameter name in the message.
  - Handler failure → `INTERNAL_ERROR` with the underlying error message.

## Transport contract

- HTTP server on `<address>:<port>` (default `127.0.0.1:4341`).
- MCP endpoint mounted at `/mcp` (e.g. `http://127.0.0.1:4341/mcp`).
- Underlying rmcp `StreamableHttpService` with `LocalSessionManager` — each HTTP client gets an isolated MCP session.
- No authentication, no rate limiting. See [[research]] R-009 for the rationale and the production-hardening notes.

## Logging contract

At info level, the server logs:

- Startup: `Initializing MCP server on <addr>:<port>`, `Starting Acropolis MCP server on http://<addr>:<port>/mcp`, `MCP server listening on http://<addr>:<port>/mcp`, `MCP server started`.
- Capability negotiation: `MCP get_info called - advertising <N> tools and <M> resources`.
- Per-request: `MCP client requested resource list`, `MCP client reading resource: <uri>`, `MCP client requested tools list - returning <N> tools`, `MCP client calling tool: <name>`.

## Out of scope

- Streaming/cursor pagination.
- Subscriptions / `resources/subscribe`.
- Prompts and sampling capabilities.
- TLS termination, auth, rate limiting (operator responsibility — see quickstart and research).
