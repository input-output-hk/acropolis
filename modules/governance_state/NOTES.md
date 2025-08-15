# Governance state module

Allows track governance actions (and keep the state of current governance 
activity in memory).

## Conway: Governance actions implementation details (in random order)

### Message bus reorders messages (???)
Serializer for GovernanceProceduresMessage is necessary (and was used).

### Users may re-vote their previously submitted votes
This is ok, mainnet Cardano scanners detect this and show latest vote.

### Governance actions expire in 'gov_action_lifetime' number of epochs
Other names: 'Gov Action Validity', 'govActionLifetime',
updated in governance_action_validity_period parameter (measured in epochs).
That is, if proposal is published in epoch E, then voting is finished at
the end of epoch E+governance_action_validity_period.

Default value (6) is taken from Conway Genesis, which is (in turn) taken from
Cardano book:
https://book.world.dev.cardano.org/environments/mainnet/conway-genesis.json

### Ratification process.
Ratification checked at epoch boundary. 
If ratified, deposits returned immediately, actions take place at E+1/E+2
boundary.
Deposits transferred to reward account.

### Genesis blocks
* Conway genesis: committee key hashes have prefix 'scriptHash-' (I believe,
'keyHash-'), followed by hex hash. To be researched....

We have two standards (this one and bech32), see next point.

### Bech32 encoding is necessary to look for objects' hash (see CIP 0129)
* The CIP 0129 is here: https://github.com/cardano-foundation/CIPs/pull/857

* DataHash as hex

* DrepKeyHash as Bech32 with 'drep' prefix

* DrepScriptHash as Bech32 with 'drep_script' prefix (??)

* Gov-action as Bech32 with 'gov_action' prefix. One byte for action index 
inside transaction should be inserted into vector:
```transaction_hash [20 bytes]; action_index [1 byte]```
That is, at most 256 voting or proposal actions per transactions are allowed.

* Current gov_action_id is taken from the current transaction info.
Field 'prev_action_id' points to previous governance action, if it has sence
(if the current governance action depends on some particular previous state of 
blockchain).

### Voting implementation: rewards/money

* At each epoch boundary current storage checked for expiring/voted actions.

* Check: accepted/expired/continuing, voting details see CIP-1694

* Need to get actual balance of registered DReps/all SPOs at the end of each
epoch. Special message? 

* If voted/expired: generate money transfer. List of 

* Each epoch boundary governance state tracker issues a "rewards update" message.

### Questions

* How can I get total drep stake? (resolved)
* How can I get pools stake? (resolved)
* DRep stake is registered; however SPO stake is of two kinds --- registered and
total. Need info about voting registration.
* DRep::epoch -- it's written that it's epoch, which has ended. But I receive
messages with this epoch in its beginning. Need to sort out.

## Alonzo-compatible vote

Speicified in separate field of Alonzo-compatible transactions.
Votes are not divided into propositions and voting.
Votes are cast by genesis key holders.
The proposal needs 5 votes: that is, 5 genesis key holders must
cast identical proposals.

### Votes counting and expiration

https://github.com/IntersectMBO/cardano-ledger/blob/640fb66d27ac202764de0dda76621c6d57852ba9/eras/shelley/formal-spec/update.tex

* New votes for a key replace old votes (one genesis key = one proposal).
That is, previous votes for the key expire immediately.

* Votes expire at the end of next epoch: simplified understanding.

* Votes activation is delayed for 6/10 of epoch length. That is, if a vote is
cast at slot with number greater than 4/10 of the epoch length, it'll be counted 
to the next epoch.

### Testing

https://cexplorer.io/params
