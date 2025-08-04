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

