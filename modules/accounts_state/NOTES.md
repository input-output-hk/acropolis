# Acropolis AccountState module - implementation notes

## Reserve calculations

```
reserve(epoch) = reserve(epoch-1)
               - monetary_expansion
               - sum_of_MIRs_to_or_from_reserves
               + undistributed_rewards
               - unspendable_earned_rewards
               + allegra_bootstrap_addresses_cancelled

monetary_expansion = reserve(epoch-1) * rho * eta
undistributed_rewards = stake_rewards - actual_rewards
stake_rewards = total_rewards - treasury_cut
total_rewards = monetary_expansion + fees(epoch-2)

treasury(epoch) = treasury(epoch-1)
                + treasury_increase
                + sum_of_MIRs_to_or_from_treasury

treasury_increase = total_rewards * tau ( 0.2 )
```

epoch 208:
    reserves at start: 13888022852926644
    fees: 10670212208 OK (ada_pots)
    mirs: 593529326186446 OK (ada_pots 'rewards' + reward_rest)

epoch 209:
    reserves at start: 13286160713028443 (ada pots)
                       13294493526740198 (acropolis)
                 diff: 8332813711755 = treasury from ada pots!


reserves(208) * rho * tau = 8332813711755 = db sync treasury cut

therefore:
  fees are actually taken from epoch-2 (as per Java code, not spec)
  reserves captured *before* MIRs of the previous epoch
  ada_pots shows reserves *after* treasury taken

 => mark/set/go needs to include fees, pot values
