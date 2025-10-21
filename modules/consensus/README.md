# Consensus module

The consensus module takes proposed blocks from a (later, multiple) upstream
source and decides which chain to favour, passing on blocks on the favoured chain
to other validation and storage modules downstream

## Configuration

The following is the default configuration - these are the default
topics so they can be left out if they are OK.  The validators *must*
be configured - if empty, no validation is performed

```toml
[module.consensus]

# Message topics
subscribe-blocks-topic = "cardano.block.available"
publish-blocks-topic = "cardano.block.proposed"

# Validation result topics
validators = [
           "cardano.validation.vrf",
           "cardano.validation.kes",
           "cardano.validation.utxo"
           ...
]

```

## Validation

The consensus module passes on blocks it receives from upstream (currently only a
single source) and sends them out as 'proposed' blocks for validation.  It then listens
on all of the `validators` topics for BlockValidation messages, which give a Go / NoGo
for the block.  The model is a NASA flight control desk, and like there, a single NoGo
is enough to stop the block.

At the moment the module simply logs the validation failure.  Once it is actually operating
consensus across multiple sources, it will use this and the length of chain to choose the best
chain.

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

