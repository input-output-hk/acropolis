# Mithril snapshot fetcher module

The Mithril snapshot fetcher fetches a signed chain snapshot from
Mithril servers and replays all the blocks from it.

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.mithril-snapshot-fetcher]

# Message topics
header-topic = "cardano.block.header"
body-topic = "cardano.block.body"

```

## Messages

For each block in the snapshot, the fetcher sends a BlockHeaderMessage on topic
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

It then sends the block body as a BlockBodyMessage
on topic `cardano.block.body`, containing the slot number and raw CBOR of the body:

```rust
pub struct BlockBodyMessage {
    /// Slot number
    pub slot: u64,

    /// Raw Data
    pub raw: Vec<u8>,
}
```


