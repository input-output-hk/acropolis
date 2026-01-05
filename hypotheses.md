# Pots Investigation - DRep Deposits

## Current Issue

Verification mismatch at epoch 509:
```
Snapshot epoch=508 treasury=1537364145455061 reserves=7789863747110689 rewards=739886917312354 deposits=4373648000000

ERROR: Verification mismatch: reserves for epoch=509 calculated=7789863747110689 desired=7789809997399209 difference=-53749711480
ERROR: Verification mismatch: treasury for epoch=509 calculated=1537364145455061 desired=1537376975934454 difference=12830479393
```

- Reserves: ~53,750 ADA too HIGH
- Treasury: ~12,830 ADA too LOW

---

## Key Finding: DRep Deposits in Haskell Ledger

From `cardano-ledger` Haskell source, the deposits pot (`utxosDeposited`) includes ALL deposit types:

**`libs/cardano-ledger-core/src/Cardano/Ledger/State/CertState.hs`:**
```haskell
data Obligations = Obligations
  { oblStake :: !Coin      -- Stake key deposits
  , oblPool :: !Coin       -- Pool deposits
  , oblDRep :: !Coin       -- DRep deposits
  , oblProposal :: !Coin   -- Governance proposal deposits
  }

sumObligation x = oblStake x <> oblPool x <> oblDRep x <> oblProposal x
```

**`eras/conway/impl/src/Cardano/Ledger/Conway/State/CertState.hs`:**
```haskell
conwayObligationCertState certState =
  (shelleyObligationCertState certState)
    { oblDRep = fromCompact $ F.foldl' accum mempty (certState ^. certVStateL . vsDRepsL) }
```

**`eras/shelley/impl/src/Cardano/Ledger/Shelley/Rules/Epoch.hs`:**
```haskell
-- Invariant: obligationCertState dpstate == utxosDeposited utxostate
oblgNew = totalObligation adjustedCertState (utxoSt'' ^. utxosGovStateL)
utxoSt''' = utxoSt'' {utxosDeposited = oblgNew}
```

**Conclusion:** `utxosDeposited` = oblStake + oblPool + oblDRep + oblProposal

---

## Changes Made

### 1. DRep Registration (`state.rs`)
```rust
TxCertificate::DRepRegistration(reg) => {
    // DRep deposits ARE part of the main deposits pot per Haskell ledger spec.
    self.pots.deposits += reg.deposit;
    self.drep_deposits += reg.deposit; // Track separately for debugging
}
```

### 2. DRep Deregistration (`state.rs`)
```rust
TxCertificate::DRepDeregistration(dereg) => {
    // Currently: refund goes to treasury (testing if stake address update is handled elsewhere)
    self.pots.treasury += dereg.refund;

    // COMMENTED OUT - testing if handled via UTxO deltas:
    // self.pots.deposits -= dereg.refund;
    // stake_addresses.add_to_reward(&stake_address, dereg.refund);
}
```

---

## Open Questions

### Q1: Are DRep deposit refunds handled via UTxO state?

The hypothesis is that when a DRep deregisters:
1. The deposit refund might already be applied to the stake address via `StakeAddressDelta` messages
2. We only need to update `pots.deposits` and `pots.treasury` in accounts_state

**To verify:** Run with current changes and check if SPDD matches after DRep deregistrations.

### Q2: Bootstrap vs Live Processing

Bootstrap pots for 507→508 were correct. Issue is at 508→509 (live processing).

The bootstrap code in `streaming_snapshot.rs` previously subtracted DRep deposits from `us_deposited` - this was reverted since we now understand DRep deposits ARE part of the deposits pot.

---

## Files Modified

- `modules/accounts_state/src/state.rs` - DRep registration/deregistration handling

---

## Next Steps

1. Test current changes to see if pots verification improves
2. If SPDD is wrong after DRep deregistrations, uncomment the `add_to_reward` call
3. Investigate governance proposal deposits (also part of obligation but not tracked during live processing)
