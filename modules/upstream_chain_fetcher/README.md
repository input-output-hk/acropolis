# Upstream chain fetcher module

The upstream chain fetcher module provides a Ouroboros network client using
ChainSync and BlockFetch to fetch blocks from a single upstream source.

It can either run independently, either from the origin or current tip, or
be triggered by a Mithril snapshot event (the default) where it starts from
where the snapshot left off, and follows the chain from there.

Rollbacks are handled by signalling in the block data - it is downstream
subscribers' responsibility to deal with the effects of this.

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.upstream-chain-fetcher]

# Upstream node connection
node-address = "backbone.cardano.iog.io:3001"
magic-number = 764824073

# Initial sync point
sync-point = "snapshot"   # or "origin", "tip", "cache"

# Message topics
header-topic = "cardano.block.header"
body-topic = "cardano.block.body"
snapshot-complete-topic = "cardano.snapshot.complete"
```

### Sync point modes (`sync-point` parameter)

Upstream fetching is very slow, so it may be an acceptable optimisation 
to take the initial part of the blockchain (which is produced long time ago and 
cannot be changed anymore) from another (off-chain) source.
In another words, fetching may start not from the origin but from a different
synchronisation point. Here are the possible variants:

* Fetch from the origin (`origin`, `tip` modes). No optimisations.

* Fetch after snapshot is replayed (`snapshot`). Snapshot is downloaded 
by other module, and when all snapshot messages are processed, that module 
sends `SnapshotComplete` message in `snapshot-complete-topic` topic.
The last snapshot message then serves as the synchronisation point, after
which fetching is started.

* Fetch after cache is replayed (`cache`). Similar to `origin` mode, 
but the received messages are saved on disk into directory, specified 
in `cache-dir` parameter (`upload-cache` is the default value).
When the node is restarted, the cached messages are not downloaded 
again, but are taken from the directory instead. The last message in 
cache serves as the synchronisation point.

## Messages

When the chain rolls forward, it sends a BlockHeaderMessage on topic
`cardano.block.header`, containing the slot number, header number and
raw CBOR of the header:

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

It then fetches the corresponding block body and sends this as a
BlockBodyMessage on topic `cardano.block.body`, containing the slot
number and raw CBOR of the body:

```rust
pub struct BlockBodyMessage {
    /// Block info
    pub block: BlockInfo,

    /// Raw Data
    pub raw: Vec<u8>,
}
```

Note that the chain fetcher currently assumes everything is volatile.
If it gets a RollBackward from the upstream, it will remember this and
the next header and body message generated on RollForward will be
tagged with status `RolledBack`.
