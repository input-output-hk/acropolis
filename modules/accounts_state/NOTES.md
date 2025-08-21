# Acropolis AccountState module - implementation notes

## Reserve calculations

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


Epoch numbers from DBSync (ada_pots):

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



Per-SPO rewards table for an epoch:

 SELECT
    encode(ph.hash_raw, 'hex') AS pool_id_hex,
    SUM(CASE WHEN r.type = 'member' THEN r.amount ELSE 0 END) AS member_rewards,
    SUM(CASE WHEN r.type = 'leader' THEN r.amount ELSE 0 END) AS leader_rewards
FROM reward r
JOIN pool_hash ph ON r.pool_id = ph.id
WHERE r.spendable_epoch = 213
GROUP BY ph.hash_raw
ORDER BY pool_id_hex;


Epoch 212 (spendable in 213), SPO 30c6319d1f680..., rewards actually given out:

SELECT
    encode(ph.hash_raw, 'hex') AS pool_id_hex,
    SUM(CASE WHEN r.type = 'member' THEN r.amount ELSE 0 END) AS member_rewards,
    SUM(CASE WHEN r.type = 'leader' THEN r.amount ELSE 0 END) AS leader_rewards
FROM reward r
JOIN pool_hash ph ON r.pool_id = ph.id
WHERE r.spendable_epoch = 213
AND encode(ph.hash_raw, 'hex') LIKE '30c6319d1f680%'
GROUP BY ph.hash_raw;

                       pool_id_hex                        | member_rewards | leader_rewards
----------------------------------------------------------+----------------+----------------
 30c6319d1f680470c8d2d48f8d44fd2848fa9b8cd6ac944d4dfc0c54 |    33869550293 |     2164196243

Total 34091555121

We have

2025-08-21T13:59:50.578627Z  INFO acropolis_module_accounts_state::rewards: Pool 30c6319d1f680470c8d2d48f8d44fd2848fa9b8cd6ac944d4dfc0c54 blocks=1 pool_stake=44180895641393 relative_pool_stake=0.001392062719472796345022132114111547444335115561171699064775592918376184270138741760710696148952284469 relative_blocks=0.0005022601707684580612757408337518834756403817177297840281265695630336514314414866901054746358613761929 pool_performance=1 **optimum_rewards=34113076193** pool_rewards=34113076193

Difference: We are too high by 21521072, or 0.06%

Input into this in epoch 212 is:

Calculating rewards: epoch=212 total_supply=31737719158318701 stake_rewards=31854784667376
