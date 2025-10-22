# Streaming Snapshot Parser

## Overview

The `streaming_snapshot.rs` module provides a **callback-based streaming parser** for Cardano snapshots designed specifically for the **bootstrap process**. This parser navigates the full `NewEpochState` structure and invokes user-provided callbacks for different data types.

## Use Case

This parser is designed for the **Acropolis bootstrap process** where initial state must be distributed via the message bus to multiple state modules:

- **UTXO State Module**: Receives individual UTXO entries
- **SPO State Module**: Receives bulk stake pool data
- **Accounts State Module**: Receives bulk stake account data (delegations + rewards)
- **DRep State Module**: Receives bulk DRep (Delegated Representative) data
- **Governance State Module**: Receives bulk proposal data

## Architecture

### Callback-Based Design

The parser uses **trait-based callbacks** for maximum flexibility:

```rust
pub trait UtxoCallback {
    fn on_utxo(&mut self, utxo: UtxoEntry) -> Result<()>;
}

pub trait PoolCallback {
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()>;
}

pub trait StakeCallback {
    fn on_accounts(&mut self, accounts: Vec<AccountState>) -> Result<()>;
}

pub trait DRepCallback {
    fn on_dreps(&mut self, dreps: Vec<DRepInfo>) -> Result<()>;
}

pub trait ProposalCallback {
    fn on_proposals(&mut self, proposals: Vec<GovernanceProposal>) -> Result<()>;
}

pub trait SnapshotCallbacks: UtxoCallback + PoolCallback + StakeCallback + DRepCallback + ProposalCallback {
    fn on_metadata(&mut self, metadata: SnapshotMetadata) -> Result<()>;
    fn on_complete(&mut self) -> Result<()>;
}
```

### Data Types

All data structures are derived from the **OpenAPI schema** (`API/openapi.yaml`):

- **UtxoEntry**: Transaction hash, output index, address (Bech32), value (lovelace), optional datum/script_ref
- **PoolInfo**: Pool ID, VRF key, pledge, cost, margin, reward account, owners, relays, metadata
- **AccountState**: Stake address, UTXO value, rewards, SPO delegation, DRep delegation
- **DRepInfo**: DRep ID, deposit, anchor (URL + hash)
- **GovernanceProposal**: Deposit, proposer, action ID, action type, anchor
- **PoolBlockProduction**: Pool ID, block count, epoch (block production statistics per pool)

### Block Production Statistics

The parser extracts **block production statistics** from the `NewEpochState` structure:

- **blocks_previous_epoch**: Pool block production data from elements `[1]` (previous epoch)
- **blocks_current_epoch**: Pool block production data from elements `[2]` (current epoch)

**CBOR Structure**: Block production data is stored as indefinite-length CBOR maps where:
- **Keys**: Pool IDs (28-byte stake pool identifiers)  
- **Values**: u8 block counts (number of blocks produced by each pool)

**Data Structure**: `PoolBlockProduction` contains:
- `pool_id`: Hex-encoded pool identifier
- `block_count`: Number of blocks produced by this pool
- `epoch`: Epoch number when blocks were produced

**Example parsing results**:
```
📦 Block production previous epoch: 1,075 pools produced 21,449 blocks total
📦 Block production current epoch: 1,037 pools produced 20,803 blocks total
```

**Important**: This data represents **aggregate statistics** per pool, not individual block details. Individual block hashes, timestamps, and slot numbers are not available in the snapshot.

### NewEpochState Navigation

The parser navigates the Haskell `NewEpochState` structure:

```
NewEpochState = [
  0: epoch_no,
  1: blocks_previous_epoch,
  2: blocks_current_epoch,
  3: EpochState = [
       0: AccountState = [
            0: treasury,
            1: reserves,
            2: rewards (map: stake_credential -> lovelace),
            3: delegations (map: stake_credential -> pool_id),
          ],
       1: SnapShots,
       2: LedgerState = [
            0: CertState = [
                 0: VState = [dreps, cc],
                 1: PState = [pools, future_pools, retiring, deposits],
                 2: DState = [unified_rewards, fut_gen_deleg, gen_deleg, instant_rewards],
               ],
            1: UTxOState = [
                 0: utxos (map: TxIn -> TxOut),
                 1: deposits,
                 2: fees,
                 3: gov_state,
                 4: donations,
               ],
          ],
       3: PParams,
       4: PParamsPrevious,
     ],
  4: PoolDistr,
  5: StakeDistr,
]
```

### Callback Invocation Order

1. **on_utxo()**: Called once per UTXO (streaming, memory-efficient)
2. **on_pools()**: Called once with all stake pool data (bulk)
3. **on_accounts()**: Called once with all stake accounts (bulk)
4. **on_dreps()**: Called once with all DReps (bulk)
5. **on_proposals()**: Called once with all proposals (bulk)
6. **on_metadata()**: Called after all data with epoch, treasury, reserves, deposits, and block counts
7. **on_complete()**: Called last when parsing finishes

## Usage Example

```rust
use acropolis_common::snapshot::{StreamingSnapshotParser, CollectingCallbacks};

// Create parser
let parser = StreamingSnapshotParser::new("/path/to/snapshot.cbor");

// Create callbacks handler (or implement your own)
let mut callbacks = CollectingCallbacks::default();

// Parse snapshot and invoke callbacks
parser.parse(&mut callbacks)?;

// Access collected data
println!("Epoch: {}", callbacks.metadata.unwrap().epoch);
println!("UTXOs collected: {}", callbacks.utxos.len());
println!("Pools collected: {}", callbacks.pools.len());
```

### Custom Callback Handler Example

```rust
struct MessageBusPublisher {
    utxo_bus: MessageBus,
    pool_bus: MessageBus,
    // ... other buses
}

impl UtxoCallback for MessageBusPublisher {
    fn on_utxo(&mut self, utxo: UtxoEntry) -> Result<()> {
        // Publish each UTXO to message bus as it's parsed
        self.utxo_bus.publish(Message::UtxoAdded { utxo })?;
        Ok(())
    }
}

impl PoolCallback for MessageBusPublisher {
    fn on_pools(&mut self, pools: Vec<PoolInfo>) -> Result<()> {
        // Publish all pools at once
        for pool in pools {
            self.pool_bus.publish(Message::PoolRegistered { pool })?;
        }
        Ok(())
    }
}

// Implement other traits...
```

## Features

### ✅ Implemented

- **Full NewEpochState navigation**: Parses epoch, treasury, reserves, rewards, delegations
- **UTXO streaming**: Memory-efficient per-entry callback for 11M+ UTXOs (~1.9M UTXOs/second)
- **Pool parsing**: Complete PState parsing with full pool details (VRF keys, pledge, cost, margin, relays, metadata)
- **DRep parsing**: Complete VState parsing with DRep deposits and anchor metadata
- **Stake account parsing**: Full delegation state with SPO and DRep delegations
- **Map-based TxOut support**: Handles both array and map formats (Conway era)
- **Callback trait architecture**: Flexible handler implementation
- **OpenAPI-aligned types**: All data structures match REST API schemas
- **Test helper**: `CollectingCallbacks` for testing and simple use cases

### 🚧 TODO (Future Enhancements)

- **Proposal parsing**: Needs GovState navigation from UTxOState[3]
- **Bech32 encoding**: Currently uses hex format for addresses (Bech32 can be added later if needed)

## Parser Design

The streaming snapshot parser is designed for:
- **Primary Use**: Bootstrap state distribution
- **UTXO Processing**: Stream all with per-entry callbacks
- **Output Style**: Callback invocation (trait-based)
- **Memory Usage**: Efficient streaming (processes one UTXO at a time)
- **Extensibility**: Trait-based callbacks for flexibility
- **Pool/DRep/Account Data**: Full details with bulk callbacks

## Integration Path

1. **Snapshot Bootstrapper Module** should implement `SnapshotCallbacks`
2. Each callback publishes messages to appropriate state modules
3. State modules process messages as they arrive during bootstrap
4. Bootstrap progress can be tracked via callback counts

## Dependencies

- **minicbor 0.26**: CBOR parsing
- **serde**: Serialization/deserialization
- **anyhow**: Error handling
- **hex**: Hex encoding utilities

## Testing

Run tests with:

```bash
cargo test --package acropolis_common snapshot
```

The `test_collecting_callbacks` test validates the trait implementation and callback invocation.

### Example Usage

The `test_streaming_parser.rs` example demonstrates the streaming parser with block parsing:

```bash
make snap-test-streaming
```

**Example Output**:
```
🔄 Parsing snapshot...
📊 Epoch: 507
💰 Treasury: 1,528,154,947 ADA
🏛️ Reserves: 7,816,251,180 ADA
🏊 Accounts: 1,344,662
🗳️ DReps: 278
🏊 Pools: 3,095
💳 UTXOs: 11,199,911 (2.4M UTXOs/sec)
💸 Fees: 156,471,928 ADA
🏛️ Proposals: 1
📦 Block production previous epoch: 1,075 pools produced 21,449 blocks total
📦 Block production current epoch: 1,037 pools produced 20,803 blocks total
✅ Parsing complete!
```

The example shows real block production statistics from Conway era epoch 507, where 1,075 pools produced 21,449 blocks in the previous epoch and 1,037 pools produced 20,803 blocks in the current epoch.

## Future Enhancements

1. **Individual block details**: Access individual block hashes, timestamps, slots (would require different data source)
2. **Governance proposal parsing**: Parse GovState from UTxOState[3] for proposal data
3. **Bech32 address encoding**: Add optional Bech32 encoding for addresses (currently hex)
4. **Memory-mapped I/O**: Use `memmap2` for even lower memory usage
5. **Progress callbacks**: Add progress tracking for long parses
6. **Selective parsing**: Allow skipping sections (e.g., "UTXOs only")
7. **Parallel processing**: Parse different sections concurrently
