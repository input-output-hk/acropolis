#!/usr/bin/env bash
#
# set-topology-partitioned.sh — Apply partitioned topology to the testnet.
#
# Usage: set-topology-partitioned.sh [TESTNET_DIR]
#   TESTNET_DIR  Path to the testnet directory (default: ./testnet)
#
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SOURCE_DIR="$SCRIPT_DIR/../testnet-forked"
TESTNET_DIR="${1:-./testnet}"

if [[ ! -d "$TESTNET_DIR" ]]; then
  echo "Error: testnet directory not found: $TESTNET_DIR" >&2
  exit 1
fi

echo "=== Applying partitioned topology ==="

for pool_dir in "$SOURCE_DIR/pools"/*/; do
  pool_name="$(basename "$pool_dir")"
  src="$pool_dir/configs/topology.json"
  dst="$TESTNET_DIR/pools/$pool_name/configs/topology.json"
  if [[ -f "$src" && -f "$dst" ]]; then
    cp "$src" "$dst"
    echo "  topology: pools/$pool_name"
  fi
done

src_rec="$SOURCE_DIR/recovery/configs/topology.json"
dst_rec="$TESTNET_DIR/recovery/configs/topology.json"
if [[ -f "$src_rec" && -f "$dst_rec" ]]; then
  cp "$src_rec" "$dst_rec"
  echo "  topology: recovery"
fi

echo "Done."
