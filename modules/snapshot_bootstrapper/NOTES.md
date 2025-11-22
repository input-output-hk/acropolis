# Bootstrapping from a Snapshot file

We can boot an Acropolis node either from genesis and replay all of the blocks up to
some point, or we can boot from a snapshot file. This module provides the components
needed to boot from a snapshot file.
See [snapshot_bootstrapper](../../modules/snapshot_bootstrapper/src/snapshot_bootstrapper.rs) for the process that
references and runs with these helpers.

Booting from a snapshot takes minutes instead of the hours it takes to boot from
genesis. It also allows booting from a given epoch which allows one to create tests
that rely only on that epoch of data. We're also skipping some of the problematic
eras and will typically boot from Conway around epoch 305, 306, and 307. It takes
three epochs to have enough context to correctly calculate the rewards.

The required data for bootstrapping are:

- snapshot files (each has an associated epoch number and point)
- nonces
- headers

## Snapshot Files

The snapshots come from the Amaru project. In their words,
"the snapshots we generated are different [from a Mithril snapshot]: they're
the actual ledger state; i.e. the in-memory state that is constructed by iterating over each block up to a specific
point. So, it's all the UTxOs, the set of pending governance actions, the account balance, etc.
If you get this from a trusted source, you don't need to do any replay, you can just start up and load this from disk.
The format of these is completely non-standard; we just forked the haskell node and spit out whatever we needed to in
CBOR."

Snapshot files are referenced by their epoch number in the config.json file below.

See [Amaru snapshot format](../../docs/amaru-snapshot-structure.md)

## Configuration files

There is a path for each network bootstrap configuration file. Network should
be one of 'mainnet', 'preprod', 'preview' or 'testnet_<magic>' where
`magic` is a 32-bits unsigned value denoting a particular testnet.

Data structure, e.g. as [Amaru mainnet](https://github.com/pragma-org/amaru/tree/main/data/mainnet)

The bootstrapper will be given a path to a directory that is expected to contain
the following files: snapshots.json and config.json. The path will
be used as a prefix to resolve per-network configuration files
needed for bootstrapping. Given a source directory `data`, and a
a network name of `preview`, the expected layout for configuration files would be:

* `data/preview/config.json`: a list of epochs to load and points
* `data/preview/snapshots.json`: a list of `SnapshotFileMetadata` values (epoch, point, url)

These files are loaded by [snapshot_bootstrapper](../../modules/snapshot_bootstrapper/src/snapshot_bootstrapper.rs)
during bootup.

## Bootstrapping sequence

The bootstrapper will be started with a configuration that specifies a network,
e.g. "mainnet". From the network, it will build a path to the configuration
and snapshot files as shown above, then load the data contained or described
in those files. config.json holds a list of typically 3 epochs that can be
used to index into snapshots.json to find the corresponding URLs and meta-data
for each of the three snapshot files. Loading occurs in this order:

1. Wait for `startup-topic` message (typically `cardano.sequence.start`)
2. Wait for `bootstrapped-topic` message with genesis values (typically `cardano.sequence.bootstrapped`)
3. Load network configuration from `config.json`
4. Load snapshot metadata from `snapshots.json`
5. Filter snapshots based on epochs specified in config.json
6. Download snapshot files (skips if already present)
7. Publish `SnapshotMessage::Startup` to the snapshot topic
8. Parse each snapshot file using the streaming parser
9. Publish `CardanoMessage::SnapshotComplete` with final block info to the completion topic

Modules in the system will have subscribed to the startup and completion topics before the
bootstrapper runs the above sequence. Upon receiving snapshot data messages,
they will use the data to populate their state, history (for BlockFrost),
and any other state required to achieve readiness to operate.

## Data update messages

The bootstrapper publishes data as it parses the snapshot files using the `SnapshotPublisher`.
Snapshot parsing is done while streaming the data to keep the memory
footprint lower. As elements of the file are parsed, callbacks provide the data
to the publisher which can then publish structured data on the message bus.

The `SnapshotPublisher` implements the streaming snapshot callbacks:

- `UtxoCallback`: Receives individual UTXO entries
- `PoolCallback`: Receives pool information
- `StakeCallback`: Receives account/stake information
- `DRepCallback`: Receives DRep (delegated representative) information
- `ProposalCallback`: Receives governance proposals
- `SnapshotCallbacks`: Receives metadata and completion signals

Currently the publisher accumulates this data for statistics and future use. Publishing
of detailed snapshot data to downstream modules can be added by implementing the
appropriate message bus publishes in the callback methods.

## Configuration

The bootstrapper supports the following configuration options:

- `network`: Network name (default: "mainnet")
- `data-dir`: Base directory for network data (default: "./data")
- `startup-topic`: Topic to wait for startup signal (default: "cardano.sequence.start")
- `snapshot-topic`: Topic to publish snapshot messages (default: "cardano.snapshot")
- `bootstrapped-subscribe-topic`: Topic to receive genesis completion (default: "cardano.sequence.bootstrapped")
- `completion-topic`: Topic to publish completion signal (default: "cardano.snapshot.complete")