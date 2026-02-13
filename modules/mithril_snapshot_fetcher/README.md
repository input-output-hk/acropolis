# Mithril snapshot fetcher module

The Mithril snapshot fetcher fetches a signed chain snapshot from
Mithril servers and replays all the blocks from it.

It will wait for
a startup event before beginning to allow the
[Genesis Bootstrapper](../genesis_bootstrapper) to complete.

When it has finished it sends a ChainSync::FindIntersect command indicating the
last block fetched, which is used by the
[Peer Network Interface](../peer_network_interface) to synchronize ongoing 
fetches.

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.mithril-snapshot-fetcher]

# Mithril source
aggregator-url = "https://aggregator.release-mainnet.api.mithril.network/aggregator"
genesis-key = "5b3139312c36362c3134302c3138352c3133382c31312c3233372c3230372c3235302c3134342c32372c322c3138382c33302c31322c38312c3135352c3230342c31302c3137392c37352c32332c3133382c3139362c3231372c352c31342c32302c35372c37392c33392c3137365d"

# Storage
directory = "../../modules/mithril_snapshot_fetcher/downloads/mainnet"

# Message topics
startup-topic = "cardano.sequence.bootstrapped"
header-topic = "cardano.block.header"
body-topic = "cardano.block.body"
completion-topic = "cardano.shapshot.complete"

```

## Messages

The fetcher waits for a `cardano.sequence.bootstrapped` message (with
no content) before starting.

For each block in the snapshot, the fetcher sends a BlockHeaderMessage
on topic `cardano.block.header`, block information and raw CBOR of the
header:

```rust
pub enum BlockStatus
{
    Bootstrap,   // Pseudo-block from bootstrap data
    Immutable,   // Now immutable (more than 'k' blocks ago)
    Volatile,    // Volatile, in sequence
    RolledBack,  // Volatile, restarted after rollback
}

pub struct BlockInfo {
    /// Block status
    pub status: BlockStatus,

    /// Slot number
    pub slot: u64,

    /// Block number
    pub number: u64,

    /// Block hash
    pub hash: Vec<u8>,
}

pub struct BlockHeaderMessage {
    /// Block info
    pub block: BlockInfo,

    /// Raw Data
    pub raw: Vec<u8>,
}
```

All blocks will be tagged as immutable.

It then sends the block body as a BlockBodyMessage on topic
`cardano.block.body`, containing the same block information and raw
CBOR of the body:

```rust
pub struct BlockBodyMessage {
    /// Block info
    pub block: BlockInfo,

    /// Raw Data
    pub raw: Vec<u8>,
}
```

When the snapshot has been fully replayed, it sends a
`cardano.sync.command` message with details of the last point in
the snapshot:

```
pub enum ChainSyncCommand {
    // The point from which to begin fetching blocks from
    FindIntersect(Point),
}
```
