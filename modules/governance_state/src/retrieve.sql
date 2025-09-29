sql
\COPY (
  SELECT epoch_no AS epoch, reserves, treasury, deposits_stake AS deposits
  FROM ada_pots
  ORDER BY epoch_no
) TO 'pots.mainnet.csv' WITH CSV HEADER
