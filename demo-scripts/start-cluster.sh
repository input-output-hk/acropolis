#!/usr/bin/env bash

set -euo pipefail

echo "Starting test net cluster of 30 nodes"

find ./testnet/dbs -name node.socket -delete
docker compose -f ./docker-compose.testnet-30.yml up -d
