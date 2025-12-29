# System description - bootstrap and sync with basic ledger

Previously we created a [simple UTXO follower with live sync](system-simple-mithril-and-sync-utxo.md)
which only tracked UTXOs.  Now we want to add a more complete ledger, with tracking of
Stake Pool Operators (SPOs), delegation, rewards, reserves and treasury.

For this we need to add some more modules:

* [SPO State](../../modules/spo_state) which tracks SPO registration and retirement
* [Epochs State](../../modules/epochs_state) which counts blocks and fees for each epoch
* [Accounts State](../../modules/accounts_state) which tracks stake address balances, SPO delegation, monetary and reward accounts
* [Stake Delta Filter](../../modules/stake_delta_filter) which handles stake pointer addresses
* [Parameters State](../../modules/parameters_state) which tracks protocol parameters
* [Governance State](../../modules/governance_state) which watches governance changes

## Module graph

```mermaid
flowchart LR
  GEN(Genesis Bootstrapper)
  MSF(Mithril Snapshot Fetcher)
  PNI(Peer Network Interface)
  BU(Block Unpacker)
  TXU(Tx Unpacker)
  UTXO(UTXO State)
  SPO(SPO State)
  ES(Epochs State)
  AC(Accounts State)
  SDF(Stake Delta Filter)
  PARAM(Parameters State)
  GOV(Governance State)

  GEN -- cardano.sequence.bootstrapped --> MSF
  MSF -- cardano.block.available --> BU
  MSF -- cardano.snapshot.complete --> PNI
  PNI -- cardano.block.available --> BU
  BU  -- cardano.txs --> TXU
  TXU -- cardano.utxo.deltas --> UTXO
  GEN -- cardano.utxo.deltas --> UTXO
  UTXO -- cardano.address.delta --> SDF
  SDF  -- cardano.stake.deltas --> AC
  TXU  -- cardano.certificates --> SDF
  TXU  -- cardano.certificates --> SPO
  TXU  -- cardano.certificates --> AC
  TXU  -- cardano.withdrawals --> AC
  TXU  -- cardano.goverance --> GOV
  SPO  SPO_AC@-- cardano.spo.state --> AC
  GEN  -- cardano.pot.deltas --> AC
  TXU  -- cardano.block.txs --> ES
  ES   ES_AC@-- cardano.epoch.activity --> AC
  PARAM PARAM_GOV@-- cardano.protocol.parameters --> GOV
  PARAM PARAM_AC@-- cardano.protocol.parameters --> AC
  GOV   GOV_PARAM@ -- cardano.enact.state --> PARAM

  click GEN "https://github.com/input-output-hk/acropolis/tree/main/modules/genesis_bootstrapper/"
  click MSF "https://github.com/input-output-hk/acropolis/tree/main/modules/mithril_snapshot_fetcher/"
  click PNI "https://github.com/input-output-hk/acropolis/tree/main/modules/peer_network_interface/"
  click BU "https://github.com/input-output-hk/acropolis/tree/main/modules/block_unpacker/"
  click TXU "https://github.com/input-output-hk/acropolis/tree/main/modules/tx_unpacker/"
  click UTXO "https://github.com/input-output-hk/acropolis/tree/main/modules/utxo_state/"
  click SPO "https://github.com/input-output-hk/acropolis/tree/main/modules/spo_state/"
  click ES "https://github.com/input-output-hk/acropolis/tree/main/modules/epochs_state/"
  click AC "https://github.com/input-output-hk/acropolis/tree/main/modules/accounts_state/"
  click SDF "https://github.com/input-output-hk/acropolis/tree/main/modules/stake_delta_filter/"
  click PARAM "https://github.com/input-output-hk/acropolis/tree/main/modules/parameters_state/"
  click GOV "https://github.com/input-output-hk/acropolis/tree/main/modules/governance_state/"

  classDef NEW fill:#efe
  class SPO NEW
  class ES NEW
  class AC NEW
  class SDF NEW
  class PARAM NEW
  class GOV NEW

  classDef EPOCH stroke:#008
  class SPO_AC EPOCH
  class ES_AC EPOCH
  class PARAM_GOV EPOCH
  class PARAM_AC EPOCH
  class GOV_PARAM EPOCH
```

## Data flow

The process bootstraps from Mithril, then syncs from the live chain and tracks UTXOs exactly
as [before](system-simple-mithril-and-sync-utxo.md).  We will add much more comprehensive
tracking of the ledger state for the Shelley era only for now - Conway governance will
come later.

### SPOs
The first thing we need to track are Stake Pool Operators.  This is done with a new
[SPO State](../../modules/spo_state) module.  It subscribes to `cardano.tx.certificates`
produced by the [TX Unpacker](../../modules/tx_unpacker), which carry most of the 'events'
to do with chain management.  In this case it is just interested in SPO registrations
and deregistrations (retirements).  It keeps an internal store of these and issues a complete
list of all active SPOs and their details at the start of each epoch, on `cardano.spo.state`.

Note that this message is the first we've seen that happens on each *epoch* rather than
each *block*.  We colour these in blue in the diagram above.

### Accounts State
This message is picked up by the new [Accounts State](../../modules/accounts_state) module.
Accounts State has a lot of functions - we'll discuss why they are all combined later - but
its primary output is the Stake Pool Delegation Distribution (SPDD) which gives the total
stake (both UTXOs and rewards) delegated to each SPO.  This is a core part of the Ouroboros
protocol, since it defines which SPOs are allowed to produce blocks.

In order to do this, Accounts State also tracks the value of each stake address.  Remember that
Cardano addresses can (and usually do) have two parts, the payment address (`addr1xxx`) and the
stake address (`stake1xxx`).  It is the stake address that people usually think of as the 'wallet',
and can have multiple payment addresses associated with it.  It is also - as its name implies -
the thing that is delegated to SPOs.

When a UTXO is created (a transaction output) or spent (by a transaction input), the
[UTXO State](../../modules/utxo_state) we've already seen sends a `cardano.address.delta`
message with the full address (both payment and stake part) and the change of value.  This
should be enough for the Accounts State to track the value, but there's a complication...

### Stake address pointers
There is another form of stake address which is a pointer (by slot,
transaction index and certificate index) to the stake registration
certificate.  This was supposed to save space compared to the full
address format, but it was hardly ever used (only 5 exist on mainnet!)
and has now been withdrawn, although the old ones are still valid.

To handle this, we add another module, the [Stake Delta Filter](../../modules/stake_delta_filter)
which keeps a list of all the stake delegations, which it receives from `cardano.certificates`
and converts any pointers into their full form.
It also filters out any address deltas that don't include any stake address information (some
addresses don't).  The cleaned-up deltas are then published on `cardano.stake.deltas`, which
is what the Accounts State actually subscribes to.

### Epoch activity

Another new module, [Epoch State](../../modules/epoch_state) counts up all the fees paid on
transactions in each epoch, and also how many blocks each SPO produced.  It gets this information
from block data sent on `cardano.block.txs` .It sends the totals to
Accounts State on `cardano.epoch.activity`.

### Monetary pots
The Accounts State module also tracks the global monetary 'pots', including the reserves,
treasury and deposit accounts.  To start this off it receives `cardano.pot.deltas` from the
genesis bootstrapper which sets the initial reserves allocation - at this point the treasury
and deposits are zero.

Then at the start of each epoch, a proportion of the reserves, plus
the fees, is allocated to the treasury, and a further portion to pay
rewards.

### Rewards

The Cardano rewards system is an accounts-based layer on top of the raw UTXO model.  Each
stake address has a reward account, and rewards are earned for block production both by SPOs -
to recompense them for running the network - and to ordinary users who delegate their stake
to them - as a kind of yield for holding Ada and participating in the Proof of Stake system.

The rewards calculation is complex, and deserves its own page (TODO) but at this level we can
survey what is required to do it in the Accounts State module.  It needs:

* The current set of SPOs and all their parameters such as fixed cost and margin
(`cardano.spo.state`)
* Delegation events indicating which stake addresses are delegated to which SPOs
(`cardano.certificates`)
* Stake address deltas (`cardano.stake.deltas`) as already mentioned
* Counts of blocks produced per SPO for each epoch (`cardano.epoch.activity`)

The result of this is at each new epoch (actually a fixed time into it),
Accounts State looks at each SPO and its success in producing
blocks in the previous epoch, derives a total share of the rewards
available to be paid to that SPO and its delegators, calculates the
amount for the SPO itself, then splits the remainder according to the
stakes of the delegators captured from two epochs ago.  These rewards
are then held ready to actually be paid at the start of the next
epoch.

### Deposits

Accounts State also needs to track SPO and stake address registrations
and deregistrations to keep account of the deposits, which it receives
through `cardano.tx.certificates`.  When an SPO retires, or a stake address
is deregistered, the deposit is paid back to their reward account.

### Withdrawals

The value accumulated in a reward account cannot be spent directly like a UTXO, but there is a
mechanism to withdraw it - a transaction can have a withdrawal added, which adds a specified
value to the sum of the input values for the transaction, which can then be moved to other UTXOs.
User wallets usually do this automatically when required so the user isn't aware of it.

Accounts State gets withdrawal information from `cardano.withdrawals` sent by the Tx Unpacker.

### Instantaneous Rewards

In earlier eras of Cardano, there was also the Move Instantaneous
Rewards (MIR) mechanism to move rewards the other way, direct from
reserves or treasury to a reward account.  This was used to move
rewards from the Incentivised Testnet (ITN) at the beginning of
Shelley, and occasionally since for adjustments.  Since Conway,
new MIRs are no longer allowed.

Accounts State receives MIRs through the `cardano.certificates` topic, stores
them up and processes them at the start of each epoch.

### The SPDD

Given all the above, the Accounts State module has enough to track both the UTXO and
rewards balance of every stake address, and which SPO (if any) that stake address is
delegated to.  This allows it, once per epoch, to generate the Stake Pool Delegation Distribution
(SPDD) which it publishes on `cardano.spo.distribution`.  We don't show this going anywhere
at the moment - it's something an external application could pick up, though.  In the
[next build](system-bootstrap-and-sync-with-conway.md) we'll see how it is fed back into
goverance for Conway.

### Parameters and Governance

Although at this stage we aren't handling the full governance of the Conway era (CIP-1694)
even before that there was a governance mechanism to alter protocol parameters, managed
by the founding entities.  We need to track this in order to maintain the correct values of
various calculation parameters as we move through the chain.

To do this, we introduce two new modules: [Governance State](../../modules/governance_state)
and [Parameters State](../../modules/parameters_state).  The Governance State gets governance
events from the Tx Unpacker on `cardano.governance`.  If anything changes during an epoch,
it sends a `cardano.enact.state` message to the Parameters State, which then sends a
`cardano.protocol.parameters` with the updated parameter set.  This is picked up by Accounts
State to get its calculation parameters, and also by Governance State since the parameters
affect the voting system as well.

## Why is AccountsState so big?

TODO: Would otherwise require the exchange or query of data structures (Stake address map) which
is too big to do efficiently.

## Configuration

Here is the
[configuration](../../processes/omnibus/configs/bootstrap-and-sync-with-basic-ledger.toml)
for this setup. You can run it in the `processes/omnibus` directory with:

```shell
$ cargo run --release -- --config configs/bootstrap-and-sync-with-basic-ledger.toml
```
