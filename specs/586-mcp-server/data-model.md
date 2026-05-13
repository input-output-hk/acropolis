# Data Model: MCP Server for Acropolis

**Phase 1 output for spec `586-mcp-server`.**

This document captures the durable data types introduced by this feature and how the MCP-protocol-level concepts (tools, resources, server info) are derived from them. There is no database, no on-disk format, and no message-bus type added: the entire data model is a shared registry consumed by both the REST and MCP modules.

## RouteDefinition (source of truth)

Defined in `modules/rest_blockfrost/src/routes.rs`. One static, hand-maintained `RouteDefinition` per endpoint.

| Field | Type | Purpose |
|---|---|---|
| `topic_pattern` | `&'static str` | Caryatid topic this route binds to, e.g. `"rest.get.epochs.*"`. Used by the REST router. |
| `rest_path` | `&'static str` | REST path template, e.g. `"/epochs/{number}"`. Used by the REST router and by humans reading the file. |
| `mcp_uri_template` | `&'static str` | MCP resource URI template, e.g. `"blockfrost://epochs/{number}"`. The MCP module advertises this verbatim. |
| `name` | `&'static str` | Human-readable name, e.g. `"Epoch Information"`. Used as both the MCP resource name and the seed for the MCP tool's display title. |
| `description` | `&'static str` | One-sentence description of what the endpoint returns. Used as the MCP description on both tools and resources. |
| `handler_type` | `HandlerType` | Discriminator: `PathOnly` or `WithQuery`. Drives JSON-schema generation for the tool. |
| `handler_name` | `&'static str` | Name of the handler function for traceability — not used at runtime. |
| `param_names` | `&'static [&'static str]` | Ordered list of path parameter names. Defines required tool arguments and is used by the URI matcher when resolving a resource read. |

### Invariants

- For any `RouteDefinition`, every `{name}` placeholder in `rest_path` and `mcp_uri_template` MUST appear in `param_names`, in order.
- `mcp_uri_template` MUST start with `blockfrost://` so the URI scheme uniquely identifies an Acropolis MCP resource.
- `name` MUST be unique across `ROUTES` — it is the basis for the tool's name and must not collide.

## HandlerType

```rust
enum HandlerType {
    PathOnly,   // handler accepts only path parameters
    WithQuery,  // handler accepts path parameters plus a query string
}
```

Drives one branch in MCP tool schema generation:
- `PathOnly`: schema has exactly the required path parameters.
- `WithQuery`: schema additionally has an optional `query` string parameter.

## ROUTES (the registry instance)

`pub const ROUTES: &[RouteDefinition]` is the registry. As of merge it contains 63 entries grouped by section (Accounts, Blocks, Governance – DReps, Governance – Proposals, Pools, Epochs, Assets, Addresses, Transactions). New endpoints are added by appending to this slice; both surfaces pick them up on the next rebuild.

## Derivations

### Registry → MCP resource list (`resources/list`)

For each `RouteDefinition r`, emit one `RawResource`:
- `uri = r.mcp_uri_template`
- `name = r.name`
- `description = r.description`
- `mime_type = "application/json"`
- everything else nil/default

### Registry → MCP tool list (`tools/list`)

For each `RouteDefinition r`, emit one `Tool`:
- `name`: derived from `r.name` via snake_case normalization with a `get_` prefix (e.g. `"Epoch Information"` → `"get_epoch_information"`).
- `description = r.description`
- `inputSchema`: a JSON Schema `object` whose properties are each entry in `r.param_names` typed as `string` and listed in `required`. If `r.handler_type == WithQuery`, add an optional `query: string` property.

### Resource read → handler dispatch

Given an incoming `resources/read` URI:
1. Match it against each `r.mcp_uri_template` in `ROUTES`, extracting the values of `r.param_names`.
2. The first match wins. Reconstruct the equivalent REST request from the matched `RouteDefinition.rest_path` and call the corresponding handler from `rest_blockfrost`.
3. Wrap the handler's JSON return value in an MCP `TextResourceContents` with `mime_type = "application/json"`.

### Tool call → handler dispatch

Given an incoming `tools/call` with `name = T` and `arguments = A`:
1. Find the `RouteDefinition` whose derived tool name equals `T`.
2. Read each `param_names[i]` out of `A` (required, string).
3. If `WithQuery`, also read the optional `query` field.
4. Dispatch to the same handler the REST module would have invoked.
5. Wrap the JSON result as the tool's textual content.

## Server identity (advertised in `get_info`)

| Field | Value |
|---|---|
| `server_info.name` | `"acropolis-mcp"` |
| `server_info.title` | `"Acropolis MCP Server"` |
| `server_info.version` | `env!("CARGO_PKG_VERSION")` of the `mcp_server` crate |
| `capabilities` | `resources` + `tools` enabled |
| `instructions` | A short sentence telling the model that this server exposes Blockfrost-compatible endpoints and naming a few representative tools. |

## Things explicitly *not* in the data model

- No persistent storage. The server is a thin protocol translator.
- No session-scoped state. Each MCP session is independent.
- No new message bus types. The MCP module reuses what `rest_blockfrost` already publishes/subscribes to.
- No per-route auth or rate-limit metadata. (Deferred — see [[research]] R-009.)
