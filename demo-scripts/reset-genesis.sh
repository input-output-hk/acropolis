#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

TS=$(date -u -v+2M +%s)
ISO=$(date -u -r "$TS" +"%Y-%m-%dT%H:%M:%SZ")

echo "Updating genesis time"
echo "startTime  = $TS"
echo "systemStart= $ISO"

find "$SCRIPT_DIR/../testnet" \
  \( -name byron-genesis.json -o -name shelley-genesis.json \) | while read -r f; do
  if [[ "$f" == *byron-genesis.json ]]; then
    perl -0pi -e "s/\"startTime\": \\d+/\"startTime\": $TS/" "$f"
    echo "updated byron: $f"
  else
    perl -0pi -e "s/\"systemStart\": \"[^\"]+\"/\"systemStart\": \"$ISO\"/" "$f"
    echo "updated shelley: $f"
  fi
done

echo "=== Clearing DBs ==="
find "$SCRIPT_DIR/../testnet/dbs" -mindepth 1 -maxdepth 1 -type d \
  -exec sh -c 'rm -rf "$1"/* "$1"/.[!.]* "$1"/..?* 2>/dev/null || true' _ {} \;
echo "DBs cleared."

echo "=== Copying genesis files ==="
cp "$SCRIPT_DIR/../genesis-downloads/genesis_bootstrapper/"* \
   "$SCRIPT_DIR/../modules/genesis_bootstrapper/downloads/"
cp "$SCRIPT_DIR/../genesis-downloads/parameters_state/"* \
   "$SCRIPT_DIR/../modules/parameters_state/downloads/"
echo "Genesis files copied."

echo
echo "Done."
