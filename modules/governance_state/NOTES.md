# Governance state module

Allows track governance actions (and keep the state of current governance 
activity in memory).

## Conway: Governance actions implementation details (in random order)

### Message bus reorders messages
Serializer for GovernanceProceduresMessage is necessary (and was used).
Update: no longer necessary.

### Users may re-vote their previously submitted votes
This is ok, mainnet Cardano scanners detect this and show latest vote.

### Governance actions expire in 'gov_action_lifetime' number of epochs
Other names: 'Gov Action Validity', 'govActionLifetime',
updated in governance_action_validity_period parameter (measured in epochs).
That is, if proposal is published in epoch E, then voting is finished at
the end of epoch E+governance_action_validity_period+1.

In another words, each action is granted 'govActionLifetime' full
epochs for voting (P is proposal, E is expiration, govActionLifetime is 6):

`---P--|-----|-----|-----|-----|-----|-----|E`

For +1 origin see e.g. `cardano-node/cardano-testnet/src/Testnet/Components/Query.hs`:

```
-- | Obtains the @govActionLifetime@ from the protocol parameters.
-- The @govActionLifetime@ or governance action maximum lifetime in epochs is
-- the number of epochs such that a governance action submitted during an epoch @e@
-- expires if it is still not ratified as of the end of epoch: @e + govActionLifetime + 1@.
```

Default value (6) is taken from Conway Genesis, which is (in turn) taken from
Cardano book:
https://book.world.dev.cardano.org/environments/mainnet/conway-genesis.json

So, if voting starts at 529, gov_action_lifetime is 6, then the last epoch
when votes have meaning is 535. At the 535/536 transition all votes expire.

### Ratification process.
Ratification checked at epoch boundary. 
If ratified at E/E+1 transition, actions take place at E+1/E+2 boundary. 
Rewards are paid at E+1/E+2 transition (?)
Deposits transferred to reward account.

### Counting votes

Proper votes counting (see `cardano-ledger/eras/conway/impl/src/Cardano/Ledger/Conway/Rules/Ratify.hs`)

Committee:
```
-- Compute the ratio yes/(yes + no), where
-- yes:
--      - the number of registered, unexpired, unresigned committee members that voted yes
-- no:
--      - the number of registered, unexpired, unresigned committee members that voted no, plus
--      - the number of registered, unexpired, unresigned committee members that did not vote for this action
--
-- We iterate over the committee, and incrementally construct the numerator and denominator,
-- based on the votes and the committee state.
```
Committee voting should also check committee members expiration.

```
-- | Final ratio for `totalAcceptedStakePoolsRatio` we want during the bootstrap period is:
-- t = y \/ (s - a)
-- Where:
--  * `y` - total delegated stake that voted Yes
--  * `a` - total delegated stake that voted Abstain
--  * `s` - total delegated stake
--
-- For `HardForkInitiation` all SPOs that didn't vote are considered as
-- `No` votes. Whereas, for all other `GovAction`s, SPOs that didn't
-- vote are considered as `Abstain` votes.
--
-- `No` votes are not counted.
-- After the bootstrap period if an SPO didn't vote, it will be considered as a `No` vote by default.
-- The only exceptions are when a pool delegated to an `AlwaysNoConfidence` or an `AlwaysAbstain` DRep.
-- In those cases, behaviour is as expected: vote `Yes` on `NoConfidence` proposals in case of the former and
-- and vote `Abstain` by default in case of the latter. For `HardForkInitiation`, behaviour is the same as
-- during the bootstrap period: if an SPO didn't vote, their vote will always count as `No`.
```

The description is a bit hard to understand, the actual code is the following:

```
         in case vote of
              Nothing
                | HardForkInitiation {} <- pProcGovAction -> (yes, abstain)
                | bootstrapPhase pv -> (yes, abstain + stake)
                | otherwise -> case defaultStakePoolVote poolId rePoolParams reDelegatees of
                    DefaultNoConfidence
                      | NoConfidence {} <- pProcGovAction -> (yes + stake, abstain)
                    DefaultAbstain -> (yes, abstain + stake)
                    _ -> (yes, abstain) -- Default is No, unless overridden by one of the above cases
              Just Abstain -> (yes, abstain + stake)
              Just VoteNo -> (yes, abstain)
              Just VoteYes -> (yes + stake, abstain)
      (yesStake, abstainStake) =
```
And then the final ratio is:
```
    toInteger yesStake %? toInteger (totalActiveStake - abstainStake)
```

So, the formula is the same and the difference between bootstrap period and normal operation is the way 
how not voted SPOs are treated:
* HardFork voting -- same as in bootstrap, all non-voted SPOs are 'No';
* Normal voting with SPO as 'DefaultNoConfidence' -- no-confidence as 'Yes', otherwise as 'No'
* Normal voting with SPO as 'DefaultAbstain' -- counted as 'Abstain'
* Normal voting with normal SPO -- counted as 'No'

Open question: (TODO) do we have default votes for SPOs?

No traces of different count of SPO active stake and total stake.
Let's consider them the same for the moment.

```
-- Compute the dRep ratio yes/(yes + no), where
-- yes: is the total stake of
--    - registered dReps that voted 'yes', plus
--    - the AlwaysNoConfidence dRep, in case the action is NoConfidence
-- no: is the total stake of
--    - registered dReps that voted 'no', plus
--    - registered dReps that did not vote for this action, plus
--    - the AlwaysNoConfidence dRep
-- In other words, the denominator `yes + no` is the total stake of all registered dReps, minus the abstain votes stake
-- (both credential DReps and AlwaysAbstain)
```

### Bootstrap period (Chang sub-era of Conway)

Conway era is split into two parts: Chang (9.0 protocol version) and 
Plomin (10.0 protocol version). The first ("Chang") epoch has limited 
governance ("bootstrap governance"):

* DReps may vote only for Info actions, they don't count for other actions.
* Only Info, ParameterChange and HardFork actions are allowed.

https://docs.cardano.org/about-cardano/evolution/upgrades/chang

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
* Implement bootstrap period (resolved).
* What if voting length changes during voting process? E.g.:
   (a) epoch 100 voting starts, voting length is 10 epochs
   (b) epoch 107 voting length change to 3 enacts (e.g., because some earlier successful vote)
  Will voting stop immediately in the begninning of epoch 107?

## Alonzo-compatible vote

Speicified in separate field of Alonzo-compatible transactions.
Votes are not divided into propositions and voting.
Votes are cast by genesis key holders.
The proposal needs `update_quorum` votes.
* This is five for mainnet: five genesis key holders must
cast identical proposals.
* This is three for SanchoNet.

### Votes counting and expiration

https://github.com/IntersectMBO/cardano-ledger/blob/640fb66d27ac202764de0dda76621c6d57852ba9/eras/shelley/formal-spec/update.tex

* New votes for a key replace old votes (one genesis key = one proposal).
That is, previous votes for the key expire immediately.

* Votes expire at the end of next epoch: simplified understanding.

* Votes activation is delayed for 6/10 of epoch length. That is, if a vote is
cast at slot with number greater than 4/10 of the epoch length, it'll be counted 
to the next epoch.

### Known issues, unfinished work in current implementation (TODO)

* Check, whether cast votes are really genesis keys. Currently we believe to the
user.

All Alonzo votes in Mainnet were successful, so there is no immediate need to fix
this. The main purpose of voting support is to support proper timing of parameter
updates. However, this may change.

### Voting timing/epochs

Each Alonzo/Babbage update proposal has a parameter (`enactment_epoch`), 
specifying moment of the enactment. The proposal (if approved) becomes effective after 
the specified epoch ends. Let's give some comprehensive example for that.

Example: parameter d=9/10 in Mainnet is proposed at epoch 210 and voted at epoch 210,
so it should be enacted (and used) in epoch 211.
This proposal is serialized (in .json test) as follows:

```
[5357060,210,1,0,[[210,[
    ["Fi+UVUrIwiU4OiJIwkVlntqHDqqC0O8l/H3Ngg==",{"decentralisation_constant":[9,10]}],
    ...
]]]]
```

Expected behaviour:

```
17:16:14.357308 acropolis_module_mithril_snapshot_fetcher: New epoch 211 ...
17:16:14.358807 acropolis_module_epochs_state::state: End of epoch 210 ...
17:16:14.380193 acropolis_module_accounts_state::state: New parameter set: ProtocolParams { 
   byron: ..., 
   alonzo: None, 
   shelley: Some(ShelleyParams { ..., decentralisation_param: Ratio { numer: 9, denom: 10 }, ... }), 
   babbage: None, 
   conway: None 
}
```

Since there is no special 'end of epoch' block, then we use the first block as the 
signal for the previous epoch end. The governance module discovers end of the epoch,
publishes update, then parameter state module catches it and publishes new parameters.

### Testing

https://cexplorer.io/params
