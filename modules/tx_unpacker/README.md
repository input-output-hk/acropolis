# Transaction unpacker module

The transaction unpacker module accepts raw transactions and unpacks
them into UTXO events.

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.tx-unpacker]

# Message topics
subscribe-topic = "cardano.tx"
publish-utxo-deltas-topic = "cardano.utxo.deltas"

```

## Messages

The transaction unpacker subscribes for TxMessages on `cardano.txs`
(see the [Block Unpacker](../block_unpacker) module for details).  It decodes
the transactions (without any validation!), and extracts the inputs and outputs.

Inputs are published as a single UTXODeltasMessage on
`cardano.utxo.deltas`, containing the block information, and an ordered
list of input and output changes for the entire block.  This ensure
the deltas are kept in order and are valid in case of intra-block
references:


```rust
/// Transaction output (UTXO)
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxOutput {
    /// Tx hash
    pub tx_hash: Vec<u8>,

    /// Output index in tx
    pub index: u32,

    /// Address data (raw)
    pub address: Vec<u8>,

    /// Output value (Lovelace)
    pub value: u64,
}

/// Transaction input (UTXO reference)
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct TxInput {
    /// Tx hash of referenced UTXO
    pub tx_hash: Vec<u8>,

    /// Index of UTXO in referenced tx
    pub index: u64,
}

/// Option of either TxOutput or TxInput
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum UTXODelta {
    None(()),
    Output(TxOutput),
    Input(TxInput),
}

impl Default for UTXODelta {
    fn default() -> Self {
        UTXODelta::None(())
    }
}

/// Message encapsulating multiple UTXO deltas, in order
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct UTXODeltasMessage {
    /// Block info
    pub block: BlockInfo,

    /// Ordered set of deltas
    pub deltas: Vec<UTXODelta>
}
```


