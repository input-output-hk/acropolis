# Getting Started with Acropolis

This guide covers everything you need to build, configure, and run the Acropolis Cardano node.

## Prerequisites

### Rust Toolchain

Acropolis requires **Rust 1.93** with `clippy` and `rustfmt` components. The project includes a `rust-toolchain.toml` that will automatically select the correct version when you build.

Install Rust via [rustup](https://rustup.rs/):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

The correct toolchain and components will be installed automatically on first build.

### System Dependencies

| Dependency | Platform | Notes |
|-----------|----------|-------|
| C compiler (gcc or clang) | All | Required for native dependencies |
| pkg-config | Linux | For locating system libraries |
| OpenSSL (libssl-dev) | Linux | TLS support |
| libiconv | macOS | Character encoding (usually pre-installed via Xcode CLI tools) |

**macOS** — install Xcode Command Line Tools if you haven't already:

```sh
xcode-select --install
```

**Ubuntu / Debian**:

```sh
sudo apt-get update
sudo apt-get install build-essential pkg-config libssl-dev
```

### Docker (optional)

[Docker](https://docs.docker.com/get-docker/) and Docker Compose are needed only if you want to run via containers instead of building from source.

---

## Quick Start

### Option 1: Build and Run from Source

```sh
# Clone the repository
git clone https://github.com/input-output-hk/acropolis.git
cd acropolis

# Build the omnibus process
make build

# Run the node (mainnet, genesis sync via Mithril)
make run
```

The node will start syncing the Cardano mainnet from genesis, fetching blocks via [Mithril](https://mithril.network/) snapshots.

### Option 2: Docker Compose

```sh
# Build and run the omnibus process on mainnet
docker compose up omnibus-mainnet

# Or on the preview testnet
docker compose up omnibus-preview
```

See [Docker Compose](#docker-compose) below for all available services and configuration.

---

## Build Commands

All build commands are available via `make`. Run `make help` for a full list.

| Command | Description |
|---------|-------------|
| `make build` | Build the omnibus process (debug) |
| `make build-release` | Build the omnibus process (release, optimised) |
| `make test` | Run all tests |
| `make fmt` | Format code with rustfmt |
| `make check` | Check formatting without modifying files |
| `make clippy` | Run clippy with `-D warnings` (treats warnings as errors) |
| `make all` | Format, lint, and test in sequence |

To build the entire workspace (all modules and processes):

```sh
cargo build
```

To run tests for a specific package:

```sh
cargo test -p acropolis_module_utxo_state
```

---

## Running the Node

The primary way to run Acropolis is via the **omnibus** process, which bundles all modules into a single binary communicating over an in-memory message bus. The omnibus process is intended for testing and development.

### Run Modes

| Command | Network | Startup Mode | Description |
|---------|---------|-------------|-------------|
| `make run` | Mainnet | Genesis | Sync from genesis via Mithril |
| `make run-preview` | Preview | Genesis | Sync the preview testnet |
| `make run-bootstrap` | Mainnet | Snapshot | Bootstrap from a ledger state snapshot |
| `make run-bootstrap-preview` | Preview | Snapshot | Bootstrap preview from snapshot |

All run commands accept a `LOG_LEVEL` variable:

```sh
make run LOG_LEVEL=debug    # Options: error, warn, info, debug, trace
```

### Genesis Mode vs Snapshot Bootstrap

**Genesis mode** (default) starts from the genesis block and syncs the full chain via Mithril. This is the simplest way to get started but takes longer to reach the chain tip.

**Snapshot bootstrap** starts from a pre-built ledger state snapshot, skipping the need to replay the full history. This is significantly faster. See [docs/bootstrap.md](bootstrap.md) for details on how the bootstrap process works.

---

## Configuration

Acropolis uses TOML configuration files. The omnibus process supports layered configuration — multiple `--config` files are merged in order, with later files overriding earlier ones.

For a complete reference of all settings, see the [Configuration Reference](configuration.md).

### Config Files

The main configuration files live in `processes/omnibus/`:

| File | Purpose |
|------|---------|
| `omnibus.toml` | Base mainnet configuration |
| `omnibus-preview.toml` | Preview testnet overrides |
| `omnibus.bootstrap.toml` | Snapshot bootstrap overrides (mainnet) |
| `omnibus.bootstrap.preview.toml` | Snapshot bootstrap overrides (preview) |
| `omnibus.store-spdd-drdd.toml` | Enable SPDD/DRDD storage |

### Key Settings

The `[global.startup]` section controls how the node starts:

```toml
[global.startup]
network-name = "mainnet"       # "mainnet" | "preview"
startup-mode = "genesis"       # "genesis" | "snapshot"
sync-mode = "mithril"          # "mithril" | "upstream"
block-flow-mode = "direct"     # "direct" | "consensus"
```

Each module has its own `[module.<name>]` section. For example, the UTXO state module:

```toml
[module.utxo-state]
store = "memory"    # "memory", "dashmap", "fjall", "fjall-async"
```

Many modules have feature flags that enable specific API endpoints. These are disabled by default to minimise resource usage. See the [Configuration Reference](configuration.md) for all available settings.

### Peer Configuration

The node connects to upstream Cardano peers for chain synchronisation:

```toml
[module.peer-network-interface]
sync-point = "dynamic"
node-addresses = [
    "backbone.cardano.iog.io:3001",
    "backbone.mainnet.cardanofoundation.org:3001",
    "backbone.mainnet.emurgornd.com:3001",
]
```

---

## Docker Compose

The `docker-compose.yml` provides pre-configured services for various deployment scenarios.

### Available Services

| Service | Ports | Description |
|---------|-------|-------------|
| `omnibus-preview` | 4340 (REST), 4341 (MCP) | Preview testnet, genesis sync |
| `omnibus-mainnet` | 5340 (REST), 5341 (MCP) | Mainnet, genesis sync |
| `omnibus-bootstrap-preview` | 6340 (REST), 6341 (MCP) | Preview, snapshot bootstrap |
| `omnibus-bootstrap-mainnet` | 7340 (REST), 7341 (MCP) | Mainnet, snapshot bootstrap |
| `midnight-indexer-preview` | 50051 (gRPC) | Midnight indexer, preview |
| `midnight-indexer-mainnet` | 60051 (gRPC) | Midnight indexer, mainnet |
| `midnight-indexer-guardnet` | 65051 (gRPC) | Midnight indexer, guardnet |

### Running a Service

```sh
# Start a specific service
docker compose up omnibus-preview

# Start in detached mode
docker compose up -d omnibus-mainnet

# View logs
docker compose logs -f omnibus-mainnet
```

### Environment Variables

Ports and paths can be customised via environment variables:

```sh
OMNIBUS_PREVIEW_REST_PORT=8340 docker compose up omnibus-preview
```

All services set `ulimits` for file descriptors (default 4096) to handle Fjall LSM storage requirements.

---

## REST API

The omnibus process exposes a Blockfrost-compatible REST API. By default it listens on port **4340**.

Once the node is running, you can query it:

```sh
# Check node tip
curl http://localhost:4340/blocks/latest

# Query a specific epoch
curl http://localhost:4340/epochs/latest
```

The full OpenAPI specification is available in [API/openapi.yaml](../API/openapi.yaml). To browse it interactively:

```sh
cd API && ./run-local-swagger.sh
# Opens Swagger UI at http://localhost:28080
```

---

## Troubleshooting

### Too Many Open Files (Fjall)

Modules using [Fjall](https://github.com/fjall-rs/fjall) LSM-tree storage can exceed the default file descriptor limit on some systems. Symptoms include "too many open files" errors.

**Fix**: Increase the file descriptor limit before running:

```sh
ulimit -n 4096
```

The Docker Compose configuration already sets this limit automatically.

### macOS Build Issues

If you encounter linker errors on macOS, ensure Xcode Command Line Tools are installed:

```sh
xcode-select --install
```

### Slow Initial Sync

Genesis mode replays the entire chain history, which takes time. For faster startup, use snapshot bootstrap mode:

```sh
make run-bootstrap
```

See [docs/bootstrap.md](bootstrap.md) for details.

---

## Further Reading

- [Configuration Reference](configuration.md) — all module settings, defaults, and API feature flags
- [Project Overview & Deliverables](overview.md) — development phases and node configurations
- [Bootstrap Guide](bootstrap.md) — snapshot bootstrap architecture and quick start
- [Architecture: Modularity](architecture/modularity.md) — pub-sub design and module communication
- [Memory Profiling](memory-profiling.md) — heap profiling with jemalloc
- [Performance Profiling](performance-profiling.md) — CPU profiling with Linux perf
- [Modules Reference](../modules/README.md) — list of all modules with descriptions
- [Processes Reference](../processes/README.md) — available process binaries
