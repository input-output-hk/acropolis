# Snapshot Bootstrapper Module

The snapshot bootstrapper module downloads and processes Cardano ledger snapshots to initialize system state before
processing the live chain.

## Overview

This module:

1. Waits for genesis bootstrap completion
2. Downloads compressed snapshot files from configured URLs
3. Streams and publishes snapshot data (UTXOs, pools, accounts, DReps, proposals)
4. Signals completion to allow chain synchronization to begin

## Messages

The snapshot bootstrapper:

- **Subscribes to** `cardano.sequence.start` - Waits for startup signal
- **Subscribes to** `cardano.sequence.bootstrapped` - Waits for genesis completion
- **Publishes to** `cardano.snapshot` - Streams snapshot data during processing
- **Publishes to** `cardano.snapshot.complete` - Signals completion with block info

## Default Configuration

```toml
[module.snapshot-bootstrapper]

# Network and data
network = "mainnet"
data-dir = "./data"

# Message topics
startup-topic = "cardano.sequence.start"
snapshot-topic = "cardano.snapshot"
bootstrapped-subscribe-topic = "cardano.sequence.bootstrapped"
completion-topic = "cardano.snapshot.complete"
```

## Directory Structure

The module expects the following files in `{data-dir}/{network}/`:

- **`config.json`** - Network configuration specifying which snapshot epochs to load
- **`snapshots.json`** - Snapshot metadata including download URLs

Snapshot files are downloaded to `{data-dir}/{network}/{point}.cbor`.