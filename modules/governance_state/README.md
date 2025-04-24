# Governance state module

Allows track governance actions (and keep the state of current governance 
activity in memory).

## Governance actions implementation details (in random order)

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

* Check: voted/expired/continuing

* Need to get actual balance of registered DReps/all SPOs at the end of each
epoch. Special message? 

* If voted/expired: generate money transfer (add transactions for rewards)

* Each epoch boundary governance state tracker issues a "rewards update" message.
