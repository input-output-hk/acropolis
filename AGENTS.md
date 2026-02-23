# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Acropolis is a modular Rust implementation of a Cardano node, built using the Caryatid framework. It provides a kit of modular parts for flexible construction of clients, services, and APIs for the Cardano ecosystem. The project aims for parity with the Haskell Cardano node implementation and is intended to be a full block-producing node on mainnet.

## Essential Commands

### Building
- `make build` - Build the omnibus process (equivalent to `cargo build -p acropolis_process_omnibus`)
- `cargo build` - Build entire workspace

### Running
- `make run` - Run the omnibus process (uses `--release` and defaults to `RUST_LOG=info`)
- `make run LOG_LEVEL=debug` - Run with custom log level (options: error, warn, info, debug, trace)
- `make run-bootstrap` - Run with bootstrap configuration (for snapshot-based startup)

### Testing
- `make test` - Run all tests (equivalent to `cargo test`)
- `cargo test <test_name>` - Run specific test
- `cargo test -p <package_name>` - Run tests for specific package

### Code Quality
- `make fmt` - Format code with rustfmt
- `make check` - Check formatting without modifying files
- `make clippy` - Run clippy with `-D warnings` (treats warnings as errors)
- `make all` - Format, lint, and test in sequence

## Architecture

### Core Concepts

Acropolis uses a **publish-subscribe message-passing architecture** based on the Caryatid framework. Modules communicate via messages on topics rather than direct function calls. This provides:
- Module isolation for independent development and testing
- Easy module replacement, addition, or removal
- Traceable communication between modules
- Natural parallelism and horizontal scalability

### Message Bus

By default, modules run in a single process communicating over an in-memory message bus (zero-copy Rust structs). Optionally, modules can run in separate processes communicating via RabbitMQ for external extensions.

The system uses a topic-based publish-subscribe pattern where:
- Publishers send messages on topics (e.g., `cardano.block.raw`)
- Subscribers listen to topics they're interested in
- Caryatid handles routing, serialization, and request-response patterns

### Module Structure

Modules are located in `modules/` and follow this pattern:

```rust
#[module(
    message_type(Message),
    name = "module-name",
    description = "One-liner description"
)]
pub struct MyModule;

impl MyModule {
    pub fn init(&self, context: Arc<Context<Message>>, config: Arc<Config>) -> Result<()> {
        // Subscribe to topics via context.message_bus
        // Capture state in subscription closures
        Ok(())
    }
}
```

Key modules include:
- **Bootstrapping**: Genesis Bootstrapper, Mithril Snapshot Fetcher, Snapshot Bootstrapper
- **Network**: Peer Network Interface (N2N protocol client)
- **Unpacking**: Block Unpacker, Tx Unpacker
- **State Management**: UTXO State, SPO State, DRep State, Accounts State, Assets State, Governance State, Epochs State
- **Validation**: Block VRF Validator, Block KES Validator, Consensus
- **Storage**: Chain Store
- **Interfaces**: REST Blockfrost API, MCP Server, TX Submitter

### Processes

Processes (in `processes/`) are executable binaries that bundle modules together:
- **Omnibus** (`processes/omnibus`): All-inclusive testing process containing all modules
- **Replayer**: Replay previously downloaded messages from JSON files
- **Golden Tests**: End-to-end golden test execution
- **TX Submitter CLI**: Command-line wrapper for transaction submission

### Configuration

Configuration files use TOML format. The omnibus process uses:
- `omnibus.toml` - Main configuration (startup mode, network, module settings)
- `omnibus.bootstrap.toml` - Snapshot bootstrap overrides
- `omnibus-local.toml`, `omnibus-sancho.toml`, `omnibus-rewards.toml` - Network-specific configs

Key configuration sections:
- `[global.startup]` - Startup mode (genesis/snapshot), sync mode (mithril/upstream), network name
- `[module.<module-name>]` - Per-module configuration

### Common Patterns

**Messages**: Defined in `common/src/messages.rs`. All messages flow through the `Message` enum which includes raw blocks, transactions, UTXO deltas, certificates, queries/responses, etc.

**Adding a New Module**:
1. Create module in `modules/<module_name>/`
2. Implement module struct with `#[module(...)]` macro
3. Add to workspace in root `Cargo.toml`
4. Register in process (e.g., omnibus) via `MyModule::register(&mut process)`
5. Add configuration section to process config file

## Cardano-Specific Context

- Uses **Pallas** library (`pallas = "0.34.0"`) for Cardano primitives and CBOR parsing
- Supports multiple eras (Shelley through Conway)
- Can bootstrap from Genesis files or ledger state snapshots (CBOR format)
- Integrates with Mithril for snapshot verification and fetching
- Implements Ouroboros Praos consensus protocol components
- Supports Cardano native assets, governance (DReps, proposals), and stake pools

## Development Workflow

1. **Starting from Genesis**: Set `startup-mode = "genesis"` in omnibus.toml, run `make run`
2. **Starting from Snapshot**: Use `make run-bootstrap` with configured snapshot epoch
3. **Integration Tests**: See `tests/integration/` for SPDD validation and other integration tests
4. **Adding Features**: Create or modify modules, publish/subscribe to relevant message topics

## Important Notes

- The omnibus process is for **testing only**, not production deployment
- All inter-module dependencies are resolved at configuration/runtime, never at compile time
- State is typically maintained in closures captured by subscription handlers
- When modifying message types, update `common/src/messages.rs` and regenerate if needed
- The project uses `cargo workspace` - changes to shared crates affect all modules
