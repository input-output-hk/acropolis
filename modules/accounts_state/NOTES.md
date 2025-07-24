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
rewards  593536826186446     match from MIRs, no stake rewards
deposits 441012000000        match
fees     10670212208         match

e210:
reserves 13278197552770393   match
treasury 16306644182013      match
rewards  277915861250199     ?? not stake, no MIRs, where do they come from?
deposits 533870000000        match
fees     7666346424          match

e211:
reserves 13270236767315870   X -3.26MA
treasury 24275595982960      match
rewards  164918966125973     X
deposits 594636000000        match
fees     7770532273          match


