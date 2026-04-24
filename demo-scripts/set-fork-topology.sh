#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo "=== Stopping cluster ==="
"$SCRIPT_DIR/stop-cluster.sh"

echo "=== Applying partitioned topology ==="
"$SCRIPT_DIR/set-topology-partitioned.sh"

echo "=== Starting cluster ==="
"$SCRIPT_DIR/start-cluster.sh"
