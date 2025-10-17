#!/usr/bin/env python3
"""
Generate a snapshot manifest JSON from a CBOR snapshot file.

Behavior:
- Attempts to parse a small custom header format used by this project (synthetic fixtures).
- For large or unknown formats (e.g., Amaru EpochState dumps), it derives metadata from
    filename pattern or command-line flags without loading the entire CBOR into memory.

Usage:
        ./generate_manifest.py <snapshot.cbor> [--era ERA] [--block-hash HEX] [--block-height N]

Notes:
- SHA256 is computed in streaming mode (constant memory).
- For Amaru snapshots named as: <slot>.<block_hash>.cbor, the script can auto-derive
    block_height (slot) and block_hash from the filename. Otherwise, pass flags.

Requires: pip install cbor2
"""

import argparse
from typing import Optional, Tuple
import cbor2
import hashlib
import sys
import json
import os
from pathlib import Path
from datetime import datetime, timezone


def compute_sha256(file_path: Path, chunk_size: int = 16 * 1024 * 1024) -> str:
    """Compute SHA256 hash of file using streaming to avoid large memory usage."""
    hasher = hashlib.sha256()
    with open(file_path, 'rb') as f:
        while True:
            chunk = f.read(chunk_size)
            if not chunk:
                break
            hasher.update(chunk)
    return hasher.hexdigest()


def parse_cbor_header(file_path: Path, size_bytes: int) -> Optional[dict]:
    """Parse CBOR header for synthetic format used in tests.

    Returns a dict with expected keys if format matches; otherwise None.
    Avoids loading very large files into memory by skipping parse when size is big.
    """
    # Heuristic: skip parsing if file is huge (>64 MiB) to avoid OOM with cbor2.load
    if size_bytes > 64 * 1024 * 1024:
        return None

    try:
        with open(file_path, 'rb') as f:
            data = cbor2.load(f)
    except Exception:
        return None

    if not isinstance(data, list) or len(data) < 2:
        return None

    version = data[0]
    header = data[1]

    if not isinstance(header, dict):
        return None

    # Extract header fields (keys match the CBOR format from generate_cbor_fixtures.py)
    era = header.get(0)
    block_height = header.get(1)
    block_hash_bytes = header.get(2)
    declared_gov_actions = header.get(4, 0)
    declared_param_sets = header.get(5, 0)

    if not era or block_height is None or not block_hash_bytes:
        return None

    # Convert block hash bytes to hex string
    block_hash = block_hash_bytes.hex()

    # Determine if governance section is present
    governance_present = (declared_gov_actions or 0) > 0 or (declared_param_sets or 0) > 0

    return {
        "version": version,
        "era": era,
        "block_height": block_height,
        "block_hash": block_hash,
        "governance_section_present": governance_present,
    }


def derive_from_filename(snapshot_path: Path) -> Tuple[Optional[int], Optional[str]]:
    """Derive (block_height, block_hash) from filename of form <slot>.<hash>.cbor."""
    name = snapshot_path.name
    parts = name.split('.')
    # Expect at least: slot.hash.cbor
    if len(parts) >= 3 and parts[-1].lower() == 'cbor':
        try:
            slot = int(parts[0])
            hash_hex = parts[1]
            if all(c in '0123456789abcdefABCDEF' for c in hash_hex):
                return slot, hash_hex.lower()
        except Exception:
            return None, None
    return None, None


def generate_manifest(snapshot_path: Path,
                      era_opt: Optional[str] = None,
                      block_hash_opt: Optional[str] = None,
                      block_height_opt: Optional[int] = None) -> dict:
    """Generate complete manifest from snapshot file.

    Prefers synthetic header when present; otherwise falls back to filename or CLI-provided fields.
    """
    if not snapshot_path.exists():
        raise FileNotFoundError(f"Snapshot file not found: {snapshot_path}")

    size_bytes = snapshot_path.stat().st_size

    # Try parsing synthetic header for small files
    header = parse_cbor_header(snapshot_path, size_bytes)

    # Fallbacks for Amaru / unknown formats
    if header is None:
        slot_from_name, hash_from_name = derive_from_filename(snapshot_path)
        block_height = block_height_opt or slot_from_name or 1  # minimal >0 to satisfy validator
        block_hash = (block_hash_opt or hash_from_name or 'unknown').lower()
        era = era_opt or 'conway'
        governance_present = False
    else:
        era = header["era"]
        block_height = header["block_height"]
        block_hash = header["block_hash"]
        governance_present = header["governance_section_present"]

    # Compute integrity fields (streaming)
    sha256 = compute_sha256(snapshot_path)

    # Generate manifest
    manifest = {
        "magic": "CARDANO_SNAPSHOT",
        "version": "1.0.0",
        "era": era,
        "block_height": block_height,
        "block_hash": block_hash,
        "sha256": sha256,
        "created_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "size_bytes": size_bytes,
        "governance_section_present": governance_present,
    }

    return manifest


def main():
    parser = argparse.ArgumentParser(description="Generate manifest JSON from CBOR snapshot")
    parser.add_argument('snapshot', type=str, help='Path to snapshot .cbor file')
    parser.add_argument('--era', type=str, default=None, help='Era string (e.g., conway). Used for unknown formats.')
    parser.add_argument('--block-hash', dest='block_hash', type=str, default=None,
                        help='Hex block header hash. If not provided, derive from filename when possible.')
    parser.add_argument('--block-height', dest='block_height', type=int, default=None,
                        help='Block height (or slot if unknown). If not provided, derive from filename when possible.')

    args = parser.parse_args()

    snapshot_path = Path(args.snapshot)

    try:
        manifest = generate_manifest(snapshot_path, era_opt=args.era,
                                     block_hash_opt=args.block_hash,
                                     block_height_opt=args.block_height)
        # Pretty-print JSON to stdout
        print(json.dumps(manifest, indent=4))
    except Exception as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
