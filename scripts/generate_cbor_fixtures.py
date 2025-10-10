#!/usr/bin/env python3
"""
Generate synthetic CBOR snapshot fixtures for testing.
Requires: pip install cbor2
"""

import cbor2
import hashlib
from pathlib import Path

def write_cbor_fixture(path, data):
    """Write CBOR-encoded data to file."""
    Path(path).parent.mkdir(parents=True, exist_ok=True)
    with open(path, 'wb') as f:
        cbor2.dump(data, f)
    print(f"✓ Generated {path}")

def main():
    fixtures_dir = Path("tests/fixtures")
    
    # Common header
    block_hash = b'\x11\x22\x33\x44' + b'\xAA' * 28  # 32 bytes
    
    # 1. Valid snapshot (snapshot-small.cbor)
    valid_snapshot = [
        1,  # version
        {   # header map
            0: "conway",
            1: 1000000,  # height
            2: block_hash,
            3: 2,  # declared_utxos
            4: 3,  # declared_gov_actions
            5: 1,  # declared_param_sets
        },
        [   # items array
            [0, b'\xFF' * 32, 0, "addr_test1xyz", 123456789],  # UTXO 1
            [0, b'\x00' * 32, 1, "addr_test1pqr", 42],          # UTXO 2
            [2, 1000000, block_hash],                            # TipUpdate
            [1, 3],                                               # GovernanceActions (delta=3)
            [3, [["minFeeA", "44"], ["minFeeB", "155381"]]],    # ParameterSet
            [4],                                                  # EndOfSnapshot
        ]
    ]
    write_cbor_fixture(fixtures_dir / "snapshot-small.cbor", valid_snapshot)
    
    # 2. Missing end marker (snapshot-missing-end.cbor)
    missing_end = [
        1,
        {0: "conway", 1: 1000000, 2: block_hash, 3: 1, 4: 0, 5: 0},
        [
            [0, b'\xFF' * 32, 0, "addr_test1xyz", 123456789],
            # Missing [4] EndOfSnapshot
        ]
    ]
    write_cbor_fixture(fixtures_dir / "snapshot-missing-end.cbor", missing_end)
    
    # 3. Count mismatch (snapshot-count-mismatch.cbor)
    count_mismatch = [
        1,
        {0: "conway", 1: 1000000, 2: block_hash, 3: 5, 4: 0, 5: 0},  # declares 5 UTXOs
        [
            [0, b'\xFF' * 32, 0, "addr_test1xyz", 123456789],  # only 1 UTXO
            [4],  # EndOfSnapshot
        ]
    ]
    write_cbor_fixture(fixtures_dir / "snapshot-count-mismatch.cbor", count_mismatch)
    
    # 4. Duplicate end marker (snapshot-duplicate-end.cbor)
    duplicate_end = [
        1,
        {0: "conway", 1: 1000000, 2: block_hash, 3: 1, 4: 0, 5: 0},
        [
            [0, b'\xFF' * 32, 0, "addr_test1xyz", 123456789],
            [4],  # First EndOfSnapshot
            [4],  # Duplicate!
        ]
    ]
    write_cbor_fixture(fixtures_dir / "snapshot-duplicate-end.cbor", duplicate_end)
    
    # 5. Wrong era (snapshot-wrong-era.cbor)
    wrong_era = [
        1,
        {0: "byron", 1: 1000000, 2: block_hash, 3: 0, 4: 0, 5: 0},  # wrong era
        [
            [4],  # EndOfSnapshot
        ]
    ]
    write_cbor_fixture(fixtures_dir / "snapshot-wrong-era.cbor", wrong_era)
    
    print("\n✓ All CBOR fixtures generated successfully")
    print("\nTo regenerate manifests from CBOR files:")
    print("  make tests/fixtures/snapshot-small.json")
    print("  make tests/fixtures/snapshot-wrong-era.json")
    print("\nRun tests with: cargo test snapshot_stream")

if __name__ == "__main__":
    main()
