#!/usr/bin/env bash

set -euo pipefail

echo "Pool 1 status (node 1):"

docker run --rm \
-v "./testnet/dbs/pool1:/ipc" \
ghcr.io/intersectmbo/cardano-node:10.6.2 \
cli query tip \
--testnet-magic 42 \
--socket-path /ipc/node.socket
