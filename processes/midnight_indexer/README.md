# Acropolis 'midnight_indexer' process

This process contains the core ledger state modules along with the
`midnight_state` module which indexes Midnight relevant state. This 
state is then accessible via a gRPC server for communication with a
`midnight_node` instance.

## How to run it

Mainnet:
```shell
$ make run-midnight-mainnet
```

Preview:
```shell
$ make run-midnight-preview
```

## Docker Compose

Build and run preview:

```shell
docker compose up --build midnight-indexer-preview
```

Build and run mainnet:

```shell
docker compose up --build midnight-indexer-mainnet
```
