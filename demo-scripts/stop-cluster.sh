#!/usr/bin/env bash

set -euo pipefail

echo "Stopping test net cluster of 30 nodes"

docker compose -f ./docker-compose.testnet-30.yml down
