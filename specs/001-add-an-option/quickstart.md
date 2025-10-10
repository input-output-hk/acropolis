# Quickstart: Parse & Display Amaru Snapshot (Conway+)

## Prerequisites
- Conway-era snapshot CBOR in `tests/fixtures/`
- Manifest script available to cross-check counts

## Summary View
- Run the CLI with the snapshot path to display epoch, era, counts, param digest

## Sections
- Use flags to display only protocol parameters, governance, pools, accounts, or UTxO

## Bootstrap
- Invoke bootstrap with snapshot path; observe per-module dispatch and acknowledgments
- Expect timeouts (5s/module) if a module is not ready

## Troubleshooting
- Pre-Conway snapshots: expected unsupported-era message
- Corrupt snapshots: check error for first failing section
- Large files: ensure progress updates at least once per second; stall warning >2s
