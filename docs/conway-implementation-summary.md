# Conway+ Metadata Extraction - Implementation Summary

## What Was Implemented

### New Types

1. **`EpochStateMetadata`** - Struct containing Conway+ era metadata:
   - `epoch: u64` - Epoch number (validated >= 505)
   - `utxo_count: Option<u64>` - UTXO count (future implementation)
   - `file_size: u64` - Snapshot file size in bytes

### New Functions

1. **`EpochStateMetadata::from_file(path)`** - Parse metadata from snapshot:
   - Reads only first 256KB (constant memory)
   - Validates 7-element EpochState array
   - Extracts epoch number from Element [0]
   - Validates epoch >= 505 (rejects pre-Conway)
   - Validates Element [1] is a map (LedgerState)

2. **`EpochStateMetadata::summary()`** - Human-readable summary

### CLI Integration

New `metadata` subcommand:
```bash
cargo run -- metadata --snapshot path/to/snapshot.cbor
```

Makefile target:
```bash
make amaru-metadata
```

### Error Handling

- **Pre-Conway rejection**: `epoch {N} is pre-Conway (requires >= 505)`
- **Structure validation**: `expected 7-element EpochState array, got {N}`
- **Element type validation**: `Element [1] should be LedgerState map`

## Memory Profile

- **~2.5 MB RSS** for metadata extraction
- **No full file load** - Only first 256KB read
- **Fast** - Completes in <1 second even for 2.5GB files

## Test Coverage

Added `test_conway_metadata_extraction`:
- Validates epoch >= 505
- Confirms epoch 507 for test snapshot
- Validates file size > 2GB
- All tests pass (24 tests total)

## Example Output

```
Extracting metadata from: tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor

Conway era snapshot: epoch 507, 2553095916 bytes

Details:
  Epoch: 507
  File size: 2553095916 bytes (2.55 GB)
  UTXO count: (not yet computed - requires full file scan)
```

## Conway-Forward Focus

**Supported**: Conway era (epoch >= 505) and forward
**Not supported**: Babbage, Alonzo, and earlier eras

This simplifies implementation by:
- Skipping legacy era handling
- Focusing on modern Conway ledger structure
- Assuming CIP-1694 governance features
- Supporting inline datums and reference scripts

## Next Steps (Phase 2)

1. **UTXO Count** - Stream through LedgerState map (Element [1]):
   ```rust
   pub fn from_file_with_counts(path: &str) -> Result<Self, SnapshotError>
   ```
   - Count map entries without deserializing values
   - Keep memory constant (~16MB buffer)
   - Return populated `utxo_count: Some(N)`

2. **Tip Extraction** - Parse tip from LedgerState:
   ```rust
   pub struct EpochStateMetadata {
       pub tip: Option<BlockInfo>,  // Block height, hash
       // ... existing fields ...
   }
   ```

3. **Delegation Count** - Count stake delegations

4. **Governance Counts** - Active proposals, votes, DReps

## Next Steps (Phase 3)

**Targeted UTXO Queries** - Look up specific UTXOs without loading full set:
```bash
cargo run -- query-utxo --snapshot snapshot.cbor --tx-hash <hash> --index 0
```

Implementation:
- Stream through LedgerState map looking for matching key
- Parse only the matching entry
- Return `TxOut` or `None`
- Constant memory regardless of UTXO set size

## Files Modified

- `src/snapshot/amaru.rs` - Added `EpochStateMetadata` and parsing logic
- `src/main.rs` - Added `metadata` subcommand and handler
- `Makefile` - Added `amaru-metadata` target
- `docs/conway-metadata.md` - Complete usage documentation

## Documentation

- [Conway Metadata Extraction Guide](./conway-metadata.md)
- [Amaru Quick-Start](./amaru-quick-start.md)
- [Snapshot Formats](./snapshot-formats.md)

## Verification

```bash
make all          # All tests pass, clippy clean, formatted
make amaru-metadata  # Extract metadata from 2.5GB snapshot
```

All 24 tests pass ✅
Clippy clean ✅
Formatted ✅
