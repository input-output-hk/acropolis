# Consensus module

The consensus module takes proposed blocks from (optionally multiple) upstream
sources and decides which chain to favour, passing on blocks on the favoured chain
to other validation and storage modules downstream

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.consensus]

# Message topics
subscribe-blocks-topic = "cardano.block.available"
publish-blocks-topic = "cardano.block.proposed"

```

## Messages

The consensus module subscribes for RawBlockMessages on
`cardano.block.available`.  It uses the consensus rules to
decide which of multiple chains to favour, and sends candidate
blocks on `cardano.block.proposed` to request validation and storage.

Both input and output are RawBlockMessages:

```rust
pub struct RawBlockMessage {
    /// Header raw data
    pub header: Vec<u8>,

    /// Body raw data
    pub body: Vec<u8>,
}
```

