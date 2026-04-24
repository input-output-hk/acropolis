#!/usr/bin/env bash

set -euo pipefail

echo "Pool 2 status (node 11):"

docker run --rm \
-v "./testnet-forked/dbs/pool11:/ipc" \
ghcr.io/intersectmbo/cardano-node:10.6.2 \
cli query tip \
--testnet-magic 42 \
--socket-path /ipc/node.socket
