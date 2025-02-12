# Genesis bootstrapper module

The genesis bootstrapper module reads chain genesis data and outputs UTXO events based on them.

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.genesis-bootstrapper]

# Message topics
publish-utxo-deltas-topic = "cardano.utxo.deltas"

```

## Messages

The genesis bootstrapper sends UTXODeltasMessage on `cardano.utxo.deltas` - see
the [Tx Unpacker](../tx_unpacker) messages for details.  The deltas are only ever
TxOutputs, of course.
