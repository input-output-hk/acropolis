# Amaru Snapshot Quick Start



## What are Amaru Snapshots?

Amaru snapshots are CBOR dumps from Cardano Haskell node's `GetCBOR` ledger-state query. They represent the internal `EpochState` type and contain the complete ledger state at a specific point on the blockchain.

Naming convention: `<slot>.<block_hash>.cbor`

Example: `134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor`
- Slot: 134092758
- Block hash: 670ca68c...

## Quick Commands

### 1. Generate Manifest (auto-derives metadata from filename)

```bash
make tests/fixtures/<slot>.<hash>.json ERA=conway
```

This:
- Computes SHA256 in streaming mode (constant memory)
- Extracts slot and hash from filename
- Creates manifest JSON for integrity validation

### 2. Inspect Structure (reads only first 256KB)

```bash
# Using Makefile (default snapshot)
make amaru-inspect

# Or specify a file
cargo run --release -- inspect --snapshot path/to/snapshot.cbor
```

Output shows:
- Top-level CBOR structure
- Element types and values
- Likely field names (for known structures)
- Memory usage: ~2MB

### 3. Test Format Detection

```bash
make amaru-info
```

Attempts to boot from the snapshot and shows:
- Format detection working correctly
- Enhanced error message with diagnostic info
- Confirms integrity check passes

## Example Session

```bash
# 1. You have an Amaru snapshot
ls -lh tests/fixtures/134092758.*.cbor
# -rw-r--r-- 2.4G Oct 9 11:56 134092758.670ca68c...cbor

# 2. Generate manifest
make tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.json ERA=conway
# Generating manifest... (0.95s, ~48MB RAM)
# ✓ SHA256: 3c01463bb6d95b3ef7fdfb334a7932199b080bbd80647ae2bdd92f76b40a127e
# ✓ Size: 2553095916 bytes
# ✓ Slot: 134092758 (from filename)
# ✓ Hash: 670ca68c... (from filename)

# 3. Inspect structure
make amaru-inspect
# === Amaru EpochState Structure Inspection ===
# Top-level: Array with 7 elements
# [0] = AccountState
# [1] = LedgerState (UTXOs, delegations, etc.)
# [2] = SnapShots
# ...

# 4. Test integrity and format detection
make amaru-info
# Status: Starting
# Parsing manifest: ...
# Era validation: OK (conway)
# Integrity check: OK
# ERROR: unsupported snapshot format...
# (This is expected - full parsing not yet implemented)
```

## Current Capabilities

✅ **Working:**
- Manifest generation with auto-metadata extraction
- SHA256 computation (streaming, constant memory)
- Format detection (Amaru vs synthetic)
- Structure inspection (first 256KB)
- Integrity validation (hash + size)

🚧 **Not Yet Implemented:**
- Full EpochState parsing
- UTXO extraction
- Governance state extraction
- Delegation/stake pool data
- Protocol parameters

## Memory Usage

All operations use constant memory:

| Operation | File Size | Memory Used | Time |
|-----------|-----------|-------------|------|
| Generate manifest | 2.4 GB | ~48 MB | 0.95s |
| Inspect structure | 2.4 GB | ~2 MB | 1.0s |
| Integrity check | 2.4 GB | ~18 MB | ~5s |

## Next Steps

To enable full Amaru support, we need to:

1. Map the 7-element `EpochState` structure to Rust types
2. Implement parsers for nested CBOR (LedgerState, UTXOs, etc.)
3. Pin to a specific cardano-ledger version for stability
4. Add streaming parser to avoid loading 2+ GB into memory
5. Extract high-value fields (tip, UTXO count, stake distribution)

See `docs/snapshot-formats.md` for detailed format documentation.
