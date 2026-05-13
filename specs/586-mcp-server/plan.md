# Implementation Plan: MCP Server for AI Queries

**Branch**: `586-mcp-server` | **Date**: 2026-05-12 | **Spec**: [spec.md](./spec.md)
**Input**: Feature specification from `/specs/586-mcp-server/spec.md`

> **Note**: This plan is retroactive — the feature shipped as PR #595. The plan reflects the decisions actually made during implementation, not a pre-coding design exercise.

## Summary

Expose Acropolis's Blockfrost-compatible API surface to MCP-capable AI clients (Claude Code, VS Code Copilot, MCP Inspector) by adding an opt-in `mcp_server` Caryatid module to the omnibus process. The module hosts an HTTP/SSE server speaking MCP 1.0 via the `rmcp` crate and turns a shared `RouteDefinition` registry — owned by `rest_blockfrost` — into MCP tools and resources. The MCP module performs no query logic of its own; it dispatches each request to the same handler the REST module would have used.

## Technical Context

**Language/Version**: Rust 1.75+ (workspace pinned via root `Cargo.toml`)
**Primary Dependencies**:
- `rmcp = "0.8"` with features `["server", "transport-streamable-http-server"]` — MCP protocol implementation
- `axum` — HTTP server (workspace version, already used elsewhere)
- `tokio` — async runtime (workspace version)
- `caryatid_sdk` — module framework
- `acropolis_common` — shared types and config helpers
- `acropolis_module_rest_blockfrost` — handler reuse and the routes registry
- `serde_json`, `tracing`, `anyhow`, `config` — standard workspace dependencies

**Storage**: None. The module is stateless; all state lives in modules queried via the existing Caryatid message bus.
**Testing**: `cargo test` for unit coverage; a Python script (`scripts/test_mcp_standalone.py` / `scripts/test_mcp_server.py`) for integration-level verification driving the live HTTP endpoint. MCP Inspector (`npx @anthropics/mcp-inspector`) for ad-hoc exploration.
**Target Platform**: Linux/macOS server, same as the omnibus process. The HTTP transport is OS-agnostic.
**Project Type**: Single-project Rust workspace member (`modules/mcp_server/`) plus a thin `processes/mcp_standalone/` binary for clients that prefer a standalone process.
**Performance Goals**: Match the underlying REST handlers — MCP adds protocol framing only. No new scaling target.
**Constraints**:
- Cannot use stdio transport: the omnibus process owns stdio for logging.
- Must not block other modules during init — the listener spawns in a background task.
- Must default to disabled and localhost-only.
**Scale/Scope**: 63 endpoints exposed at the point of merge, growing as `routes.rs` grows. Concurrent MCP sessions limited only by tokio task budget.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

This project does not have a constitution file under `.specify/memory/constitution.md` enforcing programmatic gates for this feature beyond the general principles already encoded in `CLAUDE.md`:

- **Module isolation**: Satisfied — the MCP module touches no other module's state; it consumes the `rest_blockfrost` routes registry as a public symbol and dispatches through the message bus.
- **Publish-subscribe architecture**: Satisfied — the MCP module is a consumer of existing topics; it adds no new message types.
- **Opt-in feature flags**: Satisfied — disabled by default, surfaced through `[module.mcp-server]` config.
- **No production claim**: Satisfied — the host is the omnibus process, which `CLAUDE.md` already labels as testing-only.

Re-check after Phase 1: no constitution violations introduced.

## Project Structure

### Documentation (this feature)

```text
specs/586-mcp-server/
├── plan.md              # This file
├── spec.md              # User-visible spec
├── research.md          # Phase 0: transport, handler-reuse, and registry decisions
├── data-model.md        # Phase 1: RouteDefinition shape and tool/resource derivation rules
├── quickstart.md        # Phase 1: how to enable, configure, and verify the server
├── contracts/
│   └── mcp-protocol.md  # MCP method coverage and JSON-schema conventions
└── tasks.md             # Phase 2: dependency-ordered implementation tasks
```

### Source Code (repository root)

```text
modules/
├── mcp_server/                       # New module
│   ├── Cargo.toml                    # Depends on rest_blockfrost for handlers + ROUTES
│   ├── README.md                     # Operator-facing docs
│   └── src/
│       ├── mcp_server.rs             # Module entry point + Caryatid lifecycle
│       ├── server.rs                 # ServerHandler impl, HTTP listener
│       ├── resources.rs              # URI parsing, resource dispatch, ROUTES → resource list
│       └── tools.rs                  # Tool-name + JSON-schema generation, tool dispatch
└── rest_blockfrost/
    └── src/
        └── routes.rs                 # NEW — single source of truth: RouteDefinition + ROUTES

processes/
├── omnibus/
│   ├── src/main.rs                   # Register MCPServer alongside existing modules
│   ├── omnibus.toml                  # New [module.mcp-server] section
│   └── omnibus-local.toml            # Matching local override
└── mcp_standalone/
    └── Cargo.toml                    # Thin standalone binary (currently a stub)

scripts/
├── test_mcp_server.py                # Drive the live MCP endpoint
└── test_mcp_standalone.py            # Variant for the standalone binary

.vscode/
└── mcp.json                          # Claude Code / VS Code Copilot integration target
```

**Structure Decision**: Standard Acropolis module under `modules/`, plus a passive change to `rest_blockfrost` to publish `RouteDefinition` and `ROUTES` as a public API. The MCP module depends on `rest_blockfrost` rather than the other way round so the existing REST module's surface area is untouched semantically and the new module is the only one carrying MCP-specific code.

## Complexity Tracking

> Filled because the implementation introduces a cross-module coupling (`mcp_server` → `rest_blockfrost`) that deserves explicit justification.

| Violation | Why Needed | Simpler Alternative Rejected Because |
|-----------|------------|-------------------------------------|
| MCP module depends directly on `rest_blockfrost` (not just on the common message bus) | Needs the `ROUTES` registry and the existing handler functions; routing requests purely via the message bus would require defining a parallel set of query topics and replaying handler logic | Pure message-bus routing would force a second handler registry and re-introduce the very duplication this feature is designed to eliminate (see US3/FR-002) |
| HTTP/SSE transport rather than stdio | The omnibus owns stdio for logging; HTTP also enables multiple concurrent AI clients out of the box | Stdio is the MCP "happy path" for single-process integrations, but it cannot coexist with the omnibus's existing stdio usage |
| Two binaries (`mcp_server` module hosted in omnibus + `mcp_standalone` process) | Some clients prefer a dedicated process; the standalone variant exists as a placeholder for that path without forcing the omnibus to give up the in-process route | Maintaining only the in-process module would leave stdio-only MCP clients (Claude Desktop today) unsupported in the long run |
