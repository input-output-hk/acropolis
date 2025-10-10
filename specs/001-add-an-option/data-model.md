# Data Model: Amaru Snapshot (Conway+)

## Entities

### Snapshot
- epoch: number (≥ 505)
- era: string (Conway+)
- protocolParameters: object (current)
- governance:
  - proposals: [Proposal]
  - committee: CommitteeState
  - constitution: ConstitutionRef
  - dreps: [DRep]
  - governanceActivity: { consecutiveDormantEpochs: number }
- pools:
  - registered: [Pool]
  - updates: [PoolUpdate]
  - retirements: [PoolRetirement]
- accounts:
  - treasury: number
  - reserves: number
  - fees: number
  - entries: [Account]
- utxo:
  - entries: streaming [ (TxIn, TxOut) ]

### Validation Rules
- Epoch must map to Conway+ era.
- Required sections present for summary view.
- Unknown/future fields ignored without failure.

### Relationships
- Governance proposals reference protocol parameters effective at snapshot.
- Pools reference accounts for pledge and reward distribution at epoch.

### Notes
- UTxO is streamed; represent via iterator-like interface in implementation.
