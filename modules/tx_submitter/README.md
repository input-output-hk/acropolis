# TX submission module

The TX submission module implements the TXSubmission node-to-node protocol to submit transactions to a single upstream source.

## Messages

The TX submission module listens for requests to submit transactions on the `cardano.txs.submit` topic. It will send a response once any upstream server has acknowledged the transaction.

## Default configuration

```toml
[module.tx-submitter]

# Upstream node connection
node-address = "backbone.cardano.iog.io:3001"
magic-number = 764824073

# Message topics
subscribe-topic = "cardano.txs.submit"

```