# Contracts: Operator CLI (Human-Readable)

## Commands

### Snapshot Summary
- Description: Display snapshot summary (epoch, era, counts, param digest)
- Input: path to snapshot file
- Output: human-readable text
- Errors: unsupported era (<505), corrupt file (named section), unknown fields (noted)

### Snapshot Sections
- Description: Display selected sections
- Flags: --params, --governance, --pools, --accounts, --utxo
- Input: path to snapshot file
- Output: only requested sections (human-readable)

### Snapshot Bootstrap
- Description: Parse and dispatch per-module data to bootstrap node
- Input: path to snapshot file
- Output: progress + status; final initialized state or error naming module
- Timeouts: 5s per module acknowledgment
