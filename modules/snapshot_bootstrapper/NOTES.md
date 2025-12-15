# Bootstrapping from a Snapshot file

We can boot an Acropolis node either from genesis and replay all of the blocks up to
some point, or we can boot from a snapshot file. This module provides the components
needed to boot from a snapshot file.
See [snapshot_bootstrapper](src/bootstrapper.rs) for the process that
references and runs with these helpers.

Booting from a snapshot should take minutes instead of the hours it takes to boot from
genesis. It also allows booting from a given epoch, which allows one to create tests
that rely only on that epoch of data. We're also skipping some of the problematic
eras and will typically boot from Conway. It takes only 1 NewEpochState cbor dump
to bootstrap the node.

The required data for bootstrapping are:

- snapshot file (with an associated epoch number and point (slot + block hash))
- nonces
- block.{slot}.{hash}.cbor file -- this cbor file is decoded to get the necessary block information (currently the block number) so that once bootstrap process is completed, we can send the block that we're currently synced to to the [PeerNetworkInterface](../peer_network_interface/src/peer_network_interface.rs).

## Snapshot Files

The snapshot approach comes from the Amaru project. In their words,
"the snapshots we generated are different from a NewEpochState dump that's requested from an Ogmios GetCBOR endpoint
with a synchronizing node: they're the actual ledger state; i.e. the in-memory state that is constructed by iterating
over each block up to a specific point. So, it's all the UTxOs, the set of pending governance actions, the account
balance, etc.
If you get this from a trusted source, you don't need to do any replay, you can just start up and load this from the
disk."

The snapshot file is referenced by its epoch number in the config.json file below.

See [Amaru snapshot format](../../docs/amaru-snapshot-structure.md)

## Configuration files

There is a path for each network bootstrap configuration file. Network should
be one of 'mainnet', 'preprod', 'preview' or 'testnet\_<magic>' where
`magic` is a 32-bits unsigned value denoting a particular testnet.

Data structure, e.g. as [Amaru mainnet](https://github.com/pragma-org/amaru/tree/main/data/mainnet)

The bootstrapper will be given a path to a directory that is expected to contain
the following a snapshots.json. The path will
be used as a prefix to resolve per-network configuration files
needed for bootstrapping. Given a source directory `data`, and a
a network name of `preview`, the expected layout for configuration files would be:

- `data/preview/snapshots.json`: a list of `Snapshot` values (epoch, point, url)

This file along with the TOML config is loaded by [snapshot_bootstrapper](src/bootstrapper.rs)
during bootup.

## Block CBOR and Nonces 

In order to retrieve block CBOR data and ledger state nonces, we can use [Demeter's managed services](https://demeter.run/products) (Mumak PostgreSQL and Ogmios).

### Prerequisites

A Demeter account with access to:

- [Mumak](https://demeter.run/products) PG (PostgreSQL instance with Cardano chain data)
- [Ogmios](https://demeter.run/products) ([HTTP JSON-RPC interface](https://ogmios.dev/http-api/#operation/jsonRPC) to a Cardano node)
- `curl`, `jq`, `xxd`, and `psql` installed

---

### Get Block CBOR from Mumak PG


postgresql://<username>:<password>@<host>:<port>/<database>
```

Query block CBOR by slot and save to file:

```bash
psql "postgresql://<username>:<password>@<host>:<port>/<database>" -t -A -c "
SELECT encode(cbor, 'hex') FROM blocks WHERE slot = 134092758;
" | xxd -r -p > block.cbor
```

The `-t -A` flags remove headers and alignment, giving you just the raw hex output.

---

### Query Ledger State Nonces from Ogmios


Given a url such as: `https://<instance-id>.cardano-mainnet-v6.ogmios-m1.dmtr.host`
```

Query nonces and transform with jq:

```bash
curl -s -X POST "https://<your-ogmios-endpoint>.dmtr.host" \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc": "2.0", "method": "queryLedgerState/nonces", "id": 509}' | jq '{
    active: .result.epochNonce,
    candidate: .result.candidateNonce,
    evolving: .result.evolvingNonce,
    tail: .result.lastEpochLastAncestor
  }'
```

Output:

```json
{
  "active": "4c4fbd257da2ddea6f3f4af5056eb36cf4da3aea7088acb07907ce68a4f73b97",
  "candidate": "f885c04321729ec680086413477ac46d34021fe693882f38365a38e1eac82bed",
  "evolving": "f885c04321729ec680086413477ac46d34021fe693882f38365a38e1eac82bed",
  "tail": "8f983bd7d905ea5b4f552df019718c860f90aab803f48b11b94b98e2cc2ebaa6"
}
```
Where these fields mean the following:
- `epochNonce` / `active` => The nonce for the current epoch          
- `candidateNonce`/ `candidate` => The candidate nonce for the next epoch   
- `evolvingNonce` /`evolving` => The evolving nonce (updated each block)  
- `lastEpochLastAncestor` / `tail` => Block hash of last block in prior epoch  

## Bootstrapping sequence

The bootstrapper will be started with a configuration that specifies a network,
e.g. "mainnet". From the network, it will build a path to the configuration
and snapshot files as shown above, then load the data contained or described
in those files. config.json holds a single epoch that is used to look up the
corresponding URL and metadata in snapshots.json for the snapshot file.
Loading occurs in this order:

1. Wait for `bootstrapped-topic` message with genesis values (typically `cardano.sequence.bootstrapped`)
2. Load network configuration from [Omninbus config](../../processes/omnibus/omnibus.toml)
   or [default toml](config.default.toml)
3. Load snapshot metadata from `snapshots.json`
4. Find snapshot matching the epoch specified in TOML config (step 2)
5. Download snapshot file (skips if already present)
6. Publish `SnapshotMessage::Startup` to the snapshot topic
7. Parse the snapshot file using the [streaming_snapshot](../../common/src/snapshot/streaming_snapshot.rs)
8. Publish `CardanoMessage::SnapshotComplete` with final block info to the completion topic

Modules in the system will have subscribed to the startup and completion topics before the
bootstrapper runs the above sequence. Upon receiving snapshot data messages,
they will use the data to populate their state, and any other state required to achieve readiness to operate.

## Data update messages

The bootstrapper publishes data as it parses the snapshot file using the `SnapshotPublisher`.
Snapshot parsing is done while streaming the data to keep the memory
footprint lower. As elements of the file are parsed, callbacks provide the data
to the publisher, which can then publish structured data on the message bus.

The `SnapshotPublisher` implements the streaming snapshot callbacks:

- `UtxoCallback`: Receives individual UTXO entries
- `PoolCallback`: Receives pool information
- `AccountsCallback`: Receives account/stake information
- `DRepCallback`: Receives DRep (delegated representative) information
- `ProposalCallback`: Receives governance proposals
- `SnapshotCallbacks`: Receives metadata and completion signals

Currently, the publisher just accumulates this data, but this will need to be extended to publish the corresponding
message types. Publishing of detailed snapshot data to downstream modules can be added by implementing the
appropriate message bus publishing in the callback methods.

## Configuration

The bootstrapper supports the following configuration options:

- `network`: Network name (default: "mainnet")
- `data-dir`: Base directory for network data (default: "./data")
- `snapshot-topic`: Topic to publish snapshot messages (default: "cardano.snapshot")
- `bootstrapped-subscribe-topic`: Topic to receive genesis completion (default: "cardano.sequence.bootstrapped")
- `completion-topic`: Topic to publish completion signal (default: "cardano.snapshot.complete")
