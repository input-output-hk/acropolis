# Epochs state module

The epoch activity counter module accepts fee messages from the
[TxUnpacker](../tx_unpacker) and totals up the fees on every
transaction in every block across an epoch.  It also subscribes for
block headers and records the KES keys for every block in the epoch,
and sends a report at the end of the epoch that can be used by the
reward calculator to allocate rewards to SPOs and thence to
delegators.

## Configuration

The following is the default configuration - if the defaults are OK,
everything except the section header can be left out.

```toml
[module.epochs-state]

# Message subscription topics
snapshot-subscribe-topic = "cardano.snapshot"
block-subscribe-topic = "cardano.block.proposed"
block-txs-subscribe-topic = "cardano.block.txs"
protocol-parameters-subscribe-topic = "cardano.protocol.parameters"
bootstrapped-subscribe-topic = "cardano.sequence.bootstrapped"

# Message publishing topics
epoch-activity-publish-topic = "cardano.epoch.activity"
epoch-nonce-publish-topic = "cardano.epoch.nonce"

# Validation topic
validation-publish-topic = "cardano.validation.epochs"

# Epochs state query
epochs-state-query-topic = "cardano.query.epochs"
```

## Messages

The epochs state subscribes for RawBlockMessages on
`cardano.block.proposed` (see the [Consensus](../consensus) module for details).

TODO all remaining definitions
