---
description: "Implementation task list for the MCP server feature"
---

# Tasks: MCP Server for AI Queries

**Input**: Design documents from `/specs/586-mcp-server/`
**Prerequisites**: spec.md, plan.md, research.md, data-model.md, contracts/mcp-protocol.md

**Tests**: Manual / script-driven integration tests only. No automated Rust test suite was written in PR #595; that is a polish task. Tasks listed under "(OPTIONAL)" reflect what the feature *should* have if it were re-done.

**Organization**: Tasks are grouped by the user stories in `spec.md` so each story can be implemented and demoed independently.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies)
- **[Story]**: User story this task belongs to (US1–US4)
- File paths are relative to the repository root.

---

## Phase 1: Setup (Shared Infrastructure)

- [x] T001 Add `modules/mcp_server/` as a new workspace member in the root `Cargo.toml`.
- [x] T002 Create `modules/mcp_server/Cargo.toml` with dependencies on `acropolis_common`, `acropolis_module_rest_blockfrost`, `caryatid_sdk`, `axum`, `tokio`, `rmcp = "0.8"` (features: `server`, `transport-streamable-http-server`), `tracing`, `serde_json`, `anyhow`, `config`.
- [x] T003 Stub the module: `modules/mcp_server/src/mcp_server.rs` with the `#[module(...)]` macro, a `MCPServer` struct, and an `init` that reads `enabled/address/port` from config but does nothing else yet.

**Checkpoint**: `cargo build -p acropolis_module_mcp_server` succeeds; the omnibus is not yet aware of the module.

---

## Phase 2: Foundational (Blocking Prerequisites)

These tasks introduce the shared registry that every user story depends on.

- [x] T004 [P] Add `modules/rest_blockfrost/src/routes.rs` with `pub enum HandlerType`, `pub struct RouteDefinition`, and `pub const ROUTES: &[RouteDefinition]` (initially empty).
- [x] T005 [P] Re-export `routes` from `modules/rest_blockfrost/src/lib.rs` so the MCP crate can consume `ROUTES` and `RouteDefinition`.
- [x] T006 Migrate every existing REST endpoint into a `RouteDefinition` entry in `ROUTES`. Group entries with `// ==================== <section> ====================` separators. End state: 63 entries covering accounts, blocks, governance (DReps + proposals), pools, epochs, assets, addresses, transactions.
- [x] T007 Refactor the existing REST router to mount handlers from `ROUTES` instead of duplicated wiring.
- [x] T008 Register `MCPServer` in `processes/omnibus/src/main.rs` via the standard `::register(&mut process)` call.
- [x] T009 Add `[module.mcp-server]` blocks to `processes/omnibus/omnibus.toml` and `processes/omnibus/omnibus-local.toml` with `enabled = true` for local development and the defaults documented in `quickstart.md`.

**Checkpoint**: `make build` succeeds; the omnibus starts with the MCP module loaded but the server still does nothing.

---

## Phase 3: User Story 1 — Query Blockchain via an MCP-Capable AI Client (Priority: P1) 🎯 MVP

**Goal**: Get `tools/list` and `tools/call` working end-to-end so an AI client can query the chain.

**Independent Test**: `quickstart.md` step 3 (MCP Inspector) calls `tools/call get_epoch_information { number: "latest" }` and gets the expected payload.

### Implementation for User Story 1

- [x] T010 [US1] Implement `modules/mcp_server/src/server.rs::AcropolisMCPServer` holding `Arc<Context<Message>>` and `Arc<Config>`, plus a `run(address, port)` method that builds the rmcp `StreamableHttpService`, mounts it on an `axum::Router` at `/mcp`, and serves it with `axum::serve`.
- [x] T011 [US1] Implement `ServerHandler::get_info` returning the server identity laid out in `data-model.md` and `contracts/mcp-protocol.md`.
- [x] T012 [P] [US1] Implement `modules/mcp_server/src/tools.rs::get_all_tools()` — iterate `ROUTES`, derive the tool name (snake-case + `get_` prefix), build the JSON-schema for `param_names` and the optional `query` field for `WithQuery` handlers.
- [x] T013 [P] [US1] Implement `tools::list_tools_result()` wrapping the tool list in an MCP `ListToolsResult`.
- [x] T014 [US1] Implement `tools::handle_tool_call(context, config, name, arguments)` — look up the route by tool name, validate arguments, dispatch into the existing `rest_blockfrost` handler, wrap the JSON result.
- [x] T015 [US1] Wire `ServerHandler::list_tools` and `ServerHandler::call_tool` in `server.rs` to delegate to `tools.rs`.
- [x] T016 [US1] Update `mcp_server.rs::init` to spawn `AcropolisMCPServer::run` in `tokio::spawn` when `enabled = true`; log startup lines per the logging contract.

**Checkpoint**: With `make run` and `enabled = true`, MCP Inspector lists 63 tools and a `get_epoch_information` call returns live data.

---

## Phase 4: User Story 2 — Resource-Style Browsing (Priority: P2)

**Goal**: Expose every route as an MCP resource alongside the tool form.

**Independent Test**: `resources/list` and `resources/read blockfrost://epochs/latest` both succeed against the running server.

### Implementation for User Story 2

- [x] T017 [P] [US2] Implement `modules/mcp_server/src/resources.rs::get_all_resources()` returning the same `ROUTES` view typed for resource listing (URI template, name, description).
- [x] T018 [P] [US2] Implement `resources::handle_resource(context, config, uri)` — match `uri` against every `mcp_uri_template`, extract `param_names`, dispatch through the same handler path tools use.
- [x] T019 [US2] Wire `ServerHandler::list_resources` and `ServerHandler::read_resource` in `server.rs` to delegate to `resources.rs`, wrapping responses in `TextResourceContents` with `application/json`.

**Checkpoint**: `tools/list` and `resources/list` describe the same set of endpoints.

---

## Phase 5: User Story 3 — Zero-Duplication Maintenance (Priority: P2)

**Goal**: Prove that adding/removing a `RouteDefinition` automatically propagates to both surfaces.

**Independent Test**: Append a new `RouteDefinition` to `ROUTES`, rebuild, and verify it appears in both `tools/list` (new tool name) and `resources/list` (new URI template) without any other code change.

### Implementation for User Story 3

- [x] T020 [US3] Audit the REST router from T007 to confirm it has no parallel hand-rolled route table; everything must go through `ROUTES`.
- [x] T021 [US3] Document the contract for adding a new endpoint in `modules/mcp_server/README.md` (one paragraph + a pointer to `routes.rs`).
- [ ] T022 [US3] (OPTIONAL) Add a compile-time test or `build.rs` assertion that every `mcp_uri_template` placeholder appears in `param_names`. Lesson [[L014]] argues for keeping checklists and reality in sync — this is the programmatic version.

---

## Phase 6: User Story 4 — Opt-In Deployment (Priority: P3)

**Goal**: Make the server safe to enable on existing deployments and document the operator path.

**Independent Test**: Two boots — defaults bind no port; `enabled = true` binds `127.0.0.1:4341`. Operator overrides `address`/`port` and observes the new bind.

### Implementation for User Story 4

- [x] T023 [US4] In `mcp_server.rs::init`, return early after logging `MCP server is disabled in configuration` when `enabled = false` (the default).
- [x] T024 [US4] Honour `address` and `port` from `[module.mcp-server]` with defaults `127.0.0.1` and `4341`. Use `acropolis_common::configuration::get_*` helpers.
- [x] T025 [US4] Add `.vscode/mcp.json` pointing Claude Code / Copilot at `http://localhost:4341/mcp`.
- [x] T026 [US4] Write `modules/mcp_server/README.md` covering: overview, configuration block, VS Code / Claude Desktop usage, available tools, and the test-script entry points.

**Checkpoint**: A fresh checkout with default config builds and runs without binding port 4341; flipping `enabled = true` cleanly starts the server.

---

## Phase 7: Polish & Cross-Cutting Concerns

These are the gaps PR #595 itself flagged. Track them as follow-ups.

- [ ] T027 [P] Add a Rust integration test that boots the omnibus, performs `tools/list` / `tools/call` over HTTP, and asserts the response matches the registry.
- [ ] T028 [P] Add a unit test verifying every `RouteDefinition` derives a unique tool name and a non-empty input schema (defends against name collisions when new endpoints are added).
- [ ] T029 Decide on production posture (auth, rate limit, TLS) and either implement or document the operator-side path. See [[research]] R-009.
- [ ] T030 Demote per-request `tracing::info!` logs to `debug!` once the surface is stable (see R-008).
- [ ] T031 Implement `processes/mcp_standalone/` as a real stdio-transport binary for Claude Desktop users (currently a Cargo stub).
- [ ] T032 (OPTIONAL) Run the spec through `/speckit.analyze` once tasks are merged to validate cross-artifact consistency.

---

## Dependencies & Execution Order

### Phase dependencies

- **Setup (Phase 1)**: No dependencies.
- **Foundational (Phase 2)**: Depends on Setup. Blocks every user story because `ROUTES` is the data backbone.
- **User Stories**:
  - **US1 (P1)**: Depends on Phase 2 only. The MVP.
  - **US2 (P2)**: Depends on Phase 2; integrates cleanly with US1.
  - **US3 (P2)**: Depends on Phase 2; verifies the design intent rather than adding new surface area.
  - **US4 (P3)**: Depends on US1 being functional so the gating logic has something to gate.
- **Polish (Phase 7)**: Depends on the user stories that are in scope being complete.

### Parallel opportunities

- T004 and T005 are independent of each other (different files) and can run in parallel.
- T012, T013, T017, T018 are independent and can be implemented in parallel by different developers.
- The smoke-test scripts can be drafted in parallel with code as long as the MCP endpoint contract from `contracts/mcp-protocol.md` is stable.

---

## Notes

- Every `[x]` task above is checked because the work landed in PR #595 (`cet/mcp_prototype`). The few unchecked items are the polish items the PR itself called out.
- This tasks list is illustrative: re-running `/speckit.tasks` against today's `spec.md` and `plan.md` would regenerate something structurally similar but the IDs would differ.
- Lessons [[L013]] (don't ship unfilled templates) and [[L014]] (checklists must match reality) shaped how this tasks file is written: every task entry is concrete, references a real file path, and reflects what shipped.
