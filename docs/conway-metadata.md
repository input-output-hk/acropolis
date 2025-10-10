# Conway+ Era Metadata Extraction

## Overview

The `metadata` command extracts high-level metadata from Amaru/Haskell node snapshots **without loading the entire file into memory**. This is essential for multi-gigabyte snapshots that contain millions of UTXOs.

## Supported Eras

**Conway and forward only** (epoch >= 505). Pre-Conway eras are not supported:
- ✅ Conway era (epoch 505+)
- ✅ Future eras (when they arrive)
- ❌ Babbage (epochs ~358-504)
- ❌ Alonzo and earlier

## Usage

### Basic Extraction

```bash
cargo run --release -- metadata --snapshot path/to/snapshot.cbor
```

### Via Makefile

```bash
make amaru-metadata
```

## Example Output

```
Extracting metadata from: tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor

Conway era snapshot: epoch 507, 2553095916 bytes

Details:
  Epoch: 507
  File size: 2553095916 bytes (2.55 GB)
  UTXO count: (not yet computed - requires full file scan)
```

## What Gets Extracted

### Currently Implemented (Phase 1)

1. **Epoch number** - Validates >= 505 (Conway era)
2. **File size** - Total bytes on disk
3. **Structure validation** - Confirms 7-element EpochState array

### Future Implementation

1. **UTXO count** - Streaming counter through LedgerState map
2. **Tip information** - Block height and hash
3. **Delegation count** - Number of stake delegations
4. **Governance counts** - Active proposals, votes, etc.

## Memory Profile

Metadata extraction uses **constant memory** regardless of file size:

- **~2.5 MB RSS** - For parsing first 256KB of structure
- **No full deserialization** - Only top-level array and epoch
- **Fast** - Completes in <1 second even for multi-GB files

## Implementation Details

### EpochState Structure (Conway)

The Amaru snapshot format is a 7-element CBOR array:

```
[0] = Epoch number (u64) — e.g., 507
[1] = LedgerState (indefinite map: UTXOs, delegations, governance)
[2] = SnapShots (28-byte hash)
[3] = Counter (u64)
[4] = Nonce (28-byte hash)
[5] = Counter (u64)
[6] = Hash (28-byte hash)
```

### Parsing Strategy

**Phase 1 (Current)**: Read first 256KB only
- Parse Element [0] for epoch
- Validate Element [1] is a map (LedgerState)
- Return metadata without parsing Element [1] contents

**Phase 2 (Future)**: Streaming LedgerState scan
- Stream through Element [1] indefinite map
- Count entries (UTXOs, delegations) without deserializing values
- Extract tip from map entry
- Keep memory constant (~16MB buffer)

## Error Handling

### Pre-Conway Rejection

```bash
$ cargo run -- metadata --snapshot old-babbage-snapshot.cbor
ERROR: epoch 450 is pre-Conway (requires >= 505)
```

### Invalid Structure

```bash
$ cargo run -- metadata --snapshot malformed.cbor
ERROR: expected 7-element EpochState array, got 5
```

## Integration with Boot

The boot process can use metadata extraction to:

1. **Validate era** before attempting full parse
2. **Estimate resources** based on UTXO count
3. **Show progress** during initial load

Example:

```bash
cargo run -- boot --snapshot large.cbor --manifest large.json --debug
```

In debug mode, boot will:
- Extract metadata first (epoch, file size)
- Log validation steps
- Show memory usage during load

## Next Steps

### Implement UTXO Counting (Phase 2)

Add streaming counter to `EpochStateMetadata::from_file()`:

```rust
pub fn from_file_with_counts(path: &str) -> Result<Self, SnapshotError> {
    // ... existing epoch parsing ...
    
    // Stream through LedgerState map (Element [1])
    let utxo_count = count_map_entries(&mut decoder)?;
    
    Ok(EpochStateMetadata {
        epoch,
        utxo_count: Some(utxo_count),
        file_size,
    })
}
```

### Add Targeted Queries (Phase 3)

Enable UTXO lookup by TxIn without loading full set:

```bash
cargo run -- query-utxo \
  --snapshot snapshot.cbor \
  --tx-hash 670ca68c3de580f8... \
  --output-index 0
```

## References

- [EpochState Haskell definition](https://github.com/IntersectMBO/cardano-ledger/blob/33e90ea03447b44a389985ca2b158568e5f4ad65/eras/shelley/impl/src/Cardano/Ledger/Shelley/LedgerState/Types.hs#L121-L131)
- [Amaru quick-start guide](./amaru-quick-start.md)
- [Snapshot formats documentation](./snapshot-formats.md)
