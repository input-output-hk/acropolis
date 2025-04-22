# Governance state module

Allows track governance actions (and keep the state of current governance 
activity in memory).

## Governance actions implementation details (in random order)

### Message bus reorders messages (???)
Serializer for GovernanceProceduresMessage is necessary (and was used).

### Users may re-vote their previously submitted votes
This is ok, mainnet Cardano scanners detect this and show latest vote.

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
