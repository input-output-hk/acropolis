Processing of governance transactions
=====================================

If one looks at the dependencies between Governance-state and Parameter-state modules,
one can mention circular dependency: Governance needs current parameters to properly
count votes (voting length, voting thresholds, etc), however, current parameters require
enact state from Governance. So, proper synchronisation between these streams is 
required.

Governance-state dependencies
-----------------------------

Let's describe dependencies in the following diagram, the value on the diagram N.B means
epoch N, block B. The exchange of messages between Parameter-state and Governance-state
takes place asynchronically, after receiving the first block (Governance tx) of a new epoch.

```
Governance txs:     N.k  (N+1).0               *                     *                      (N+1).1
SPOs, Dreps:        *    (N+1).0, epoch N end
GovernanceOutcomes: *    *                     (N+1).0, epoch N end
Parameters:         *    *                     *                     (N+1).0, epoch N+1 start
```

Governance-state dependencies and synchronisation description
-------------------------------------------------------------

The messages are generated in the following order:

1. Governance transactions ("cardano.governance" subscribe topic default name),
where proposals and votes from blockchain users are specified. This message is delivered
each block.

2. Stake distribution channels (SPOs and Dreps). These messages are delivered each epoch:
After epoch N is over (that is, at the first block of the next epoch), the info about stake
distribution is sent, indexed by first block of the epoch N+1.

3. After receiving all source info (Governance txs, SPOs, Dreps) for epoch N, votes
counting is performed, and new enact states are published, indexed by first block of N+1.

4. Parameters state module receives Enact states (and Governance Outcomes in general), 
and publishes updated Parameters, indexed by first block of the epoch N+1.

That is, SPOs, Dreps GovernanceOutcomes and Parameters are all epoch-based channels.
Parameters are published in the beginning of the corresponding epoch, with its first
block; SPO, Drep, GovOutcomes publish info about previous epoch at the first block of next
epoch.
