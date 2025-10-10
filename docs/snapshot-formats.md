# Snapshot Formats

This document describes the two snapshot formats supported by spec-test.

## 1. Synthetic Test Format

Our internal test format for controlled, reproducible testing.

### Structure

```
CBOR: [version, header_map, items_array]
```

- `version` (u64): Format version, currently 1
- `header_map` (map): Metadata with integer keys
  - `0`: era (string) - "conway"
  - `1`: block_height (u64)
  - `2`: block_hash (bytes, 32)
  - `3`: declared_utxos (optional u64)
  - `4`: declared_gov_actions (optional u64)
  - `5`: declared_param_sets (optional u64)
- `items_array` (array): Ledger messages with tagged tuples
  - `[0, tx_hash, index, address, value]` - UTXO entry
  - `[1, count_delta]` - Governance actions
  - `[2, height, hash]` - Tip update
  - `[3, [[key, val], ...]]` - Parameter set
  - `[4]` - End of snapshot marker

### Status

✅ **Fully supported** - parsing, validation, streaming, and state updates all work.

### Usage

```bash
# Generate synthetic fixtures
python3 scripts/generate_cbor_fixtures.py

# Generate manifest
make tests/fixtures/snapshot-small.json

# Boot with synthetic snapshot
cargo run -- --snapshot tests/fixtures/snapshot-small.cbor \
             --manifest tests/fixtures/test-manifest.json
```

## 2. Amaru/Haskell Node Format

Real Cardano ledger state dumps from the Haskell node's `GetCBOR` query.

### Structure

```
CBOR: [top-level array representing the full snapshot]
```

This format is based on the internal Haskell `EpochState` type from:
https://github.com/IntersectMBO/cardano-ledger/blob/33e90ea03447b44a389985ca2b158568e5f4ad65/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState/Types.hs#L121-L131

**Note**: This format is not formally specified and may change across Haskell ledger updates.

**📖 See [Amaru Snapshot Structure Specification](./amaru-snapshot-structure.md) for complete structural details**, including:
- Full array hierarchy and navigation paths
- UTXO location: `[3][1][1][0]` (Epoch State → Ledger State → UTxO State → utxo map)
- Treasury/reserves, governance, protocol parameters locations
- Conway-era specific features (CIP-1694)
- Code examples for extracting specific data

### Naming Convention

Amaru snapshots are named: `<slot>.<block_hash>.cbor`

Example:
```
134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor
```

Where:
- `134092758` = absolute slot number
- `670ca68c...` = 64-character hex-encoded block header hash

### Status

🚧 **Partial support** - the tool can:
- ✅ Detect Amaru format vs synthetic format
- ✅ Auto-derive slot/hash from filename for manifest
- ✅ Compute SHA256 and size in streaming mode (constant memory)
- ✅ Display diagnostic info (file size, top-level structure)
- ✅ Extract epoch number (Conway+ validation, epoch >= 505)
- ✅ **Count UTXOs** (~11.2M on mainnet epoch 507, ~2s scan time)
- ✅ Navigate to nested structures (documented paths)
- ✅ **Extract boot data** (epoch, treasury, reserves from AccountState)
- ✅ **Parse individual UTXOs** (callback pattern for memory efficiency)
- ❌ Not yet: Full streaming boot from Amaru snapshot
- ❌ Not yet: Extract governance actions, protocol parameters

### Usage

```bash
# Generate manifest (auto-derives slot and hash from filename)
make tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.json ERA=conway

# Extract metadata (fast, reads only first 256KB)
make amaru-metadata
# Output: Conway era snapshot: epoch 507, 2553095916 bytes

# Extract metadata with UTXO count (slow, scans entire file)
cargo run --release -- metadata --snapshot snapshot.cbor --with-counts
# Output: Conway era snapshot: epoch 507, 2553095916 bytes, ~11199911 UTXOs
# Time: ~2 seconds for 2.5GB file

# Extract boot data (epoch, treasury, reserves)
make amaru-boot-data
# Output:
#   Epoch:     507
#   Treasury:  1528154947 ADA (1528154947846618 lovelace)
#   Reserves:  7816251180 ADA (7816251180544575 lovelace)
#   File Size: 2553095916 bytes (2.38 GB)

# Inspect CBOR structure (reads first 256KB)
make amaru-inspect

# Test format detection (will show diagnostic info)
make amaru-info
```

## Future Work

To add full Amaru support:

1. **Map EpochState structure** - Document the 7-element array layout
2. **Implement parser** - Extract UTXOs, tip, governance state from nested CBOR
3. **Pin ledger version** - Lock to specific cardano-ledger commit for stability
4. **Streaming parse** - Avoid loading 2+ GB files into memory
5. **Test coverage** - Validate against real mainnet/testnet snapshots

## Choosing a Format

- **Synthetic format**: For tests, CI, controlled scenarios
- **Amaru format**: For bootstrapping from real Cardano network state (when parser is complete)

## Inspecting Amaru Snapshots

You can now inspect the CBOR structure of Amaru snapshots without loading the entire file:

```bash
# Quick inspection (reads only first 256KB)
make amaru-inspect

# Or with a specific file
cargo run --release -- inspect --snapshot path/to/snapshot.cbor
```

This shows:
- Top-level structure (array/map with element count)
- Types and values of each element (up to 3 levels deep)
- Hex preview for byte strings
- Size information

Example output for a 2.4GB mainnet snapshot:
```
=== Amaru EpochState Structure Inspection ===

Top-level: Array with 7 elements

Likely EpochState structure (7 elements):
  [0] = AccountState
  [1] = LedgerState (UTXOs, delegations, etc.)
  [2] = SnapShots
  [3] = NonMyopic
  ...

Element 0:
  Unsigned: 507
Element 1:
  Map {indefinite}:
...
```

Memory usage: ~2MB regardless of snapshot size.
