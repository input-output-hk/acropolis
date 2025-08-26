# Acropolis AccountState module - implementation notes

## Reserve calculations

```
reserve(epoch) = reserve(epoch-1)
               - monetary_expansion
               - sum_of_MIRs_to_or_from_reserves
               + undistributed_rewards                  TODO
               - unspendable_earned_rewards             TODO
               + allegra_bootstrap_addresses_cancelled  TODO

monetary_expansion = reserve(epoch-1) * rho * eta
undistributed_rewards = stake_rewards - actual_rewards  TODO
stake_rewards = total_rewards - treasury_cut
total_rewards = monetary_expansion + fees(epoch-2)   !! Java has -2, spec -1

treasury(epoch) = treasury(epoch-1)
                + treasury_increase
                + sum_of_MIRs_to_or_from_treasury

treasury_increase = total_rewards * tau ( 0.2 )
```

## Epoch numbers from DBSync (ada_pots)

```
e208:    13888022852926644   from Java implementation - can't replicate

e209:
reserves 13286160713028443   match
treasury 8332813711755       match
rewards  593536826186446     match
deposits 441012000000        match
fees     10670212208         match

e210:
reserves 13278197552770393   match
treasury 16306644182013      match
rewards  277915861250199     match
deposits 533870000000        match
fees     7666346424          match

e211:
reserves 13270236767315870   match
treasury 24275595982960      match
rewards  164918966125973     match
deposits 594636000000        match
fees     7770532273          match

e212:
reserves 13262280841681299   match
treasury 32239292149804      match
rewards  147882943225525     match
deposits 626252000000        match
fees     6517886228          match

e213:
reserves 13247093198353459   X - too low by 491kA   \
treasury 40198464232058      X - too low by 4.6kA   |-  Sums almost match
rewards  133110645284460     X - too high by 496kA  /
deposits 651738000000        match
fees     5578218279          match
```


## Per-SPO rewards table for an epoch

```sql
 SELECT
    encode(ph.hash_raw, 'hex') AS pool_id_hex,
    SUM(CASE WHEN r.type = 'member' THEN r.amount ELSE 0 END) AS member_rewards,
    SUM(CASE WHEN r.type = 'leader' THEN r.amount ELSE 0 END) AS leader_rewards
FROM reward r
JOIN pool_hash ph ON r.pool_id = ph.id
WHERE r.spendable_epoch = 213
GROUP BY ph.hash_raw
ORDER BY pool_id_hex;
```

## Specific SPO test

Epoch 211 (spendable in 213), SPO 30c6319d1f680..., rewards actually given out:

```sql
SELECT
    encode(ph.hash_raw, 'hex') AS pool_id_hex,
    SUM(CASE WHEN r.type = 'member' THEN r.amount ELSE 0 END) AS member_rewards,
    SUM(CASE WHEN r.type = 'leader' THEN r.amount ELSE 0 END) AS leader_rewards
FROM reward r
JOIN pool_hash ph ON r.pool_id = ph.id
WHERE r.spendable_epoch = 213
AND encode(ph.hash_raw, 'hex') LIKE '30c6319d1f680%'
GROUP BY ph.hash_raw;
```

Note: pool_id for this SPO is 93

|                       pool_id_hex                        | member_rewards | leader_rewards |
|----------------------------------------------------------|----------------|----------------|
| 30c6319d1f680470c8d2d48f8d44fd2848fa9b8cd6ac944d4dfc0c54 |   32024424770 |      2067130351 |

Total 34091555121

We have

```
2025-08-26T10:49:39.003335Z  INFO acropolis_module_accounts_state::rewards: Pool 30c6319d1f680470c8d2d48f8d44fd2848fa9b8cd6ac944d4dfc0c54 blocks=0 pool_stake=44180895641393 relative_pool_stake=0.001392062719472796345022132114111547444335115561171699064775592918376184270
138741760710696148952284469 relative_blocks=0 pool_performance=1 optimum_rewards=34091555158 pool_rewards=34091555158
```

Optimum rewards: 34091555158

Difference: We are too high by 37 LL compared to DBSync - suspect rounding of individual payments
We match the maxP from the Haskell node:

```
**** Calculating PoolRewardInfo: epoch=0, rewardInfo=PoolRewardInfo {poolRelativeStake = StakeShare (44180895641393 % 31737719158318701), poolPot = Coin 34091555158, poolPs = PoolParams {ppId = KeyHash {unKeyHash = "30c6319d1f680470c8d2d48f8d44fd2848fa9b8cd6ac944d4dfc0c54"}, ppVrf = VRFVerKeyHash {unVRFVerKeyHash = "f2b08e8ec5fe945b41ece1c254e25843e35e574dd43535cbf244524019f704e9"}, ppPledge = Coin 50000000000, ppCost = Coin 340000000, ppMargin = 1 % 20, ppRewardAccount = RewardAccount {raNetwork = Mainnet, raCredential = KeyHashObj (KeyHash {unKeyHash = "8a10720c17ce32b75f489ed13fb706dac51c6006b7fee1a687f36620"})}, ppOwners = fromList [KeyHash {unKeyHash = "8a10720c17ce32b75f489ed13fb706dac51c6006b7fee1a687f36620"}], ppRelays = StrictSeq {fromStrict = fromList [SingleHostName (SJust (Port {portToWord16 = 3001})) (DnsName {dnsToText = "europe1-relay.jpn-sp.net"})]}, ppMetadata = SJust (PoolMetadata {pmUrl = Url {urlToText = "https://tokyostaker.com/metadata/jp3.json"}, pmHash = "\201\246\183K\128\&1 \EOT*\f\194\GS>B\168\136j\239\241\&4\189\230\175\SI4\163\160P\206\162\163]"})}, poolBlocks = 1, poolLeaderReward = LeaderOnlyReward {lRewardPool = KeyHash {unKeyHash = "30c6319d1f680470c8d2d48f8d44fd2848fa9b8cd6ac944d4dfc0c54"}, lRewardAmount = Coin 2067130351}}, activeStake=Coin 10177811974822904, totalStake=Coin 31737719158318701, pledgeRelative=50000000000 % 31737719158318701, sigmaA=44180895641393 % 10177811974822904, maxP=34091555158, appPerf=1 % 1, R=Coin 31834688329017****
```

## ADA pots data from DBSync

First 10 epochs in ada_pots:

```
id  |  slot_no  | epoch_no |     treasury     |     reserves      |     rewards     |       utxo        | deposits_stake |     fees     | block_id | deposits_drep | deposits_proposal 
-----+-----------+----------+------------------+-------------------+-----------------+-------------------+----------------+--------------+----------+---------------+-------------------
   1 |   4924800 |      209 |    8332813711755 | 13286160713028443 | 593536826186446 | 31111517964861148 |   441012000000 |  10670212208 |  4512244 |             0 |                 0
   2 |   5356800 |      210 |   16306644182013 | 13278197552770393 | 277915861250199 | 31427038405450971 |   533870000000 |   7666346424 |  4533814 |             0 |                 0
   3 |   5788800 |      211 |   24275595982960 | 13270236767315870 | 164918966125973 | 31539966264042924 |   594636000000 |   7770532273 |  4555361 |             0 |                 0
   4 |   6220800 |      212 |   32239292149804 | 13262280841681299 | 147882943225525 | 31556964153057144 |   626252000000 |   6517886228 |  4576676 |             0 |                 0
   5 |   6652800 |      213 |   40198464232058 | 13247093198353459 | 133110645284460 | 31578940375911744 |   651738000000 |   5578218279 |  4597956 |             0 |                 0
   6 |   7084800 |      214 |   48148335794725 | 13230232787944838 | 121337581585558 | 31599599756081623 |   674438000000 |   7100593256 |  4619398 |             0 |                 0
   7 |   7516800 |      215 |   55876297807656 | 13212986170770203 | 117660526059600 | 31612774463528795 |   695040000000 |   7501833746 |  4640850 |             0 |                 0
   8 |   7948807 |      216 |   63707722011028 | 13195031638588164 | 122159720478561 | 31618386634872973 |   706174000000 |   8110049274 |  4662422 |             0 |                 0
   9 |   8380800 |      217 |   71629614335572 | 13176528835451373 | 127730158329564 | 31623386398075064 |   719058000000 |   5935808427 |  4683639 |             0 |                 0
  10 |   8812800 |      218 |   79429791062499 | 13157936081322000 | 134680552513121 | 31627219255406326 |   729244000000 |   5075696054 |  4704367 |             0 |                 0
```
