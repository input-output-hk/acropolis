# Data Model: Consensus Tree

**Phase 1 output for feature 1-consensus-tree**

**Sources**:
- `docs/architecture/consensus-tree.md`
- `docs/architecture/system-multi-peer-consensus.md`

## Entities

### BlockValidationStatus

Tracks where a block is in the fetch-validate lifecycle.

| Variant   | Description                                       |
|-----------|---------------------------------------------------|
| Offered   | Header known; on unfavoured fork; not yet fetched |
| Wanted    | Fetch requested; on favoured chain                |
| Fetched   | Body received; awaiting validation                |
| Validated | Passed validation; safe to apply                  |
| Rejected  | Failed validation; will be removed                |

**Lifecycle**:
```text
  Offered ──(chain switch)──> Wanted ──(body arrives)──> Fetched ──> Validated
     │                                                       │
     │──(extends favoured)──> Wanted                         └──> Rejected
```

### TreeBlock

A node in the consensus tree representing a block header (and
optionally its body) within the volatile window.

| Field    | Type                    | Description                        |
|----------|-------------------------|------------------------------------|
| hash     | BlockHash               | 32-byte block hash (identity key)  |
| number   | u64                     | Block height                       |
| slot     | u64                     | Slot number                        |
| body     | Option\<Vec\<u8\>\>     | Raw block body; None until fetched |
| parent   | Option\<BlockHash\>     | Parent block hash; None for root   |
| children | Vec\<BlockHash\>        | Child block hashes                 |
| status   | BlockValidationStatus   | Current lifecycle status           |

**Identity**: `hash` (unique, used as HashMap key)

### ConsensusTree

The top-level data structure managing all volatile blocks.

| Field        | Type                               | Description                       |
|--------------|------------------------------------|-----------------------------------|
| blocks       | HashMap\<BlockHash, TreeBlock\>    | All blocks keyed by hash          |
| root         | Option\<BlockHash\>                | Root of the tree (oldest block)   |
| favoured_tip | Option\<BlockHash\>                | Current favoured chain tip        |
| k            | u64                                | Security parameter (default 2160) |
| observer     | Box\<dyn ConsensusTreeObserver\>   | Callback receiver                 |

**Invariants**:
- `favoured_tip` is always the tip of the longest chain in the tree.
- `root` is always the oldest (lowest-numbered) block in the tree.
- All blocks in `blocks` have `number >= root.number`.
- No block has a fork depth from the favoured chain exceeding `k`.
- Blocks with status `Rejected` are never present (immediately
  removed along with descendants).

## Relationships

```text
ConsensusTree
  |
  +-- blocks: HashMap<BlockHash, TreeBlock>
  |     |
  |     +-- TreeBlock.parent -> BlockHash (another entry in blocks)
  |     +-- TreeBlock.children -> Vec<BlockHash> (entries in blocks)
  |     +-- TreeBlock.status -> BlockValidationStatus
  |
  +-- root -> BlockHash (entry in blocks)
  +-- favoured_tip -> BlockHash (entry in blocks)
```

- **Parent-child**: Each TreeBlock points to its parent by hash and
  maintains a list of children by hash. This is a tree (not DAG) —
  each block has exactly one parent (except root which has none).
- **ConsensusTree -> TreeBlock**: The tree owns all blocks in the
  HashMap. The root and favoured_tip are hash references into it.

## State Transitions

### TreeBlock Lifecycle

```text
  check_block_wanted()     check_block_wanted()      add_block()        mark_validated()
  (unfavoured fork)        (favoured chain)              |                    |
         |                       |                       v                    v
  [Not in tree] ──> [Offered] ──> [Wanted] ──────> [Fetched] ──────> [Validated]
         |                                               |
         └──(extends favoured)──> [Wanted]               v
                                                   mark_rejected()
                                                         |
                                                         v
                                                   [Removed + descendants]
```

When a chain switch occurs, `Offered` blocks on the new favoured
chain transition to `Wanted` and their hashes are returned to the
caller for fetching.

### ConsensusTree State Changes

| Operation          | Tree mutation                        | Observer events            |
|--------------------|--------------------------------------|----------------------------|
| check_block_wanted | Insert TreeBlock (Offered or Wanted) | rollback* + block_wanted*  |
| add_block          | Set body, status → Fetched           | block_proposed*            |
| mark_validated     | Status → Validated                   | (none)                     |
| mark_rejected      | Remove block + descendants           | block_rejected + rollback* |
| remove_block       | Remove block + descendants           | rollback* + block_wanted*  |
| prune              | Remove old blocks + dead forks       | (none)                     |

\* Only if the operation changes the favoured chain.

## Message Flow Mapping

Per `system-multi-peer-consensus.md`, the consensus module maps tree
operations to bus messages:

```text
PNI                        Consensus Module              Tree
 |                              |                          |
 |-- block.offered ------------>|-- check_block_wanted() ->|
 |<- block.wanted --------------|<- wanted hashes ---------|
 |                              |                          |
 |-- block.available ---------->|-- add_block() ---------->|
 |                              |<- block_proposed --------|
 |                              |-- block.proposed ------->| (to validators)
 |                              |                          |
 |                              |<- validation result -----|
 |                              |-- mark_validated() ----->|
 |                              |   or mark_rejected() --->|
 |<- block.rejected ------------|<- block_rejected --------|
 |                              |                          |
 |-- block.rescinded ---------->|-- remove_block() ------->|
 |<- block.wanted --------------|<- wanted hashes ---------|
```

## Error Types

```text
ConsensusTreeError
  +-- ParentNotFound { hash: BlockHash }
  +-- InvalidBlockNumber { expected: u64, got: u64 }
  +-- BlockNotInTree { hash: BlockHash }
  +-- ForkTooDeep { fork_depth: u64, max_k: u64 }
  +-- ValidationFailed { hash: BlockHash }
```

## Observer Trait

```text
ConsensusTreeObserver
  +-- block_proposed(number: u64, hash: BlockHash, body: &[u8])
  +-- rollback(to_block_number: u64)
  +-- block_rejected(hash: BlockHash)
```
