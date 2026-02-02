# Acropolis DrepState module - DRep (Delegated Representative) semantics for Conway era

This document describes the Conway-era DRep model as implemented by the Cardano ledger rules
(the Haskell `cardano-ledger` implementation) and as described at a high level by CIP-1694.

## Terms and moving parts

- **DRep**: can receive delegated voting stake and cast votes on governance actions.
- **Governance action (“proposal”)**: a transaction body’s `proposal_procedures`.
  This is the thing voters vote on.
- **Vote**: a transaction body’s `voting_procedures`.
- **Inactive DRep**: does not count towards “active voting stake”.
- **Dormancy**: a Conway-ledger mechanism that avoids penalizing governance participants during
  periods where there is nothing to vote on. This is tracked via an epoch-level counter and
  interacts with per-DRep expiry.
- **`dRepActivity`**: protocol parameter used when computing `drepExpiry` for a DRep that votes.

## DRep expiration

The ledger (i.e. node state) maintains a per-DRep “expiry”/“activity deadline” state
(`drepExpiry` in `cardano-ledger`). This is an epoch number used to decide whether the DRep
is active at a given epoch.

## High-level lifecycle

### Per-tx (within a block, in tx order)

- For each transaction with proposals, reset the dormant counter to 0 and add the counter to all active DReps expiration.

- Apply DRep activity events that occur in the tx:
  - **DRep vote** in this tx: counts as DRep activity and updates that DRep’s `drepExpiry`.
  - **DRep update certificate** in this tx: also counts as DRep activity.
  - **DRep registration / deregistration** affects whether the DRep exists and whether it can be
    active.

Transactions must be processed in order (the ledger applies its rules sequentially per tx).

### Per-epoch boundary (when moving to the next epoch)

At epoch boundary, the ledger applies epoch-level rules for DReps:

Update the epoch-level **dormancy counter** (“number of dormant epochs”) based on whether there
were governance actions available to vote on during this epoch.

This **dormancy counter** is then used as an input when computing/updating `drepExpiry` for a
specific DRep on activity events (votes or DRep update certificates).

### Dormancy counter rules

The dormancy counter tracks epochs where no governance actions were available to vote on.
This prevents DReps from being penalized for inactivity when there was nothing to vote on.

1. **At each epoch boundary**: If there are no active proposals (all have expired), the dormancy
   counter increases by one.

2. **When a new proposal appears**: The dormancy counter resets to zero, and all active DReps
   get their expiry extended by the number of dormant epochs that were accumulated.

A proposal is considered "active" until the epoch it was submitted plus the governance action
lifetime (a protocol parameter) has passed.

### DRep expiry calculation rules

When a DRep performs an activity (registration, update, or vote), their expiry is set to allow
them to remain active for a number of epochs defined by the `dRepActivity` protocol parameter.

**For DRep registration**, the calculation depends on the protocol version:
- During the Conway bootstrap phase (protocol version 9), the expiry is simply the current epoch
  plus the activity period. Dormant epochs are not considered.
- After the bootstrap phase (protocol version 10 and later), dormant epochs are subtracted from
  the expiry, effectively giving the DRep credit for time when there was nothing to vote on.

**For DRep updates and votes**, dormant epochs are always subtracted from the expiry calculation,
regardless of protocol version.

**Reference**: See `computeDRepExpiryVersioned` and `computeDRepExpiry` in
`cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/GovCert.hs`

## What makes a DRep “active” after it expired?

DRep becomes active again when it performs any of the activity events:

- Vote: any vote on any governance action
- Submits a DRep update certificate
- Registering DRep (first registration or after deregistration) makes it automatically active
