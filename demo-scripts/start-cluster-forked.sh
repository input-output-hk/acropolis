#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "Starting forked test net cluster of 30 nodes"

find ./testnet-forked/dbs -name node.socket -delete
TESTNET_DIR=testnet-forked docker compose -f "$SCRIPT_DIR/../docker-compose.testnet-30.yml" up -d
