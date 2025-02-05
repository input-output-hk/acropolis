# Transaction unpacker module

The transaction unpacker module accepts raw transactions and unpacks them into UTXO events.

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.tx-unpacker]

# Message topics
subscribe-topic = cardano.tx
publish-input-topic = "cardano.utxo.spent"
publish-output-topic = "cardano.utxo.created"

```

## Messages

The transaction unpacker subscribes for TxMessages on `cardano.tx`
(see the [Block Unpacker](../block_unpacker) module for details).  It decodes
the transactions (without any validation!), and extracts the inputs and outputs.

Inputs are published as multiple InputMessages on `cardano.utxo.spent`,
containing the slot number, transaction index in the block, input index in the
transaction, referenced transaction hash and referenced output index:

```rust
pub struct InputMessage {
    /// Slot number
    pub slot: u64,

    /// Tx index in block
    pub tx_index: u32,

    /// Inpu index in tx
    pub index: u32,

    /// Tx hash of referenced UTXO
    pub ref_hash: Vec<u8>,

    /// Index of UTXO in referenced tx
    pub ref_index: u64,
}
```

Outputs are published as multiple OutputMessages on `cardano.utxo.created`,
containing the slot number, transaction index in the block, transaction hash,
output index, address and value:

```rust
pub struct OutputMessage {
    /// Slot number
    pub slot: u64,

    /// Tx index in block
    pub tx_index: u32,

    /// Tx hash
    pub tx_hash: Vec<u8>,

    /// Output index in tx
    pub index: u32,

    /// Address data (raw)
    pub address: Vec<u8>,

    /// Output value (Lovelace)
    pub value: u64,
}
```


