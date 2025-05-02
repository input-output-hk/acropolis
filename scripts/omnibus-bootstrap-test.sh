#!/usr/bin/env bash

set -e
logfile=omnibus.txt

# ensure download is set to true
pipx install toml-cli
~/.local/bin/toml set --toml-path  processes/omnibus/omnibus.toml module.mithril-snapshot-fetcher.download true

pushd processes/omnibus
cargo build --release
cargo run --release > $logfile 2>&1  &
sleep 2
cargopid=$(pidof -s acropolis_process_omnibus)

# kill the process if it takes too long, e.g. 60x180=3hrs
sleeptime=60
maxcount=180
count=0
snapshot_complete=

# loop until runtime is reached or message found
while [ $count -lt $maxcount -a "$snapshot_complete" = "" -a "$cargopid" ]
do
  # check for 'snapshot complete' message
  set +e
  snapshot_complete=$(since $logfile | egrep "Notified snapshot complete at slot ")
  set -e

  if [ -z "$snapshot_complete" ]; then
    count=$(( $count + 1 ))
    # show any new epochs/errors
    set +e
    # show errors, and new epochs for progress
    since -s .new_epoch_since $logfile | egrep "(ERROR|acropolis_module_mithril_snapshot_fetcher.*:.* New epoch)"
    set -e
    sleep $sleeptime
    # periodic log as lifetest
    if [ $(( $count % 2 )) -eq 0 ]; then
      date
    fi
  else
    echo $snapshot_complete
    [ $cargopid ] && kill $cargopid
  fi
  cargopid=$(pidof -s acropolis_process_omnibus)

done
