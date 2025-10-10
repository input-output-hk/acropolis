# Memory Profiling in Debug Mode

spec-test includes built-in memory profiling to track RSS (Resident Set Size) during snapshot loading. This helps detect memory leaks, excessive cloning, and verify constant-memory behavior for large files.

## Usage

Enable debug mode to see memory stats:

```bash
# Via command line
cargo run -- --snapshot <file> --manifest <file> --debug

# Via environment variable
DEBUG=1 cargo run -- --snapshot <file> --manifest <file>

# Via Makefile
make debug SNAPSHOT=<file> MANIFEST=<file>
```

## Output Format

Memory stats appear alongside timestamps in debug output:

```
DEBUG: header ... | mem=2.5MB (VMS: 400477.6MB) delta=+0.0MB
DEBUG: progress items=1000 ... | mem=3.2MB delta=+0.7MB
DEBUG: summary ... | mem=3.5MB total_delta=+1.0MB
```

### Fields

- **mem**: Current RSS (Resident Set Size) - physical memory used by process
- **VMS**: Virtual Memory Size (optional, platform-dependent)
- **delta**: Change in RSS since last checkpoint (MB, with sign)
- **total_delta**: Total change in RSS from start of streaming (MB, with sign)

## What to Look For

### ✅ Good (Constant Memory)

```
DEBUG: progress items=0 ... | mem=2.5MB delta=+0.0MB
DEBUG: progress items=1000 ... | mem=2.6MB delta=+0.1MB
DEBUG: progress items=2000 ... | mem=2.6MB delta=+0.0MB
DEBUG: progress items=3000 ... | mem=2.6MB delta=+0.0MB
```

Small, stable deltas indicate efficient streaming with minimal allocations.

### ⚠️ Warning (Growing Memory)

```
DEBUG: progress items=0 ... | mem=2.5MB delta=+0.0MB
DEBUG: progress items=1000 ... | mem=10.2MB delta=+7.7MB
DEBUG: progress items=2000 ... | mem=18.4MB delta=+8.2MB
DEBUG: progress items=3000 ... | mem=26.1MB delta=+7.7MB
```

Consistently growing memory suggests:
- Data accumulation instead of streaming
- Excessive cloning of large structures
- Missing drop/deallocation

### 🔴 Critical (Memory Spike)

```
DEBUG: progress items=0 ... | mem=2.5MB delta=+0.0MB
DEBUG: progress items=1000 ... | mem=2.6MB delta=+0.1MB
DEBUG: progress items=2000 ... | mem=250.3MB delta=+247.7MB  <-- SPIKE
DEBUG: progress items=3000 ... | mem=251.0MB delta=+0.7MB
```

Sudden spike indicates:
- Loading large chunk into memory
- Deep clone of nested structure
- Buffer reallocation

## Platform Support

### macOS
Uses `task_info` API for accurate process memory:
- RSS: Resident memory (physical pages)
- VMS: Virtual memory size

### Linux
Reads `/proc/self/statm`:
- RSS: From `statm` field 2 × page size
- VMS: From `statm` field 1 × page size

### Other Platforms
Returns zero values (graceful degradation).

## Implementation Notes

- **Zero Overhead**: Memory profiling is only active when `--debug` flag is used
- **Checkpoints**: Memory is sampled at:
  - Start of streaming
  - Every 1000 items or 1 second (whichever comes first)
  - End of streaming (summary)
- **Safety**: Uses platform-specific `unsafe` blocks for system APIs (documented and minimal)
- **Accuracy**: RSS is the most reliable metric for physical memory usage

## Example: Large File Analysis

```bash
# Test with 2.4GB Amaru snapshot
cargo run --release -- \
  --snapshot tests/fixtures/134092758.*.cbor \
  --manifest tests/fixtures/134092758.*.json \
  --debug

# Expected output (constant memory):
# DEBUG: header ... | mem=18.2MB delta=+0.0MB
# DEBUG: progress items=1000 ... | mem=18.5MB delta=+0.3MB
# DEBUG: progress items=2000 ... | mem=18.6MB delta=+0.1MB
# ...
# DEBUG: summary ... | mem=19.0MB total_delta=+0.8MB
```

For a 2.4GB file, RSS should remain under ~20MB if streaming correctly.

## Troubleshooting

**Q: Why is VMS so large?**  
A: VMS includes memory-mapped files, shared libraries, and address space reservations. Focus on RSS for actual physical memory usage.

**Q: Delta shows +0.0MB but memory is clearly growing**  
A: Deltas < 0.05MB are rounded to 0.0. Check total_delta in summary for accumulated changes.

**Q: Getting zero values on Windows**  
A: Windows support not yet implemented. Consider adding via Windows API calls.

## See Also

- [Debug Mode Documentation](./debug-mode.md)
- [Snapshot Formats](./snapshot-formats.md)
- Memory profiler source: `src/memory.rs`
