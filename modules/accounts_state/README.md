# AccountsState module

This is the module which does the majority of the work in calculating monetary change
(reserves, treasury) and rewards

## Notes on verification

The module has an inbuilt 'Verifier' which can compare against CSV files dumped from
DBSync.

### Pots verification

Verifying the 'pots' values (reserves, treasury, deposits) is a good overall marker of
successful calculation since everything (including rewards) feeds into it.

To create a pots verification file, export the `ada_pots` table as CSV
from Postgres on a DBSync database:

```sql
\COPY (
  SELECT epoch_no AS epoch, reserves, treasury, deposits_stake AS deposits
  FROM ada_pots
  ORDER BY epoch_no
) TO 'pots.mainnet.csv' WITH CSV HEADER
```

Then configure this as (e.g.)

```toml
[module.accounts-state]
verify-pots-file = "../../modules/accounts_state/test-data/pots.mainnet.csv"
```

This is the default, since the pots file is small.  It will be updated periodically.

### Rewards verification

The verifier can also compare the rewards paid to members (delegators) and leader (pool)
against a capture from the DBSync `rewards` table.  We name the files for the epoch *earned*,
which is one less than when we calculate it.

To create a rewards CSV file in Postgres on a DBSync database:

```sql
\COPY (
  select encode(ph.hash_raw, 'hex') as spo, encode(a.hash_raw, 'hex') as address,
         r.type, r.amount
  from reward r
  join pool_hash ph on r.pool_id = ph.id
  join stake_address a on a.id = r.addr_id
  where r.earned_epoch=211 and r.amount > 0
) to 'rewards.mainnet.211.csv' with csv header
```

To configure verification, provide a path template which takes the epoch number:

```toml
[module.accounts-state]
verify-rewards-files = "../../modules/accounts_state/test-data/rewards.mainnet.{}.csv"
```

The verifier will only verify epochs where this file exists.

### SPDD verification

Reference SPDD for epoch N (found in `spdd.mainnet.{}.csv`) was taken from Haskell node 
debug dump (rewards computation routine, Go snapshot for epoch N+3). It is compared with 
`live` SPDD for epoch N (actual at N/N+1 epoch border, distributed with first block of 
epoch N+1).

If verification is not successful, all different pool balances are printed, along
with all pool ids, missing from Acropolis distribution (but found in reference SPDD), 
and extra pool ids (found in Acropolis, but missing in reference SPDD).

To activate verification, use

```toml
[module.accounts-state]
verify-spdd-files = "../../modules/accounts_state/test-data/spdd.mainnet.{}.csv"
```
