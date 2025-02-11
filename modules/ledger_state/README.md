# Ledger state

The ledger state module accepts UTXO changes and maintains an
in-memory ledger state.  It naively tracks the creation and spending
of UTXOs and logs them.  It doesn't currently have any query interface or
generate any further messages.

Note it does not yet hold enough state to handle rollbacks.

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.ledger-state]

# Message topics
subscribe-topic = "cardano.utxo.deltas"
```

## Messages

The ledger state module subscribes for UTXODeltasMessages `cardano.utxo.deltas`
(see the [Transaction Unpacker](../tx_unpacker) module for details).

It doesn't currently publish any messages.

