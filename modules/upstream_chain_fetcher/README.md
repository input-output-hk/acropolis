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
sync-point = "snapshot"   # or "origin", "tip"

# Message topics
header-topic = "cardano.block.header"
body-topic = "cardano.block.body"
snapshot-complete-topic = "cardano.snapshot.complete"
```

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

