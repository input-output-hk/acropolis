# System description - multi-peer consensus

[Previously](system-script-validation.md) we tracked the ledger
state for the whole history of the chain, with full validation.  The Consensus
module was informed of any validation failures, but since it was only tracking
a single upstream chain, it had nowhere else to go and could only log an error.

Now we want to implement consensus properly, fetching chain proposals from multiple
peers, choosing the best one and validating it.  If it turns out we've been fed
bad blocks, we need to roll back the ledger state and sanction the peer that gave
us it.  If the chain forks, we may find our preference for the best chain switches,
and we need to roll back to the branch point and replay the blocks on the newly
favoured chain to its current tip.

To do this we don't need to add any new modules, but we do need additional communication
paths and features in existing ones.

**This section is a proposed design, it does not exist yet (Jan 2026)**

## Module graph

We massive simplify the graph now to show just the consensus operation - the whole block unpacking,
ledger and validation infrastructure is just shown as a cloud.

```mermaid
flowchart LR
  PNI(Peer Network Interface)
  CON(Consensus)
  CS(Chain Store)
  CLOUD@{ shape: cloud, label: "Ledger Validation"}

  PNI -- cardano.peer.offer --> CON
  CON -- cardano.peer.fetch --> PNI
  PNI -- cardano.block.available --> CON
  PNI -- cardano.block.uavailable --> CON
  CON -- cardano.block.fetch --> CS
  CON -- cardano.block.delete --> CS
  CS -- cardano.block.fetched --> CON
  PNI -- cardano.peer.lost --> CON
  CON -- cardano.peer.drop --> PNI
  CON -- cardano.block.proposed --> CLOUD
  CLOUD -- cardano.validation.* --> CON

  click PNI "https://github.com/input-output-hk/acropolis/tree/main/modules/peer_network_interface/"
  click CON "https://github.com/input-output-hk/acropolis/tree/main/modules/consensus/"
  click CS "https://github.com/input-output-hk/acropolis/tree/main/modules/chain_store/"
```

## New functionality

We need to add the following new functionality and data flows to existing modules:

### Peer Network Interface
The [Peer Network Interface](../../modules/peer_network_interface) is already able to follow
multiple peers, but selects between them only if the initially selected one disconnects, not
on the basis of the quality of the chain being offered. We need to give this control to
Consensus.

To do this we insert consensus into the decision about which blocks to fetch.  The Peer
Network Interface will indicate that a block is being offered by a peer on `cardano.peer.offer`,
quoting the peer ID, block number and hash.  This allows Consensus to associate the offer with
a particular chain fork (there will likely be multiple peers offering the same fork).

Consensus may then request that a block is fetched with
`cardano.peer.fetch` quoting a list of potential peer IDs that have
offered it, block number and hash.  The Peer Network Interface will
then fetch it from one of the peers, chosen either round-robin or on performance metrics,
and send a `cardano.block.available` when it receives it, as it does already.  If no peer
can provide it, it sends a `cardano.block.unavailable` instead.

If the block provided turns out to be bad, Consensus may tell the Peer Network Interface to
drop the peer connection with a `cardano.peer.drop` message, quoting the peer ID.  Conversely,
if the Peer Network Interface loses or drops the connection to a peer, it can issue a
`cardano.peer.lost` message informing Consensus of this.

Note the peer IDs can be anything that makes sense to identify the peer in the Peer Network
Interface - they are opaque externally.

TODO: Automatic P2P discovery, "ledger peers" (from SPO state?)

### Consensus
The [Consensus](../../modules/consensus) module will need to maintain a tree of chain forks
being offered, with links to and from the peers offering them.  On receipt of a `cardano.peer.offer`
it can look up in the tree which fork it applies to, and look at the block number to see if it
is an extension of the existing chain or a rollback creating a new fork.

It's likely it will be offered the same block from multiple peers, so it should keep its own map
of blocks fetched (limited by 'k' depth), and request to fetch it with a `cardano.peer.fetch`
message if it doesn't already have it.  It should then get a `cardano.block.available` from the
Peer Network Interface, containing the block data.  If it gets a `cardano.block.unavailable` it will
prune the chain tree to remove it.

On receipt of the block, it sends it to the Chain Store with a `cardano.block.store` message.  It
then has two options:

1. If the new block is on the favoured chain (see below) it sends it for validation with
   `cardano.block.proposed` as we saw in the [Phase 1 Validation](system-ledger-validation.md)
   system.
   If successful, it will be added to the chain tree.  If it fails, it won't, and we may trigger
   sanctions against the peers that offered it.

2. If the new block is on another chain, we don't validate it yet, but add it marked as unvalidated
   to the relevant chain in the tree.

Each time a new block is offered, Consensus runs the Ouroboros chain selection rules to determine
the longest / densest chain (density is used when fast syncing - TBD).  This may result in the
favoured chain switching.  When this happens Consensus will signal a rollback and then reissue
blocks on the new favoured chain from the common branching point with the old one onwards.

To do this, it needs to retrieve the blocks from the Chain Store.  It requests them with a
`cardano.block.fetch` message, and the Chain Store responds with a `cardano.block.fetched`
message (note these are not request-response, we don't want to hold up Consensus while it
happens).  On receipt, Consensus then sends them out to the Block Unpacker.  If the blocks
haven't already been validated, it will request validation;  if they have, there is no need.

If the blocks being replayed fail validation, that chain needs to be truncated, which will
probably (but not necessarily) mean it is no longer the favoured one.  The peers that provided it
may be sanctioned accordingly - in the limit, by telling the Peer Network Interface to drop them
with one or more `cardano.peer.drop` messages.

Once the branch point of an unfavoured chain is more than 'k' blocks old, it can never be
selected, so Consensus can prune its tree and tell the Chain Store to delete the candidate blocks
with a `cardano.block.delete` message.

### Chain Store

The [Chain Store](../../modules/chain_store) already provides persistent block storage for
the historical REST APIs.  Now we are extending that to serve as the recovery store for chain
switches as described above.

The Chain Store will no longer accept every block on `cardano.block.available` but will wait for
Consensus to ask it to store one with `cardano.block.store`.  As noted above, it will also
provide a retrieval function with `cardano.block.fetch` and `cardano.block.fetched`, and cleanup
with `cardano.block.delete`.

## Configuration
TODO

## Next steps
TODO



