---
title: "MS5.08 — Acropolis: Multi-Peer Networking"
subtitle: "Delivery Report for Third-Party Audit"
author: "Acropolis Engineering Team · Input Output Engineering"
date: "April 21, 2026"
header-includes:
  - \usepackage{booktabs}
  - \usepackage{longtable}
  - \usepackage{array}
  - \renewcommand{\arraystretch}{1.4}
  - \setlength{\LTpre}{6pt}
  - \setlength{\LTpost}{6pt}
---

|                    |                                                         |
| :----------------- | :------------------------------------------------------ |
| **Project**        | 2025 Input Output Engineering Core Development Proposal |
| **Project number** | EC-0002-25-2025                                         |
| **Milestone**      | MS5.08 — Acropolis: Multi-peer networking               |
| **Prepared by**    | Acropolis Engineering Team, Input Output Engineering    |
| **Date**           | April 21, 2026                                          |

# Milestone Acceptance Criteria

As defined in the Intersect Milestone Acceptance Form (MAF) for MS5.08:

> - **Able to construct, maintain, and tear down connections with multiple peers.**
> - **Able to select the best chain following consensus rules.**

Expected deliverables:

- (a) P2P network module
- (b) Chain selection / consensus module
- (c) Test network infrastructure and reports

# Deliverables

## P2P Network Module

The P2P network module manages outbound and inbound connections to other Cardano nodes using the Cardano Node-to-Node
(N2N) mini-protocol suite. It maintains a cold peer pool (known but unconnected peers), promotes peers to hot (active)
connections, discovers new peers via Cardano's peer-sharing mini-protocol, and performs periodic churn to prevent the
node from being captured by a fixed set of peers.

**Location:** `modules/peer_network_interface/`

**Key components:**

| File                            | Responsibility                                             |
| :------------------------------ | :--------------------------------------------------------- |
| `src/peer_manager.rs`           | Cold/hot peer pool, churn, promotion/demotion              |
| `src/peer_sharing.rs`           | Cardano peer-sharing mini-protocol implementation          |
| `src/connection.rs`             | Per-connection state machine (connect, active, disconnect) |
| `src/network.rs`                | Raw N2N socket handling and event loop                     |
| `src/block_flow.rs`             | Block offer / want / fetch round-trip with peers           |
| `src/peer_network_interface.rs` | Module entry point and Caryatid registration               |

### Three-tier Peer Model

| Tier       | Description                                           |
| :--------- | :---------------------------------------------------- |
| **Hot**    | Actively connected; running ChainSync + BlockFetch    |
| **Cold**   | Known addresses held in reserve; no active connection |
| **Failed** | Session blacklist; not retried until next restart     |

### Connection Lifecycle

**Construct** — On startup, the node connects in parallel to up to `min-hot-peers` (default: 3) from the configured
address list. If there are less than `min-hot-peers`, the node will over time connect to more peers in order to meet the
criteria.

**Maintain** — If a hot peer disconnects, `NetworkManager` immediately promotes a random cold peer. If none are
available, it retries configured peers with backoff. The node shall never permanently drop below `min-hot-peers`. Over
time the node will apply churn logic to rotate peers in the hot peer pool.

**Tear down** — On shutdown, all hot connections close cleanly. Every `churn-interval` (default: 10 min), one surplus
hot peer is rotated out and replaced to avoid lock-in.

**Discover** — Periodically the node queries a cooldown-eligible hot peer via **peer-sharing (Node-to-Node V11+)**.
Valid addresses are normalised and added to the cold pool, capped at `4 x target-peer-count` (default: 60).

### Peer-Sharing Protocol Exchange

The peer-sharing exchange uses a short-lived side connection, independent of the main ChainSync/BlockFetch connection:

```
  Acropolis                        Hot Peer
     |                                |
     |-------- TCP connect ---------->|
     |-------- V11 handshake -------->|
     |         (peer-sharing=1)       |
     |-------- MsgShareRequest(15) -->|
     |<------- MsgSharePeers(...) ----|
     |-------- MsgDone -------------->|
     |                                |
     |      (connection closed)       |
```

- Implemented directly against pallas 0.35 — no library modifications and therefore this protocol has been implemented
  in Acropolis
- Responses are bounded: entries beyond `amount` are CBOR-skipped without allocation
- Per-peer 5-minute cooldown prevents excessive querying

GH issues resolved: [#780](https://github.com/input-output-hk/acropolis/issues/780),
[#781](https://github.com/input-output-hk/acropolis/issues/781)

## Chain Selection / Consensus Module

The consensus module receives block announcements from connected peers, maintains a fork-aware tree of candidate chains,
and selects the canonical chain using the `maxvalid` rule from the Ouroboros Praos specification:

> _"Return the longest valid chain; break ties in favour of the existing chain."_

When a competing fork overtakes the current tip, the module emits a rollback message. Each downstream module (UTXO
state, accounts state, etc.) handles rollback independently.

**Location:** `modules/consensus/`

**Key components:**

| File                    | Responsibility                                     |
| :---------------------- | :------------------------------------------------- |
| `src/consensus.rs`      | Module entry point, offer/want/fetch orchestration |
| `src/consensus_tree.rs` | Fork-aware chain tree; `maxvalid` selection logic  |
| `src/tree_block.rs`     | Per-block state within the tree                    |
| `src/tree_observer.rs`  | Rollback notification to downstream modules        |

### What the Consensus Module Does

- Maintains a **volatile chain tree** — all viable forks within the rollback window (_k_ blocks)
- **`check_block_wanted`**: evaluates a peer's offered block header; returns whether the node wants the body
- **`add_block`**: stores the fetched block body; fires `block_proposed` for newly favoured-chain blocks
- **`maxvalid` chain selection**: longest chain wins; equal-length ties favour the incumbent (no unnecessary rollbacks)
- **Bounded variant**: candidate chains forking deeper than _k_ blocks are rejected
- **Rollback handling**: when the favoured chain switches, downstream modules receive rollback notifications

### PNI-Consensus Integration

`BlockFlowHandler` bridges the P2P module and Consensus over the in-memory message bus:

- PNI offers block headers, published as `BlockOfferedMessage` on `cardano.consensus.offers`
- Consensus replies with wants, published as `BlockWantedMessage` on `cardano.consensus.wants`
- PNI fetches bodies, calls `add_block`, which fires `block_proposed` to downstream validators and state modules

### System Architecture

```
+------------------------------------------------------------+
|                 Peer Network Interface                     |
|                                                            |
|  peer tasks / timers / command forwarders                  |
|                 |                                          |
|                 v                                          |
|       +------------------------------+                     |
|       | NetworkManager               |                     |
|       | (single async event loop)    |                     |
|       | - owns hot peer connections  |                     |
|       | - handles NetworkEvents      |                     |
|       +--------------+---------------+                     |
|                      |                                     |
|          +-----------+-----------+                         |
|          v                       v                         |
|  +---------------+      +------------------+               |
|  | PeerManager   |      | BlockFlowHandler |               |
|  | (cold pool,   |      | Direct or        |               |
|  | discovery,    |      | Consensus mode   |               |
|  | churn)        |      +--------+---------+               |
|  +---------------+               |                         |
|          ^                       |                         |
|          |                       |                         |
|  +---------------+               |                         |
|  | peer_sharing  |               |                         |
|  | short-lived   |               |                         |
|  | side client   |               |                         |
|  +---------------+               |                         |
+----------------------------------+-------------------------+
                                   |
                     cardano.consensus.offers /
                     cardano.consensus.wants
                                   |
                                   v
                        +--------------------+
                        | Consensus module   |
                        | (maxvalid select)  |
                        +--------------------+
```

Acropolis uses a pub/sub message-passing architecture. PNI and Consensus are separate modules communicating over the
in-memory bus.

GH issues resolved: [#201](https://github.com/input-output-hk/acropolis/issues/201),
[#704](https://github.com/input-output-hk/acropolis/issues/704) via
[PR #709](https://github.com/input-output-hk/acropolis/pull/709)

## Test Network Infrastructure and Reports

A 30-node private Cardano testnet running in Docker containers, with Prometheus and Grafana monitoring. It was used to
demonstrate peer connection management, chain propagation, fork creation, longest-chain selection, and rollback in a
realistic multi-node environment.

**Location:** `testnet/`, `demo-scripts/`, `docker-compose.testnet-30.yml`

### Testnet Topology

```
pool1 -- pool2 -- pool3 -- ... -- pool30
  |         |                       |
  +---------+---- (ring + skip) ----+
        30 block-producing pools
        each connected to 4 peers
        120 total peer links
```

- **30 block-producing pools** running Haskell Cardano node (`cardano-node:10.6.2`)
- Each pool has its own KES key, VRF key, and operational certificate
- **Ring topology with skip-links**: each pool connects to 4 peers (neighbours + cross-ring), giving 2-hop reachability
  across the whole network
- Ports `3001-3030` exposed; Acropolis connects to any subset as configured peers
- Nodes are split into two subnets of 10 and 20 to allow controlled fork testing

### Fork / Partition Test Setup

After some blocks are produced, the network is split into two independent subnets to simulate a network partition,
producing competing forks. Reconnecting the subnets triggers `maxvalid` chain selection and rollback:

```

      +------------------+         +----+
      |  Acropolis Node  |-------> |    |
      +------------------+         |    | 10 nodes
                                   +----+

                                   +----+
                                   |    |
                                   |    | 20 nodes
                                   |    |
                                   |    |
                                   +----+

      +------------------+         +----+
      |  Acropolis Node  |         |    |
      +------------------+         |    | 10 nodes
                |                  +----+
                |
                |                  +----+
                |                  |    |
                |----------------> |    | 20 nodes
                                   |    |
                                   |    |
                                   +----+
```

In the test environment, the partition is controlled via a `socat` TCP proxy (`set-fork-1.sh` / `set-fork-2.sh`) that
redirects a bridge port between the two subnets.

### Infrastructure Summary

| Parameter     | Value                                                            |
| :------------ | :--------------------------------------------------------------- |
| Nodes         | 30 block-producing stake pools                                   |
| Node software | `cardano-node:10.6.2`                                            |
| Network magic | 42 (private testnet)                                             |
| Topology      | Ring with skip-links — 4 peers/node, 120 total links             |
| P2P           | Enabled with peer sharing on all nodes                           |
| Monitoring    | Prometheus (ports 9101-9130) + Grafana (`http://localhost:3000`) |

### Demo Scripts

| Script             | Purpose                                           |
| :----------------- | :------------------------------------------------ |
| `start-cluster.sh` | Start all 30 nodes via Docker Compose             |
| `stop-cluster.sh`  | Stop and clean up all nodes                       |
| `get-pool-1.sh`    | Query chain tip from node 1 (subnet A)            |
| `get-pool-2.sh`    | Query chain tip from node 11 (subnet B)           |
| `set-fork-1.sh`    | Redirect bridge port to node 3 (create partition) |
| `set-fork-2.sh`    | Redirect bridge port to node 28 (heal partition)  |
| `reset-genesis.sh` | Reset genesis timestamps to 2 minutes from now    |

### Automated Test Suite

| Area                      | What is tested                                                                                      |    Tests |
| :------------------------ | :-------------------------------------------------------------------------------------------------- | -------: |
| PeerManager               | Cold pool seeding, promotion, cap/eviction, per-peer cooldown, churn floor                          |       14 |
| Peer Sharing Protocol     | Address validation (loopback, RFC1918, link-local, port 0, IPv4-mapped), protocol exchange, timeout |       18 |
| NetworkManager            | Peer recovery on disconnect, discovery & churn wiring, disabled mode, block fetch retry             |       10 |
| BlockFlow Handler         | Offer/want/rescind lifecycle, fork tracking, lagging peers, chain sync point sampling               |       33 |
| ChainState                | Published block window, out-of-order bodies, rollback, upstream switch, chain fork                  |        9 |
| PNI-Consensus Integration | Full offer/want/fetch/proposed round-trips, fork competition, rollback, `maxvalid` selection        |       14 |
| **Total**                 |                                                                                                     | **~100** |

# Results reproduction

## Prerequisites

- Docker
- Rust toolchain
- `socat` or similar TCP proxy

## Test Scenario

Reproducing the results, a local testnet is required. As mentioned in the deliverables, a 30-node testnet is available
in the `testnet/` directory. The testnet is configured to have a ring topology with skip-links, where each node is
connected to 4 peers. This allows for a realistic multi-node environment to demonstrate peer connection management,
chain propagation, fork creation, longest-chain selection, and rollback.

In order to make the reprocess easier a set of scripts have been prepared.
Running the Acropolis node together with the testnet in the docker requires the node to be able to see the docker
network. The node has a default setting for local docker gateway IP (`localhost-gateway-ip`), if your docker gateway IP 
is different, it needs to be adjusted.

There are two options to run testnets: fresh testnet or already prepared, forked testnet.

* Fresh testnet:

1. Use Acropolis node from `MS-5-8-tests-v1` tag.
1. Before starting the cluster, run `reset-genesis.sh` to set genesis timestamps to 2 minutes from your current now.
   This is necessary to have synchronized nodes. This script will also clean the 'database'.
1. Start the cluster with `start-cluster.sh`.
1. Let the network running for a while. Check with `get-pool-1.sh` script how many blocks are produced.
1. Once some blocks are produced, run `set-fork-topology.sh`. This script will stop the cluster, then adjust topology
   and then start the cluster again.
1. Let the network produce some blocks again. This time it will produce two forks.
1. Setup the TCP proxy with `set-fork-1.sh`.
1. Execute `make run-testnet`. This will build and run the Acropolis node connected to the first network partition (fork).
1. Once there are some diverged blocks on the forks (check with `get-pool-1.sh` and `get-pool-2.sh`), run
   `set-fork-2.sh` to switch the proxy.
1. Observe the logs of the Acropolis node executing rollbacks.

Note that at any step when Acropolis is running and connected to the test network, p2p sharing can be observed in the
logs.


* Forked testnet:

This testnet will not produce new blocks but already contains two forks.

1. Use Acropolis node from `MS-5-8-tests-v1` tag.
1. Start the cluster with `start-cluster-forked.sh`.
1. Setup the TCP proxy with `set-fork-1.sh`.
1. Execute `make run-testnet`. This will build and run Acropolis node connected to the first network partition (fork).
1. Run `set-fork-2.sh` in order to switch the proxy (fork).

**Automated test verification** (no Docker required):

```bash
cargo test -p acropolis_module_peer_network_interface
cargo test -p acropolis_module_consensus
```

# Acceptance Criteria Mapping

| Acceptance Criterion                        | Deliverable                                            | Verification                                                                                                    |
| :------------------------------------------ | :----------------------------------------------------- | :-------------------------------------------------------------------------------------------------------------- |
| Construct connections with multiple peers   | `peer_manager.rs`, `connection.rs`                     | 30-node testnet: connections formed on startup                                                                  |
| Maintain connections with multiple peers    | Churn logic, cold-to-hot promotion in `NetworkManager` | Sustained block propagation; 10 `NetworkManager` recovery tests                                                 |
| Tear down connections with multiple peers   | Clean disconnect on churn / shutdown                   | `stop-cluster.sh`; unit test: all announcers disconnect cleanly                                                 |
| Select best chain following consensus rules | `maxvalid` rule in `consensus_tree.rs`                 | Two chain-selection unit tests (longer fork wins; equal-length tie-breaks to incumbent); testnet fork/heal demo |
