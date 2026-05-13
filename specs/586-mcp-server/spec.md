# Feature Specification: MCP Server for AI Queries

**Feature Branch**: `586-mcp-server`
**Created**: 2026-05-12 (retroactive — feature shipped in PR #595, merged from `cet/mcp_prototype`)
**Status**: Implemented (spec written after implementation as a speckit reference example)
**Input**: User description: "Add a Model Context Protocol (MCP) server to the running node so AI clients can query Cardano blockchain data using the Blockfrost-compatible API surface that Acropolis already exposes."

> **Note**: This spec is reverse-engineered from GitHub issue #586, PR #595, the
> `modules/mcp_server/` source, and `modules/rest_blockfrost/src/routes.rs`. It
> is preserved as a worked example of the speckit workflow on an existing
> feature; the prose in this document is what the spec *should* have said
> before code was written.

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Query Blockchain via an MCP-Capable AI Client (Priority: P1)

As an Acropolis user with an MCP-capable AI client (Claude Code, VS Code Copilot, MCP Inspector), I want the running node to expose its Blockfrost-compatible API as MCP tools and resources so that I can ask natural-language questions about live chain state without learning the REST API or shelling out to `curl`.

**Why this priority**: This is the whole feature. Without it nothing else has value. Everything below is an enabler or refinement of this single capability.

**Independent Test**: Run the omnibus with `[module.mcp-server] enabled = true`, connect Claude Code via `.vscode/mcp.json`, ask "what is the current epoch?" and observe a correct answer derived from a `get_epoch_information` tool call.

**Acceptance Scenarios**:

1. **Given** the omnibus is running with the MCP server enabled, **When** an MCP client opens an HTTP/SSE session to `/mcp` on the configured port, **Then** the server completes the MCP handshake and reports `resources` and `tools` capabilities.
2. **Given** a connected client, **When** the client issues `tools/list`, **Then** the server returns one tool per registered Blockfrost route, each with a JSON-schema description of its parameters.
3. **Given** a connected client, **When** the client issues `tools/call` with `name = "get_epoch_information"` and `arguments = { "number": "latest" }`, **Then** the server returns the same JSON payload the REST endpoint would have returned for `/epochs/latest`.

---

### User Story 2 - Resource-Style Browsing of Endpoints (Priority: P2)

As an AI client author or human exploring the API, I want each Blockfrost endpoint also exposed as an MCP resource with a stable `blockfrost://` URI template so that clients which prefer resource browsing (rather than tool calling) can list and read endpoints uniformly.

**Why this priority**: Tool calling alone (US1) is enough to deliver value; resources are a usability win for clients with different UX models, but the feature is shippable without them.

**Independent Test**: From a connected client, call `resources/list`, then `resources/read` on `blockfrost://epochs/latest`, and verify the returned JSON matches `tools/call` for `get_epoch_information(number=latest)`.

**Acceptance Scenarios**:

1. **Given** a connected MCP client, **When** the client issues `resources/list`, **Then** the server returns one resource per registered route, each with a URI template, name, description, and `application/json` MIME type.
2. **Given** a known resource URI like `blockfrost://pools/{pool_id}`, **When** the client issues `resources/read` with the template variable substituted, **Then** the server dispatches the request to the matching Blockfrost handler and returns the JSON result.

---

### User Story 3 - Zero-Duplication Maintenance (Priority: P2)

As an Acropolis contributor adding a new Blockfrost endpoint, I want a single place to declare the endpoint so that the REST server and the MCP server both pick it up automatically with no duplicated handler wiring.

**Why this priority**: Drives long-term maintainability. Independently demonstrable by adding a new route and observing both surfaces gain it without any MCP-side code change.

**Independent Test**: Add a new `RouteDefinition` entry to `routes.rs`, rebuild, and verify the new endpoint shows up in both the REST path table and in the MCP `tools/list` and `resources/list` output.

**Acceptance Scenarios**:

1. **Given** a new `RouteDefinition` is appended to `ROUTES`, **When** the omnibus is rebuilt, **Then** the REST module mounts the route and the MCP module advertises a corresponding tool and resource without any other code change.
2. **Given** a route is removed from `ROUTES`, **When** the omnibus is rebuilt, **Then** the route disappears from both surfaces.

---

### User Story 4 - Opt-In Deployment (Priority: P3)

As a node operator, I want the MCP server to be off by default and bind to localhost when enabled, so that adding the module does not expose a new network surface on existing deployments and so that I can opt in deliberately.

**Why this priority**: Operational safety. Important for adoption but does not gate the demo or the MVP.

**Independent Test**: Build the omnibus without changing config, confirm port 4341 is not bound. Then set `enabled = true`, restart, confirm the server listens on `127.0.0.1:4341` only.

**Acceptance Scenarios**:

1. **Given** the default omnibus configuration, **When** the process starts, **Then** the MCP module logs that it is disabled and binds no port.
2. **Given** `[module.mcp-server] enabled = true` with no other overrides, **When** the process starts, **Then** the server binds to `127.0.0.1:4341` and not to a public interface.
3. **Given** `address` and `port` are set in configuration, **When** the process starts, **Then** the server binds to that address and port instead of the defaults.

---

### Edge Cases

- What happens when a client reads a resource URI that does not match any registered route? The server MUST return a structured MCP error (not a panic) identifying the unrecognized URI.
- What happens when a tool is called with missing or wrongly-typed arguments? The server MUST return a structured MCP error referencing the offending parameter rather than crashing or silently substituting defaults.
- What happens when the underlying Blockfrost handler returns an error (e.g., upstream query timeout)? The server MUST surface the error to the MCP client as a tool-call/resource-read error with the underlying message.
- What happens when the configured port is already in use? Startup MUST fail loudly with a clear error; the rest of the node MUST continue to run since the server is spawned in a background task.
- What happens when multiple MCP clients connect concurrently? Each session MUST be isolated; one client's request MUST NOT block or corrupt another's.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The system MUST expose every endpoint registered in the shared routes registry as both an MCP tool and an MCP resource.
- **FR-002**: The system MUST source endpoint metadata (path, URI template, name, description, parameters) from a single shared registry — the REST and MCP surfaces MUST NOT maintain independent copies.
- **FR-003**: The system MUST implement the MCP capabilities `resources` and `tools`, and respond correctly to `resources/list`, `resources/read`, `tools/list`, and `tools/call`.
- **FR-004**: The system MUST reuse the existing Blockfrost request-handling logic to service MCP requests — it MUST NOT reimplement query logic.
- **FR-005**: The system MUST be opt-in via a configuration flag, defaulting to disabled.
- **FR-006**: The system MUST default to binding `127.0.0.1` and a non-privileged port when enabled, and MUST honour operator-provided `address`/`port` overrides.
- **FR-007**: The system MUST return MCP-protocol errors (not panics or unhandled exceptions) for malformed requests, unknown URIs, unknown tool names, and handler failures.
- **FR-008**: The system MUST run as an Acropolis module under the same Caryatid lifecycle as other modules, and MUST run its HTTP listener in a background task so it does not block module initialization.
- **FR-009**: The system MUST advertise itself to MCP clients with a stable server name and the module's package version.
- **FR-010**: The system MUST log the bound address, the resource count, and the tool count at startup, and MUST log each `tools/call` and `resources/read` invocation at info level.
- **FR-011**: The system MUST support multiple concurrent MCP sessions over HTTP without cross-session interference.

### Key Entities

- **RouteDefinition**: The single source of truth for one Blockfrost endpoint. Holds: REST path template, MCP URI template, topic pattern (for internal message routing), human-readable name and description, handler type (path-only vs. path+query), handler function name, and the ordered list of parameter names extracted from the path. Lives in `rest_blockfrost` so the REST module owns the registry and the MCP module consumes it.
- **MCP Tool**: A callable function exposed to AI clients. Derived 1:1 from a `RouteDefinition`. Has a snake_case name (e.g. `get_epoch_information`), a JSON-schema input derived from `param_names` and `handler_type`, and a description copied from the route.
- **MCP Resource**: A readable URI exposed to AI clients. Derived 1:1 from a `RouteDefinition`. Has the `blockfrost://...` URI template, the route's name and description, and an `application/json` MIME type.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: With `enabled = true`, Claude Code can connect using the documented `.vscode/mcp.json` configuration and successfully execute at least one tool call end-to-end on the first attempt.
- **SC-002**: A `tools/list` response contains at least one entry for every Blockfrost route registered in `ROUTES` — the current count is 63 — with no duplicates and no missing entries.
- **SC-003**: A `resources/list` response contains the same set of endpoints as `tools/list`, with stable `blockfrost://`-scheme URI templates.
- **SC-004**: Adding one new `RouteDefinition` to `ROUTES` and rebuilding causes both the REST and MCP surfaces to expose the new endpoint, with no code edits in `modules/mcp_server/`.
- **SC-005**: With `enabled = false` (the default), the node binds no additional port and the MCP module produces a single startup log line indicating it is disabled.
- **SC-006**: A complex multi-call workflow — for example, "list pools, then for the top 5 fetch info and block counts, then summarise efficiency" — completes end-to-end from a connected AI client driving the server.
