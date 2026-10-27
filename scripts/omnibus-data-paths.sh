#!/usr/bin/env bash
# Usage: omnibus-data-paths.sh <data-dir> <config>...
#
# Writes <data-dir>/paths.toml, a config overlay that moves the on-disk stores of the
# omnibus under <data-dir>. A `[module.X]` section enables module X, so the overlay only
# covers the modules the given configs already enable.
set -euo pipefail

dir=$1
shift
mkdir -p "$dir"
out="$dir/paths.toml"
: >"$out"

# module, config key, sub-directory
while read -r module key subdir; do
  if grep -q "^\[module\.$module\]" "$@"; then
    printf '[module.%s]\n%s = "%s/%s"\n\n' "$module" "$key" "$dir" "$subdir" >>"$out"
  fi
done <<'EOF'
mithril-snapshot-fetcher directory mithril
chain-store database-path fjall-blocks
utxo-state database-path fjall-immutable-utxos
address-state db-path fjall-addresses
historical-epochs-state db-path fjall-epochs
historical-accounts-state db-path fjall-accounts
accounts-state spdd-db-path fjall-spdd
stake-delta-filter cache-dir cache
EOF
