# Block unpacker module

The block unpacker module accepts block bodies and unpacks them into
transactions

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.block-unpacker]

# Message topics
subscribe-topic = "cardano.block.proposed"
publish-topic = "cardano.txs"

```

## Messages

The block unpacker subscribes for RawBlockMessages on
`cardano.block.proposed` (see the [Upstream Chain
Fetcher](../upstream_chain_fetcher) module for details).  It unpacks
this into transactions, which it publishes as a single RawTxsMessage
on `cardano.txs`, containing the block information and an ordered vector of
raw transaction CBOR.  This ensure the transactions are kept in order.

```rust
pub struct RawTxsMessage {
    /// Block info
    pub block: BlockInfo,

    /// Raw Data for each transaction
    pub txs: Vec<Vec<u8>>,
}
```

