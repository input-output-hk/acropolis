# Block unpacker module

The block unpacker module accepts block bodies and unpacks them into transactions

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.block-unpacker]

# Message topics
subscribe-topic = cardano.block.body
publish-topic = cardano.tx

```

## Messages

The block unpacker subscribes for BlockBodyMessages on `cardano.block.body`
(see the [Mini-protocols](../miniprotocols) module for details).  It unpacks
this into transactions, which it publishes as multiple TxMessages on `cardano.tx`,
containing the slot number, transaction index in the block, and raw CBOR:

```rust
pub struct TxMessage {
    /// Slot number
    pub slot: u64,

    /// Index in block
    pub index: u32,

    /// Raw Data
    pub raw: Vec<u8>,
}
```

