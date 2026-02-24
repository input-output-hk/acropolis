#!/usr/bin/env bash

set -euo pipefail

acropolis_url=${ACROPOLIS_REST_URL:-http://localhost:4340}

# Calculate current epoch
# no leap seconds since genesis, and none planned
genesis=1506203091
epoch_slots=432000
printf -v now "%(%s)T"
current_epoch=$(( $(( now - genesis )) / epoch_slots ))
previous_epoch=$(( current_epoch - 1 ))

target_epoch=${TARGET_EPOCH:-$previous_epoch}
echo "Target epoch: $target_epoch"

logfile=omnibus.txt

get_acropolis_epoch()
{
  local _epoch=${1:-latest}
  curl -s "$acropolis_rest_url/epochs/$_epoch"|jq -r .epoch
}

# clear data from previous runs
rm -rf processes/omnibus/downloads/* \
       processes/omnibus/fjall-* \
       modules/mithril_snapshot_fetcher/downloads/* \
       modules/snapshot_bootstrapper/data/mainnet/*.cbor

# start omnibus process in the background as it needs to
#  still be running when this step ends
make run-bootstrap-store-spdd-drdd > $logfile 2>&1  &

# give omnibus plenty of time to get REST up and running
sleep 30

current_acropolis_epoch=$(get_acropolis_epoch latest)

sleeptime=60

# loop until target epoch is reached
while [ $current_acropolis_epoch -lt $target_epoch ]
do
  printf "%(%c )T"
  echo "Acropolis epoch: $current_acropolis_epoch"
  sleep "$sleeptime"
  current_acropolis_epoch=$(get_acropolis_epoch latest)
done

echo "Reached target epoch: $current_acropolis_epoch"
