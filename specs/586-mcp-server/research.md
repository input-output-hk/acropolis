# Research: MCP Server for Acropolis

**Phase 0 output for spec `586-mcp-server`.**

This document captures the design questions weighed during implementation of the MCP server and the rationale for each choice. It is written retroactively to explain why the code looks the way it does.

## R-001: Which MCP SDK?

**Decision**: Use the `rmcp` Rust crate at version `0.8` with the `transport-streamable-http-server` feature.

**Alternatives considered**:
- Hand-rolled JSON-RPC framing over `axum` — rejected: MCP 1.0 has enough surface area (capabilities negotiation, resource templating, content types, error codes) that reimplementing it would be net negative on maintenance.
- Use the Python or TypeScript reference MCP servers and shell out — rejected: pulling another runtime into the omnibus process is heavier than adopting a Rust crate.

**Notes**: Pin the version. PR #669 (and lesson [[L012]]) flag unpinned git dependencies as a reproducibility hazard; `rmcp` is on crates.io so a numeric pin is sufficient.

## R-002: Which transport?

**Decision**: HTTP with Streamable HTTP transport (`StreamableHttpService` over `axum`), exposed at `/mcp` on a configurable address and port (default `127.0.0.1:4341`).

**Alternatives considered**:
- **stdio transport** — the most common MCP transport and the simplest. Rejected because the omnibus process already owns stdio for tracing/logging; a second consumer of stdin/stdout would race with logs and break interactive use.
- **WebSocket transport** — rejected because `rmcp` did not ship a stable WebSocket server at the time of implementation, and HTTP/SSE is already widely supported by MCP clients (Claude Code, MCP Inspector).

**Consequence**: Clients that only speak stdio (Claude Desktop at the time of writing) need a thin adapter or the `processes/mcp_standalone/` binary. This is documented in the module README.

## R-003: How is the API surface kept in sync between REST and MCP?

**Decision**: A single `pub const ROUTES: &[RouteDefinition]` in `rest_blockfrost/src/routes.rs` is the source of truth. The REST module mounts handlers from it; the MCP module derives tools and resources from it.

**Alternatives considered**:
- Maintain two parallel registries — rejected: doubles the maintenance cost and guarantees drift.
- Generate routes from `axum` router metadata at runtime — rejected: `axum` does not preserve URI parameter names or human-readable descriptions, both of which MCP needs.
- Macro-driven generation from the handler function signatures — rejected as over-engineering for ~60 entries; the explicit table is greppable and reviewable.

**Notes**: The handler reuse here is what removes the duplication lesson [[L006]] warns about — both surfaces share the same `handle_*` functions.

## R-004: Tools vs. resources — pick one, or expose both?

**Decision**: Expose every route as both a tool and a resource.

**Rationale**:
- **Tools** (`get_epoch_information`) are easier for the model to call directly with structured arguments and are how Claude Code and Copilot prefer to interact.
- **Resources** (`blockfrost://epochs/latest`) give browse-style clients and resource-aware UIs a uniform addressing scheme.
- Both views are cheap to derive from the same `RouteDefinition`, so exposing both is essentially free.

**Alternatives considered**:
- Tools only — rejected: a real cost only for resource-oriented clients but no win for tool-oriented ones.
- Resources only — rejected: tools are the de-facto MCP interaction model today.

## R-005: How are tool input schemas generated?

**Decision**: Derive a JSON schema for each tool from `RouteDefinition.param_names` plus the `HandlerType` flag:
- Every entry in `param_names` becomes a required string parameter.
- `HandlerType::WithQuery` adds an optional `query` parameter for the request's query string.

**Rationale**: The REST handlers take their path parameters as strings and accept arbitrary query strings, so a string-typed schema is honest about what the handler will accept. Inventing tighter types (numeric `slot`, hex-encoded `hash`) would have to be specified per route, which the registry does not currently model — keep that ambition for a follow-up rather than blocking the MVP.

## R-006: Default port?

**Decision**: 4341 (one above the existing REST port 4340).

**Notes**: Arbitrary, but consistent with the REST module's port and unlikely to clash with anything documented in this repo. PR #595 explicitly flagged this as a reviewer question — no objection was raised.

## R-007: How does the listener interact with module init?

**Decision**: The listener is spawned in a `tokio::spawn` task; `init` returns `Ok(())` immediately after the spawn.

**Rationale**:
- Caryatid module init is expected to be fast — blocking it on `axum::serve` would prevent other modules from initializing.
- The spawned task takes ownership of all the values it needs; nothing in the module body holds onto the listener.
- A failure inside the task is logged via `tracing::error!`. The wider process continues running; the operator notices via logs.

**Trade-off**: A late bind failure (port in use, address unparseable) surfaces as an `error` log rather than aborting the process. This is acceptable for an opt-in development feature. A production-grade rework would surface the bind result back to the caller before reporting "started".

## R-008: Where do per-request logs go?

**Decision**: `tracing::info!` at the entry of `list_resources`, `read_resource`, `list_tools`, and `call_tool`, with the relevant URI / tool name in the message.

**Rationale**: This server is operator-facing and used for debugging AI workflows; info-level visibility on every request is appropriate during the prototype phase. A future polish task can demote routine calls to debug once the surface is mature.

## R-009: Security posture

**Decision**: Bind `127.0.0.1` by default, no authentication, no rate limiting. Document explicitly that production deployments need network-level isolation.

**Alternatives considered**:
- Add a shared-secret header check — deferred: not in PR #595's scope; the spec marks adding auth as an explicit reviewer question.
- Require TLS — deferred for the same reason; `axum` supports it but the operational story (cert management) is out of scope here.

## R-010: Standalone binary

**Decision**: Reserve `processes/mcp_standalone/` for a future stdio-transport binary aimed at clients (Claude Desktop) that cannot use HTTP. The directory currently contains only a `Cargo.toml`.

**Notes**: Worth keeping as a deliberate placeholder rather than deleting — the README points at it and the standalone test script exists for the eventual implementation.
