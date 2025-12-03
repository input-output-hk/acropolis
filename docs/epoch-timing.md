# Rewards and pools timing: epochs 

The following is the result of discussions with ledger team
and of analysis of Haskell code.

## Anikett Deshpande's comment on the epoch boundary transition:

In summary, at the epoch boundary:

    TICK is called, calls
    NEWEPOCH which forces the existing rewards pulser to complete and distributes the rewards and then calls
    EPOCH, which calls
    SNAP to rotate the snapshots: now new -> mark, mark -> set , and set -> go
    SNAP returns to
    EPOCH, which returns to
    NEWEPOCH, which returns to
    TICK, which calls
    RUPD, which in turn sets off the new rewards pulser using the newly rotated go snapshot (after stability window ~1.5 days), and returns to
    TICK

In short:

    TICK calls NEWEPOCH
    NEWEPOCH forces pulser and distributes rewards from the go snapshot (we are about to deallocate) which was rotated and marked as go at the previous boundary and was originally snapshotted as mark 2 epoch boundaries before that.
    SNAP rotates the snapshots and takes a new one for mark
    RUPD sets of the new pulser with the newly rotated go snapshot, which was marked as set in the previous epoch and was used for leader schedule processing.

I hope this answers the question much better than before. :blush: (edited) 

## Rewards distribution timing

As we can conclude from that and from the Haskell node, the sequence of events is the following:

* Rewards calculated during epoch E, are calculated based on Go (E-3), and applied to Ledger state:

   ```"NEWEPOCH" rule: es' <- ... updateRewards es eNo ru'```

   ```updateRewards: let !(!es', filtered) = applyRUpdFiltered ru' es```

   ```applyRUpdFiltered:
         ls' =
           ls
             & lsUTxOStateL . utxosFeesL %~ (`addDeltaCoin` deltaF ru)
             & lsCertStateL . certDStateL . dsUnifiedL .~ (rewards dState UM.∪+ registeredAggregated)```

* Current ledger state is converted then into new Mark.

   ```
      es' <- case ru of
        SJust (Complete ru') -> updateRewards es eNo ru'
      es'' <- trans @(EraRule "MIR" era) $ TRC ((), es', ())
      es''' <- trans @(EraRule "EPOCH" era) $ TRC ((), es'', eNo)
      let adaPots = totalAdaPotsES es'''
      ...
      let pd' = ssStakeMarkPoolDistr (esSnapshots es)
    ```

* However, new Mark Pool distribution field does not include rewards.
* Rewards for epoch E first appear in snapshot in epoch E+3 (as mark in EpochState).
* Rewards for epoch E first used for leader scheduling in epoch E+4 (when it becomes set).

Conclusion: rewards, earned by block validation during epoch 209 (TODO: double-check,
add code) and evaluated in epoch 210 (based on epoch 207 stake distribution: 'go' for 210), 
appear in snapshot in epoch 211 (as mark), and first used in epoch 212 for scheduling.

## Pool retirement timing

Each epoch boundary has a set of rules, concerning pool retirement. So, if we have epoch E-1 to E transition:

* Rule "EPOCH", called for epoch (E-1) => E transition, which calls "SNAP" and then "POOLREAP":
* Rule "SNAP" rotates epochs (so, we have Set snapshot for E-1)
* Rule "POOLREAP" removes all pools, retiring in epoch E (so, all pools, 
retiring in E, are not there from the start of the epoch)

So, next iteration of "EPOCH" rule (E=>E+1 transition) would make 
Set snapshot of epoch E (first shapshot without pools).

One more iteration (E+1=>E+2) makes shapshot Mark without pools.

Conclusion: if pool retires in epoch E, it disappears from current Mark 
(VRF active stake) in the beginning of epoch E+2.
