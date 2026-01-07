# UXTO state

The UTXO state module accepts UTXO changes and maintains an
in-memory UTXO state.  It naively tracks the creation and spending
of UTXOs and logs them.  It doesn't currently have any query interface or
generate any further messages.

Note it does not yet hold enough state to handle rollbacks.

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.utxo-state]

# Message topics
subscribe-topic = "cardano.utxo.deltas"
pool-certificates-deltas-subscribe-topic = "cardano.pool.certificates.deltas"
stake-certificates-deltas-subscribe-topic = "cardano.stake.certificates.deltas"
```

## Messages

The utxo state module subscribes for UTXODeltasMessages `cardano.utxo.deltas`
(see the [Transaction Unpacker](../tx_unpacker) module for details).

It doesn't currently publish any messages.

Also subscribe to `PoolCertificatesDeltas` Messages and `StakeCertificatesDeltas` Messages
in order to validate UTxO Rule `ValueNotConservedUTxO` because we need to 
calculate transaction's `deposit` and `refund` for Pool & Stake Addresses' Registration
