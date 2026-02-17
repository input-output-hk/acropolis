# Quickstart: Consensus Tree

**Phase 1 output for feature 1-consensus-tree**

## Build

```bash
# Build the consensus module (which contains the tree)
cargo build -p acropolis_module_consensus

# Run tests
cargo test -p acropolis_module_consensus

# Full quality check
make fmt && make clippy && make test
```

## Usage (library API sketch)

```rust
use consensus_tree::{
    ConsensusTree, ConsensusTreeObserver, BlockValidationStatus,
};

// 1. Create an observer
struct MyObserver;
impl ConsensusTreeObserver for MyObserver {
    fn block_proposed(&self, number: u64, hash: BlockHash, body: &[u8]) {
        // Publish cardano.block.proposed → validators
    }
    fn rollback(&self, to_block_number: u64) {
        // Publish rollback state transition
    }
    fn block_rejected(&self, hash: BlockHash) {
        // Publish cardano.block.rejected → PNI (sanction peers)
    }
}

// 2. Create the tree with security parameter k
let observer = Box::new(MyObserver);
let mut tree = ConsensusTree::new(k, observer);

// 3. Set the root (genesis or snapshot starting point)
tree.set_root(genesis_hash, genesis_number, genesis_slot)?;

// 4. When PNI offers a block (cardano.block.offered)
let wanted = tree.check_block_wanted(hash, parent_hash, number, slot)?;
// → Returns hashes to request from peers
// → Blocks on unfavoured forks are tracked but NOT in wanted list
// → If chain switch detected: fires rollback, returns newly wanted

// 5. When PNI delivers a block body (cardano.block.available)
tree.add_block(body)?;
// → Status: Wanted → Fetched
// → Fires block_proposed for contiguous fetched blocks

// 6. When validators respond
tree.mark_validated(hash)?;   // Status: Fetched → Validated
// OR
tree.mark_rejected(hash)?;    // Fires block_rejected, removes block
                               // + descendants, may trigger chain switch

// 7. When PNI rescinds a block (cardano.block.rescinded)
let newly_wanted = tree.remove_block(hash)?;
// → Removes block + descendants
// → Fires rollback if chain switches, returns new wanted blocks
```

## Block lifecycle

```text
PNI offers block → check_block_wanted()
  ├─ On favoured chain → status: Wanted → returned in wanted list
  └─ On unfavoured fork → status: Offered → NOT returned

PNI delivers body → add_block()
  └─ status: Wanted → Fetched → fires block_proposed

Validator responds:
  ├─ Success → mark_validated() → status: Validated
  └─ Failure → mark_rejected() → removed, fires block_rejected

Chain switch (unfavoured becomes longest):
  └─ Offered blocks on new chain → Wanted → returned to caller
```

## Integration with existing consensus module

The tree replaces the `// TODO Actually decide on favoured chain!`
in `modules/consensus/src/consensus.rs` (line 85). The module wires
bus messages to tree operations:

| Bus message              | Tree operation           |
|--------------------------|--------------------------|
| cardano.block.offered    | check_block_wanted()     |
| cardano.block.available  | add_block()              |
| cardano.block.rescinded  | remove_block()           |
| cardano.validation.go    | mark_validated()         |
| cardano.validation.nogo  | mark_rejected()          |

Outbound via observer callbacks:

| Observer callback  | Bus message              |
|--------------------|--------------------------|
| block_proposed     | cardano.block.proposed   |
| rollback           | state transition rollback|
| block_rejected     | cardano.block.rejected   |

## Verification

```bash
cargo test -p acropolis_module_consensus -- --nocapture
```

Expected: All tests pass, including:
- Fork topology tests (SC-001): 10+ distinct topologies
- Rollback tests (SC-002): single and multi-level
- Ordering tests (SC-003): block_proposed fires in order
- Pruning tests (SC-004): memory bounded by k
- Error handling tests (SC-005): no panics
- Deep fork rejection (SC-006): bounded maxvalid
- Determinism tests (SC-007): same inputs → same output
- Validation lifecycle tests: Offered → Wanted → Fetched → Validated
- Rejection + chain truncation tests
```
