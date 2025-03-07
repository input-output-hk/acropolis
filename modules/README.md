# Microservice modules for Acropolis

This directory holds microservice modules for a Caryatid framework which
compose the Acropolis Architecture

* [Upstream Chain Fetcher](upstream_chain_fetcher) -
  implementation of the Node-to-Node (N2N) client-side (initiator)
  protocol, allowing chain synchronisation and block fetching
* [Mithril Snapshot Fetcher](mithril_snapshot_fetcher) -
  Fetches a chain snapshot from Mithril and replays all the blocks in it
* [Genesis Bootstrapper](genesis_bootstrapper) - reads the Genesis
  file for a chain and generates initial UTXOs
* [Block Unpacker](block_unpacker) - unpacks received blocks
  into individual transactions
* [Tx Unpacker](tx_unpacker) - parses transactions and generates UTXO
  changes
* [UTXO State](utxo_state) - watches UTXO changes and maintains a basic in-memory UTXO state
