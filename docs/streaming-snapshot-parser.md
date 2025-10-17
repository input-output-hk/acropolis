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

1. **on_metadata()**: Called first with epoch, treasury, reserves
2. **on_utxo()**: Called once per UTXO (streaming, memory-efficient)
3. **on_pools()**: Called once with all stake pool data (bulk)
4. **on_accounts()**: Called once with all stake accounts (bulk)
5. **on_dreps()**: Called once with all DReps (bulk)
6. **on_proposals()**: Called once with all proposals (bulk)
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
- **UTXO streaming**: Memory-efficient per-entry callback for 11M+ UTXOs
- **Map-based TxOut support**: Handles both array and map formats (Conway era)
- **Callback trait architecture**: Flexible handler implementation
- **OpenAPI-aligned types**: All data structures match REST API schemas
- **Test helper**: `CollectingCallbacks` for testing and simple use cases

### 🚧 TODO (Stub Implementations)

- **Pool parsing**: `parse_pools()` currently returns empty vec (needs PState parsing)
- **DRep parsing**: `parse_dreps()` currently returns empty vec (needs VState parsing)
- **Proposal parsing**: Needs GovState navigation from UTxOState[3]
- **Bech32 encoding**: `encode_address_bech32()` currently returns hex placeholder
- **DRep delegations**: Not yet extracted from stake credentials

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

## Future Enhancements

1. **Memory-mapped I/O**: Use `memmap2` for even lower memory usage
2. **Progress callbacks**: Add progress tracking for long parses
3. **Selective parsing**: Allow skipping sections (e.g., "UTXOs only")
4. **Parallel processing**: Parse different sections concurrently
5. **Complete PState/VState parsing**: Fully implement pool and DRep extraction
