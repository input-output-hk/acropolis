# Boot Testing Guide

This document describes how to test the Amaru snapshot boot functionality.

## Prerequisites

You need an Amaru/Haskell node snapshot in Conway era (epoch >= 505).

Example snapshot (mainnet epoch 507):
```
tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor
```

Size: ~2.4 GB
UTXOs: 11,199,911
Slot: 134,092,758
Block hash: `670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327`

## Test Scenarios

### 1. Normal Boot (Happy Path)

**Command:**
```bash
cargo run --release -- \
  --snapshot tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor \
  --manifest tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.json
```

**Expected Output:**
```
Status: Starting
Parsing manifest: tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.json
Snapshot info: era=conway, height=134092758, hash=670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327
Era validation: OK (conway)
Status: LoadingSnapshot
Validating integrity...
Integrity check: OK
Loading Amaru/Haskell node snapshot...
Counting UTXOs...
Snapshot loaded: 11199911 UTXOs, tip at slot 134092758 (height ~10661936)
Status: Ready
Boot duration: 5.75s
Node READY
```

**Exit Code:** 0

**Performance:** ~5-6 seconds (release mode)

**Validation:**
- ✅ Status transitions: Starting → LoadingSnapshot → Ready
- ✅ UTXO count: 11,199,911
- ✅ Tip slot: 134,092,758
- ✅ Estimated height: ~10,661,936
- ✅ Boot completes successfully

---

### 2. Boot with Debug Output

**Command:**
```bash
cargo run --release -- \
  --snapshot tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor \
  --manifest tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.json \
  --debug
```

**Expected Output:**
```
Status: Starting
Parsing manifest: tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.json
Snapshot info: era=conway, height=134092758, hash=670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327
Era validation: OK (conway)
Status: LoadingSnapshot
Validating integrity...
Integrity check: OK
Loading Amaru/Haskell node snapshot...
DEBUG: boot_data epoch=507 treasury=1528154947846618 reserves=7816251180544575
DEBUG: tip slot=134092758 hash=670ca68c3de580f8 estimated_height=10661936
Counting UTXOs...
DEBUG: counted 11199911 UTXOs in 1.08s
Snapshot loaded: 11199911 UTXOs, tip at slot 134092758 (height ~10661936)
Status: Ready
Boot duration: 5.64s
Node READY
```

**Exit Code:** 0

**Validation:**
- ✅ DEBUG lines show extracted boot data
- ✅ Epoch: 507 (Conway era)
- ✅ Treasury: 1,528,154,947 ADA
- ✅ Reserves: 7,816,251,180 ADA
- ✅ UTXO counting time: ~1-2 seconds

---

### 3. Error: Tip Hash Mismatch

**Command:**
```bash
# Create manifest with wrong block hash
cat > /tmp/test-wrong-hash.json << 'EOF'
{
    "magic": "CARDANO_SNAPSHOT",
    "version": "1.0.0",
    "era": "conway",
    "block_height": 134092758,
    "block_hash": "0000000000000000000000000000000000000000000000000000000000000000",
    "sha256": "3c01463bb6d95b3ef7fdfb334a7932199b080bbd80647ae2bdd92f76b40a127e",
    "created_at": "2025-10-09T19:41:51Z",
    "size_bytes": 2553095916,
    "governance_section_present": false
}
EOF

cargo run --release -- \
  --snapshot tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor \
  --manifest /tmp/test-wrong-hash.json
```

**Expected Output:**
```
Status: Starting
Parsing manifest: /tmp/test-wrong-hash.json
Snapshot info: era=conway, height=134092758, hash=0000000000000000000000000000000000000000000000000000000000000000
Era validation: OK (conway)
Status: LoadingSnapshot
Validating integrity...
Integrity check: OK
Loading Amaru/Haskell node snapshot...
ERROR: tip hash mismatch: filename has 670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327, manifest has 0000000000000000000000000000000000000000000000000000000000000000
```

**Exit Code:** 1

**Validation:**
- ✅ Error detected before expensive UTXO counting
- ✅ Clear error message explaining the mismatch
- ✅ Shows both expected and actual hashes

---

### 4. Error: File Size Mismatch

**Command:**
```bash
cargo run --release -- \
  --snapshot tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor \
  --manifest tests/fixtures/test-manifest.json
```

**Expected Output:**
```
Status: Starting
Parsing manifest: tests/fixtures/test-manifest.json
Snapshot info: era=conway, height=1000000, hash=11223344aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
Era validation: OK (conway)
Status: LoadingSnapshot
Validating integrity...
ERROR: Decode failed: File size mismatch: manifest says 245 bytes, file is 2553095916 bytes (truncated?)
```

**Exit Code:** 1

**Validation:**
- ✅ Error detected during integrity check
- ✅ Fails before loading expensive snapshot data
- ✅ Clear error message about size mismatch

---

### 5. Error: Wrong Era

**Command:**
```bash
cargo run --release -- \
  --snapshot tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor \
  --manifest tests/fixtures/wrong-era-manifest.json
```

**Expected Output:**
```
Status: Starting
Parsing manifest: tests/fixtures/wrong-era-manifest.json
Snapshot info: era=alonzo, height=134092758, hash=670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327
ERROR: Era mismatch: expected 'conway', got 'alonzo'
```

**Exit Code:** 1

**Validation:**
- ✅ Error detected during era validation
- ✅ Fails immediately, no expensive operations
- ✅ Only Conway era is supported

---

### 6. Error: Missing Arguments

**Command:**
```bash
cargo run --release -- --manifest tests/fixtures/test-manifest.json
```

**Expected Output:**
```
ERROR: --snapshot is required for boot mode
```

**Exit Code:** 1

**Validation:**
- ✅ Clear error message about missing required argument

---

## Integration Tests

### Unit Tests (Fast)

Run all unit tests (< 2 seconds):
```bash
cargo test --lib
```

**Tests:**
- Memory profiler tests (2 tests)
- Amaru snapshot detection
- Conway metadata extraction
- Tip extraction from filename
- Sample UTXO parsing

**Total:** 7 unit tests

---

### Integration Test (Slow)

The full boot integration test is marked `#[ignore]` because it counts 11.2M UTXOs (~77 seconds in debug mode, ~2 seconds in release mode).

Run the ignored test:
```bash
cargo test --test snapshot_boot_test test_boot_with_valid_snapshot -- --ignored --nocapture
```

**Note:** This test requires the large Amaru snapshot file to exist.

---

## Performance Benchmarks

| Operation | Time (release) | Time (debug) |
|-----------|---------------|--------------|
| Parse manifest | < 1ms | < 1ms |
| Validate era | < 1ms | < 1ms |
| Validate integrity (SHA256) | ~500ms | ~1s |
| Extract boot data | ~10ms | ~50ms |
| Extract tip | < 1ms | < 1ms |
| Count UTXOs (11.2M) | ~1-2s | ~30-60s |
| **Total boot time** | **~5-6s** | **~30-60s** |

**Hardware:** Results may vary depending on CPU and disk I/O.

---

## Makefile Targets

Quick testing shortcuts:

```bash
# Run all checks (fmt, clippy, tests)
make all

# Inspect snapshot structure
make amaru-inspect

# Extract metadata
make amaru-metadata

# Extract boot data
make amaru-boot-data

# Extract tip information
make amaru-tip
```

---

## Known Limitations

1. **UTXO Counting is Slow:** Counting 11.2M UTXOs takes 1-2 seconds even in release mode. This is required for accurate state initialization but could be optimized or cached.

2. **No Governance Extraction:** Currently sets `governance_action_count = 0`. Future work should extract actual governance actions from `[3][1][1][3]`.

3. **Estimated Block Height:** Block height is estimated from slot number using empirical ratios. For exact height, query a Cardano node.

4. **No Query State Persistence:** The boot process exits after loading. Query commands return hardcoded data. Future work should keep the state in memory or persist it.

---

## Success Criteria

A successful boot test should verify:

- ✅ Status transitions correctly (Starting → Loading → Ready)
- ✅ Manifest parsed and validated
- ✅ Integrity check passes (SHA256 match)
- ✅ Tip extracted from filename matches manifest
- ✅ Boot data extracted (epoch, treasury, reserves)
- ✅ UTXO count is correct (11,199,911 for epoch 507 snapshot)
- ✅ Boot completes in reasonable time (~5-6s release mode)
- ✅ Final status is Ready
- ✅ Node prints "Node READY"
- ✅ Exit code is 0

---

## Troubleshooting

### Boot takes too long (> 10 seconds)

**Cause:** Debug build is much slower for UTXO counting.

**Solution:** Use `--release` flag:
```bash
cargo run --release -- --snapshot <path> --manifest <path>
```

### ERROR: Decode failed: expected at least 4 top-level elements

**Cause:** Trying to boot from a synthetic snapshot (not Amaru format).

**Solution:** Only Amaru/Haskell node snapshots are supported for boot. Synthetic snapshots are for testing stream parsing only.

### ERROR: File not found

**Cause:** Snapshot file doesn't exist at the specified path.

**Solution:** Verify the path is correct:
```bash
ls -lh tests/fixtures/134092758.670ca68c3de580f8469677754a725e86ca72a7be381d3108569f0704a5fca327.cbor
```

### Integration test hangs for a long time

**Cause:** Debug mode is very slow for counting 11.2M UTXOs.

**Solution:** This is expected. The test takes ~77 seconds in debug mode. Use `--release` for faster execution, or skip the test (it's marked `#[ignore]` by default).
