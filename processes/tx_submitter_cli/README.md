# Acropolis tx-submitter-cli tool

This process is a CLI wrapper for the [tx_submitter module](../../modules/tx_submitter/). It allows you to submit transactions to upstream peers.

## How to run it

```shell
cd processes/tx_submitter_cli
cargo run -- <tx-file>
```
The `tx-file` arg should be the path to a file containing a raw signed transaction.
