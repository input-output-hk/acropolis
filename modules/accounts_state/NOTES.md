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


deposits:
    Difference of 998 ADA in 208
    We are 998 too low
    998 = 2 * 500 (SPO) - 2 (one stake address?)

    Total tx deposits in 208:  448512 A
    Listed in ada_pots:        441012
    Ours:                      440014
