# Snapshot Bootstrapper Module

The snapshot bootstrapper module downloads and processes a Cardano ledger snapshot to initialize system state before
processing the live chain.

## Overview

This module:

1. Waits for genesis bootstrap completion
2. Downloads a gzip-compressed NES snapshot and matching UTxO sidecar from configured HTTP URLs
3. Streams and publishes snapshot data (UTXOs, pools, accounts, DReps, proposals)
4. Signals completion to allow chain synchronization to begin

## Messages

The snapshot bootstrapper:

- **Subscribes to** `cardano.sequence.bootstrapped` - Waits for genesis completion
- **Publishes to** `cardano.snapshot` - Streams snapshot data during processing
- **Publishes to** `cardano.sync.command` - Signals completion with point to begin sync from

## Default Configuration

```toml
[global.startup]
network-name = "mainnet"

[module.snapshot-bootstrapper]

# Target epoch to download and process.
epoch = 500

# Data directory
data-dir = "./data"

# Message topics
snapshot-topic = "cardano.snapshot"
bootstrapped-subscribe-topic = "cardano.sequence.bootstrapped"
sync-command-topic = "cardano.sync.command"

# Download settings
[download]
timeout-secs = 300
connect-timeout-secs = 30
progress-log-interval = 200
```

## Directory Structure

The module expects the following files in `{data-dir}/{network}/`:

- **`snapshots.json`** - Snapshot metadata including HTTP download URLs

Each snapshot entry provides:

- `url` - the NES artifact URL, expected to point to a `nes.<slot>.<hash>.cbor.gz` object
- `utxo_url` - an optional explicit UTxO sidecar URL; if omitted, the bootstrapper derives a sibling `utxos.<slot>.<hash>.cbor.gz` URL in the same directory

CloudFront/S3-hosted artifacts work as normal HTTP downloads. The bootstrapper expects the
published objects to be gzip-compressed `.cbor.gz` files and stores the decompressed results as:

- `{data-dir}/{network}/nes.<slot>.<hash>.cbor`
- `{data-dir}/{network}/utxos.<slot>.<hash>.cbor`

## Example config.json

```json
{
  "snapshot": 500,
  "points": [
    {
      "epoch": 500,
      "id": "abc123...",
      "slot": 12345678
    }
  ]
}
```
