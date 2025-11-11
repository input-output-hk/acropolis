# Bootstrapping from a Snapshot file

We can boot an Acropolis node either from geneis and replay all of the blocks up to
some point, or we can boot from a snapshot file. This module provides the components
needed to boot from a snapshot file.
See [snapshot_bootsrapper](../../../modules/snapshot_bootstrapper/src/snapshot_bootstrapper.rs) for the process that
references and runs with these helpers.

Booting from a snapshot takes minutes instead of the hours it takes to boot from
genesis. It also allows booting from a given epoch which allows one to create tests
that rely only on that epoch of data. We're also skipping some of the problematic
eras and will typically boot from Conway around epoch 305, 306, and 307. It takes
three epochs to have enough context to correctly calculate the rewards.

The required data for boostrapping are:

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

See [Amaru snapshot format](../../../docs/amaru-snapshot-structure.md)

## Configuration files

There is a path for each network bootstrap configuration file. Network Should
be one of 'mainnet', 'preprod', 'preview' or 'testnet_<magic>' where
`magic` is a 32-bits unsigned value denoting a particular testnet.

Data structure, e.g. as [Amaru mainnet](https://github.com/pragma-org/amaru/tree/main/data/mainnet)

The bootstrapper will be given a path to a directory that is expected to contain
the following files: snapshots.json, nonces.json, and headers.json. The path will
be used as a prefix to resolve per-network configuration files
needed for bootstrapping. Given a source directory `data`, and a
a network name of `preview`, the expected layout for configuration files would be:

* `data/preview/config.json`: a list of epochs to load.
* `data/preview/snapshots.json`: a list of `Snapshot` values (epoch, point, url)
* `data/preview/nonces.json`: a list of `InitialNonces` values,
* `data/preview/headers.json`: a list of `Point`s.

These files are loaded by [snapshot_bootsrapper](../../../modules/snapshot_bootstrapper/src/snapshot_bootstrapper.rs)
during bootup.

## Bootstrapping sequence

The bootstrapper will be started with an argument that specifies a network,
e.g. "mainnet". From the network, it will build a path to the configuration
and snapshot files as shown above, then load the data contained or described
in those files. config.json holds a list of typically 3 epochs that can be
used to index into snapshots.json to find the corresponding URLs and meta-data
for each of the three snapshot files. Loading occurs in this order:

* publish `SnapshotMessage::Startup`
* download the snapshots (on demand; may have already been done externally)
* parse each snapshot and publish their data on the message bus
* read nonces and publish
* read headers and publish
* publish `CardanoMessage::GenesisComplete(GenesisCompleteMessage {...})`

Modules in the system will have subscribed to the Startup message and also
to individual structural data update messages before the
boostrapper runs the above sequence. Upon receiving the `Startup` message,
they will use data messages to populate their state, history (for BlockFrost),
and any other state required to achieve readiness to operate on reception of
the `GenesisCompleteMessage`.

## Data update messages

The bootstrapper will publish data as it parses the snapshot files, nonces, and
headers. Snapshot parsing is done while streaming the data to keep the memory
footprint lower. As elements of the file are parsed, callbacks provide the data
to the boostrapper which publishes the data on the message bus.

There are TODO markers in [snapshot_bootsrapper](../../../modules/snapshot_bootstrapper/src/snapshot_bootstrapper.rs)
that show where to add the
publishing of the parsed snapshot data.



