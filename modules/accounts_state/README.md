# AccountsState module

This is the module which does the majority of the work in calculating monetary change
(reserves, treasury) and rewards

## Notes on verification

The module has an inbuilt 'Verifier' which can compare against CSV files dumped from
DBSync.

### Pots verification

Verifying the 'pots' values (reserves, treasury, deposits) is a good overall marker of
successful calculation since everything (including rewards) feeds into it.

To create a pots verification file, export the ada_pots table as CSV
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

