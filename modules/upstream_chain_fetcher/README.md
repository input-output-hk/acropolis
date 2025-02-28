# Network Mini-protocols module

The Network Mini-protocols module provides a multi-connection, multi-protocol
client implementing the Cardano node mini-protocol multiplexer layer.

Currently it only supports a single connection, and uses ChainSync from the chain tip
to fetch new headers, and BlockFetch one-at-a-time to fetch them.  It handles rollbacks
only to the extent that a subscriber to the block headers or bodies will see a rewind
in the slot number.

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.miniprotocols]

# Upstream node connection
node-address = "preview-node.world.dev.cardano.org:30002"
magic-number = 2

# Initial sync point
sync-point = "tip"   # or "origin"

# Message topics
header-topic = "cardano.block.header"
body-topic = "cardano.block.body"

```

## Messages

When the chain rolls forward, it sends a BlockHeaderMessage on topic
`cardano.block.header`, containing the slot number, header number and
raw CBOR of the header:

```rust
pub struct BlockHeaderMessage {
    /// Slot number
    pub slot: u64,

    /// Header number
    pub number: u64,

    /// Raw Data
    pub raw: Vec<u8>,
}
```

It then fetches the corresponding block body and sends this as a BlockBodyMessage
on topic `cardano.block.body`, containing the slot number and raw CBOR of the body:

```rust
pub struct BlockBodyMessage {
    /// Slot number
    pub slot: u64,

    /// Raw Data
    pub raw: Vec<u8>,
}
```

