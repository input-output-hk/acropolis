# Configuration Reference

This document describes all configuration settings available in Acropolis TOML configuration files.

Acropolis uses layered TOML configuration. Multiple `--config` files can be specified and are merged in order, with later files overriding earlier ones. For example:

```sh
cargo run --release --bin acropolis_process_omnibus -- --config omnibus.toml --config omnibus.bootstrap.toml
```

Configuration files live in the process directory (e.g. `processes/omnibus/`).

---

## Global Startup

The `[global.startup]` section controls how the node starts and which network it connects to.

```toml
[global.startup]
network-name = "mainnet"
startup-mode = "genesis"
sync-mode = "mithril"
block-flow-mode = "direct"
topic = "cardano.sequence.start"
```

| Setting | Type | Default | Options | Description |
|---------|------|---------|---------|-------------|
| `network-name` | string | `"mainnet"` | `"mainnet"`, `"preview"`, `"sanchonet"` | Cardano network to connect to |
| `startup-mode` | string | `"genesis"` | `"genesis"`, `"snapshot"` | Start from genesis block or a ledger state snapshot |
| `sync-mode` | string | `"mithril"` | `"mithril"`, `"upstream"` | Fetch blocks via Mithril snapshots or directly from upstream peers |
| `block-flow-mode` | string | `"direct"` | `"direct"`, `"consensus"` | Block delivery mode — direct pass-through or via consensus module |
| `topic` | string | `"cardano.sequence.start"` | — | Message bus topic for startup sequencing |

---

## Bootstrap Modules

### `[module.genesis-bootstrapper]`

Reads the genesis file for a chain and generates initial UTXOs. No required user-facing settings — network selection is inherited from `[global.startup]`.

### `[module.mithril-snapshot-fetcher]`

Fetches chain snapshots from the Mithril aggregator and replays blocks.

```toml
[module.mithril-snapshot-fetcher]
aggregator-url = "https://aggregator.release-mainnet.api.mithril.network/aggregator"
genesis-key = "5b3139312c..."
download-max-age = "never"
pause = "none"
stop = "none"
```

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `aggregator-url` | string | Mainnet aggregator URL | Mithril aggregator endpoint |
| `genesis-key` | string | Mainnet genesis key | Mithril genesis verification key |
| `download-max-age` | string | `"never"` | Maximum age of cached download before re-fetching. E.g. `"8"` for 8 hours, or `"never"` to always use cache |
| `directory` | string | `"../../modules/mithril_snapshot_fetcher/downloads/<network>"` | Download directory for snapshots |
| `pause` | string | `"none"` | Pause syncing at a point. E.g. `"epoch:100"`, `"block:1200"`, `"every-epoch:10"`, `"every-block:500"` |
| `stop` | string | `"none"` | Stop syncing at a point (same format as `pause`) |
| `profile` | string | `"none"` | Trigger profiling at a point (same format as `pause`) |

### `[module.snapshot-bootstrapper]`

Downloads and parses ledger state snapshots for fast bootstrap.

```toml
[module.snapshot-bootstrapper]
epoch = 507
data-dir = "../../modules/snapshot_bootstrapper/data"
```

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `epoch` | integer | — | Epoch number of the snapshot to bootstrap from |
| `data-dir` | string | `"../../modules/snapshot_bootstrapper/data"` | Directory containing snapshot data files |

---

## Network & Consensus

### `[module.peer-network-interface]`

Node-to-Node (N2N) client protocol for chain synchronisation and block fetching.

```toml
[module.peer-network-interface]
sync-point = "dynamic"
node-addresses = [
    "backbone.cardano.iog.io:3001",
    "backbone.mainnet.cardanofoundation.org:3001",
    "backbone.mainnet.emurgornd.com:3001",
]
```

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `node-addresses` | array of strings | — | **Required.** List of upstream peer addresses (`host:port`) |
| `sync-point` | string | — | Sync start point. `"origin"` for genesis+upstream, `"dynamic"` for snapshot or Mithril modes, `"tip"` for chain tip, `"cache"` for cached position |
| `magic-number` | integer | — | Network magic number. Set for non-mainnet networks (e.g. `2` for preview) |
| `cache-dir` | path | — | Directory for caching chain sync state |
| `target-peer-count` | integer | `15` | Target number of peers to maintain |
| `min-hot-peers` | integer | `3` | Minimum number of active (hot) peers |
| `peer-sharing-enabled` | bool | `true` | Enable peer sharing (P2P peer discovery) |
| `churn-interval-secs` | integer | `600` | Seconds between peer churn cycles |
| `peer-sharing-timeout-secs` | integer | `10` | Timeout for peer sharing requests |
| `connect-timeout-secs` | integer | `15` | Timeout for TCP connection attempts |
| `ipv6-enabled` | bool | `false` | Allow IPv6 peer connections |
| `allow-non-public-peer-addrs` | bool | `true` | Allow connections to private/non-routable addresses |
| `discovery-interval-secs` | integer | `60` | Seconds between peer discovery rounds |
| `peer-sharing-cooldown-secs` | integer | `30` | Cooldown between peer sharing requests to same peer |

### `[module.consensus]`

Ouroboros Praos consensus — block ordering, validation coordination, and fork selection.

```toml
[module.consensus]
consensus-offers-topic = "cardano.consensus.offers"
consensus-wants-topic = "cardano.consensus.wants"
validators = []
force-validation = false
validation-timeout = 60
```

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `force-validation` | bool | `true` | Validate all blocks including those from Mithril snapshots. Set to `false` to skip validation for snapshot blocks |
| `validation-timeout` | integer | `60` | Seconds to wait for validation results before timing out |
| `validators` | array of strings | `[]` | List of validation result topics to listen on (e.g. `"cardano.validation.vrf"`, `"cardano.validation.kes"`) |
| `consensus-offers-topic` | string | `"cardano.consensus.offers"` | Topic for consensus block offers |
| `consensus-wants-topic` | string | `"cardano.consensus.wants"` | Topic for consensus block wants |

---

## Block & Transaction Processing

### `[module.block-unpacker]`

Unpacks received blocks into individual transactions. No required user-facing settings.

### `[module.tx-unpacker]`

Parses transactions and generates UTXO changes, asset deltas, certificates, and governance actions.

```toml
[module.tx-unpacker]
bootstrapped-subscribe-topic = "cardano.sequence.bootstrapped"
protocol-parameters-subscribe-topic = "cardano.protocol.parameters"
```

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `publish-utxo-deltas-topic` | string | `"cardano.utxo.deltas"` | Topic for UTXO delta messages |
| `publish-asset-deltas-topic` | string | `"cardano.asset.deltas"` | Topic for asset delta messages |
| `publish-withdrawals-topic` | string | `"cardano.withdrawals"` | Topic for withdrawal messages |
| `publish-certificates-topic` | string | `"cardano.certificates"` | Topic for certificate messages |
| `publish-governance-topic` | string | `"cardano.governance"` | Topic for governance action messages |
| `publish-tx-validation-topic` | string | `"cardano.validation.tx"` | Topic for transaction validation results |

### `[module.block-vrf-validator]`

Validates block VRF proofs. No required user-facing settings.

### `[module.block-kes-validator]`

Validates block KES signatures. No required user-facing settings.

---

## Ledger State Modules

### `[module.utxo-state]`

Maintains the UTXO set. Supports multiple storage backends.

```toml
[module.utxo-state]
store = "memory"
```

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `store` | string | `"memory"` | Storage backend: `"memory"`, `"dashmap"`, `"fjall"`, `"fjall-async"`, `"sled"`, `"sled-async"`, `"fake"` |
| `database-path` | string | `"fjall-immutable-utxos"` | Path to Fjall/Sled database (only used with persistent backends) |
| `flush-every` | integer | `1000` | Flush to disk every N blocks (Fjall backend only) |
| `address-delta-topic` | string | `"cardano.address.deltas"` | Topic for address delta messages |
| `block-totals-topic` | string | `"cardano.block.txs"` | Topic for block totals messages |
| `address-delta-publish-mode` | string | `"compact"` | Address delta publishing mode |

### `[module.spo-state]`

Tracks stake pool operator registrations, retirements, delegators, and blocks.

```toml
[module.spo-state]
store-epochs-history = false
store-retired-pools = false
store-registration = false
store-updates = false
store-delegators = false
store-votes = false
store-blocks = false
store-stake-addresses = false
```

| Setting | Type | Default | API Endpoints Enabled |
|---------|------|---------|----------------------|
| `store-epochs-history` | bool | `false` | `/pools/{pool_id}/history`, active stakes queries |
| `store-retired-pools` | bool | `false` | `/pools/retired` |
| `store-registration` | bool | `false` | `/pools/{pool_id}` |
| `store-updates` | bool | `false` | `/pools/{pool_id}/updates` |
| `store-delegators` | bool | `false` | `/pools/{pool_id}/delegators` (requires `store-stake-addresses`) |
| `store-votes` | bool | `false` | `/pools/{pool_id}/votes` |
| `store-blocks` | bool | `false` | `/pools/{pool_id}/blocks`, `/epochs/{number}/blocks/{pool_id}` |
| `store-stake-addresses` | bool | `false` | Internal — required by `store-delegators` |

### `[module.drep-state]`

Tracks DRep (Delegated Representative) registrations and activity.

```toml
[module.drep-state]
store-info = false
store-delegators = false
store-metadata = false
store-updates = false
store-votes = false
```

| Setting | Type | Default | API Endpoints Enabled |
|---------|------|---------|----------------------|
| `store-info` | bool | `false` | `/governance/dreps/{drep_id}` (requires `store-delegators`) |
| `store-delegators` | bool | `false` | `/governance/dreps/{drep_id}/delegators` |
| `store-metadata` | bool | `false` | `/governance/dreps/{drep_id}/metadata` |
| `store-updates` | bool | `false` | `/governance/dreps/{drep_id}/updates` |
| `store-votes` | bool | `false` | `/governance/dreps/{drep_id}/votes` |

### `[module.governance-state]`

Tracks governance actions and voting.

```toml
[module.governance-state]
stake-drep-distribution-topic = "cardano.drep.distribution"
stake-spo-distribution-topic = "cardano.spo.distribution"
```

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `stake-drep-distribution-topic` | string | `"cardano.drep.distribution"` | Topic for DRep distribution data |
| `stake-spo-distribution-topic` | string | `"cardano.spo.distribution"` | Topic for SPO distribution data |
| `verification-output-file` | string | — | Path to write verification output CSV |
| `verify-votes-files` | string | — | Glob pattern for vote verification CSV files |

### `[module.parameters-state]`

Tracks protocol parameters and their changes across epochs.

```toml
[module.parameters-state]
enact-state-topic = "cardano.enact.state"
store-history = false
```

| Setting | Type | Default | API Endpoints Enabled |
|---------|------|---------|----------------------|
| `enact-state-topic` | string | `"cardano.enact.state"` | Topic for governance enact state |
| `store-history` | bool | `false` | `/epochs/{number}/parameters` |

### `[module.accounts-state]`

Tracks stake accounts, reward distribution, and stake pool delegation distributions (SPDD).

```toml
[module.accounts-state]
drep-state-topic = "cardano.drep.state"
spdd-retention-epochs = 0
spdd-db-path = "./fjall-spdd"
spdd-clear-on-start = true
```

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `drep-state-topic` | string | `"cardano.drep.state"` | Topic for DRep state (when governance is active) |
| `spdd-retention-epochs` | integer | `0` | Number of SPDD epochs to retain. Enables `/epochs/{number}/stakes` endpoints |
| `spdd-db-path` | string | `"./fjall-spdd"` | Path to SPDD Fjall database |
| `spdd-clear-on-start` | bool | `true` | Clear SPDD database on startup |
| `verify-pots-file` | string | — | Path to CSV file for pot verification |
| `verify-rewards-files` | string | — | Glob pattern for reward verification CSV files |
| `verify-spdd-files` | string | — | Glob pattern for SPDD verification CSV files |

### `[module.epochs-state]`

Tracks fees, blocks minted, and epoch history. No required user-facing settings.

### `[module.assets-state]`

Tracks native asset supply, metadata, transactions, and addresses.

```toml
[module.assets-state]
store-assets = false
store-info = false
store-history = false
store-transactions = "none"
store-addresses = false
index-by-policy = false
```

| Setting | Type | Default | API Endpoints Enabled |
|---------|------|---------|----------------------|
| `store-assets` | bool | `false` | `/assets` |
| `store-info` | bool | `false` | `/assets/{asset}` (requires `store-assets`) |
| `store-history` | bool | `false` | `/assets/{asset}/history` |
| `store-transactions` | string | `"none"` | `/assets/{asset}/transactions`. Options: `"none"`, `"all"`, or a number like `"20"` for most recent |
| `store-addresses` | bool | `false` | `/assets/{asset}/addresses` |
| `index-by-policy` | bool | `false` | Enables policy-based asset lookup (requires `store-assets`) |

---

## Distribution Snapshots

### `[module.spdd-state]`

Stake Pool Delegation Distribution snapshots.

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `store-spdd` | bool | `false` | Enable SPDD storage. Enables `active_stakes` in `/epochs/latest` and `/epochs/{number}` |

### `[module.drdd-state]`

DRep Delegation Distribution snapshots.

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `store-drdd` | bool | `false` | Enable DRDD storage |

### `[module.stake-delta-filter]`

Filters stake address changes and resolves stake pointers.

```toml
[module.stake-delta-filter]
cache-mode = "predefined"
write-full-cache = "false"
```

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `cache-mode` | string | `"predefined"` | Cache mode: `"predefined"` (use built-in cache), `"read"` (read from disk), `"write"` (generate cache), `"write-if-absent"` (write only if no cache exists) |
| `write-full-cache` | bool | `false` | Write full cache data (all entries, not just deltas) |
| `cache-dir` | string | `"cache"` | Directory for stake delta cache files |
| `network` | string | `"Mainnet"` | Network identifier for cache scoping: `"Mainnet"` or `"Testnet"` |

---

## Persistent State Modules

These modules use Fjall LSM storage for historical data that persists across restarts.

### `[module.chain-store]`

Persistent block and transaction storage.

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `store` | string | `"fjall"` | Storage backend (currently only `"fjall"`) |
| `database-path` | string | `"fjall-blocks-<network>"` | Path to the Fjall database directory |
| `clear-on-start` | bool | `true` | Wipe database on startup |

### `[module.address-state]`

Address-level transaction and balance tracking.

```toml
[module.address-state]
clear-on-start = true
store-info = false
store-totals = false
store-transactions = false
```

| Setting | Type | Default | API Endpoints Enabled |
|---------|------|---------|----------------------|
| `db-path` | string | `"./fjall-addresses"` | Path to Fjall database |
| `clear-on-start` | bool | `true` | Wipe database on startup |
| `store-info` | bool | `false` | `/addresses/{address}`, `/addresses/{address}/extended`, `/addresses/{address}/utxos`, `/addresses/{address}/utxos/{asset}` |
| `store-totals` | bool | `false` | `/addresses/{address}/totals` |
| `store-transactions` | bool | `false` | `/addresses/{address}/transactions` |

### `[module.historical-accounts-state]`

Historical account data (rewards, delegations, registrations).

```toml
[module.historical-accounts-state]
clear-on-start = true
store-rewards-history = false
store-active-stake-history = false
store-registration-history = false
store-delegation-history = false
store-mir-history = false
store-withdrawal-history = false
store-addresses = false
store-tx-count = false
```

| Setting | Type | Default | API Endpoints Enabled |
|---------|------|---------|----------------------|
| `db-path` | string | `"./fjall-accounts"` | Path to Fjall database |
| `clear-on-start` | bool | `true` | Wipe database on startup |
| `store-rewards-history` | bool | `false` | `/accounts/{stake_address}/rewards` |
| `store-active-stake-history` | bool | `false` | `/accounts/{stake_address}/history` |
| `store-registration-history` | bool | `false` | `/accounts/{stake_address}/registrations` |
| `store-delegation-history` | bool | `false` | `/accounts/{stake_address}/delegations` |
| `store-mir-history` | bool | `false` | `/accounts/{stake_address}/mirs` |
| `store-withdrawal-history` | bool | `false` | `/accounts/{stake_address}/withdrawals` |
| `store-addresses` | bool | `false` | `/accounts/{stake_address}/addresses` |
| `store-tx-count` | bool | `false` | `/accounts/{stake_address}/addresses/total` (requires `store-addresses`) |

### `[module.historical-epochs-state]`

Historical epoch data.

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `db-path` | string | `"./fjall-epochs"` | Path to Fjall database |
| `clear-on-start` | bool | `true` | Wipe database on startup |

---

## Services

### `[module.rest-server]`

The HTTP server that hosts the Blockfrost-compatible REST API.

```toml
[module.rest-server]
address = "0.0.0.0"
port = 4340
```

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `address` | string | `"0.0.0.0"` | Bind address for the REST API |
| `port` | integer | `4340` | Port for the REST API |

### `[module.rest-blockfrost]`

Blockfrost-compatible REST API handler configuration.

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `external_api_timeout` | integer | `3` | Timeout in seconds for external API calls (e.g. pool metadata fetching) |

All other settings are internal message bus topic mappings that rarely need overriding.

### `[module.mcp-server]`

Model Context Protocol (MCP) server for AI client integration. Clients connect to `http://<address>:<port>/mcp`.

```toml
[module.mcp-server]
enabled = true
address = "0.0.0.0"
port = 4341
```

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `enabled` | bool | `false` | Enable the MCP server |
| `address` | string | `"127.0.0.1"` | Bind address |
| `port` | integer | `4341` | Port for the MCP server |

### `[module.tx-submitter]`

Transaction submission to the Cardano network.

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `node-address` | string | `"backbone.cardano.iog.io:3001"` | Upstream node address for transaction submission |
| `magic-number` | integer | `764824073` | Network magic number (mainnet: `764824073`, preview: `2`) |

---

## Message Bus

The message bus section configures how modules communicate.

### `[message-bus.internal]`

In-memory message bus for single-process deployments (default, recommended).

```toml
[message-bus.internal]
class = "in-memory"
```

### `[message-bus.external]`

Optional RabbitMQ message bus for multi-process deployments.

```toml
[message-bus.external]
class = "rabbit-mq"
url = "amqp://127.0.0.1:5672/%2f"
exchange = "caryatid"
```

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `class` | string | — | Message bus type: `"in-memory"` or `"rabbit-mq"` |
| `url` | string | — | RabbitMQ connection URL (external bus only) |
| `exchange` | string | — | RabbitMQ exchange name (external bus only) |

---

## Example: Enabling All API Endpoints

To enable all REST API features, set all `store-*` flags to `true` in your config overlay:

```toml
[module.spo-state]
store-epochs-history = true
store-retired-pools = true
store-registration = true
store-updates = true
store-delegators = true
store-votes = true
store-blocks = true
store-stake-addresses = true

[module.drep-state]
store-info = true
store-delegators = true
store-metadata = true
store-updates = true
store-votes = true

[module.assets-state]
store-assets = true
store-info = true
store-history = true
store-transactions = "all"
store-addresses = true
index-by-policy = true

[module.address-state]
store-info = true
store-totals = true
store-transactions = true

[module.historical-accounts-state]
store-rewards-history = true
store-active-stake-history = true
store-registration-history = true
store-delegation-history = true
store-mir-history = true
store-withdrawal-history = true
store-addresses = true
store-tx-count = true

[module.parameters-state]
store-history = true

[module.spdd-state]
store-spdd = true

[module.drdd-state]
store-drdd = true
```

> **Note:** Enabling all flags increases memory and storage usage. Enable only the endpoints you need.
