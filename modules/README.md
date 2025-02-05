# Microservice modules for Acropolis

This directory holds microservice modules for a Caryatid framework which
compose the Acropolis Architecture

* [Mini-protocols](miniprotocols) - implementation of the
  Node-to-Node (N2N) client-side (initiator) protocol, allowing chain
  synchronisation and block fetching
* [Block Unpacker](block_unpacker) - unpacks received blocks into
  individual transactions
* [Tx Unpacker](tx_unpacker) - parses transactions and generates UTXO
  changes
* [Ledger State](ledger_state) - watches UTXO changes and maintains a
  basic in-memory ledger state
