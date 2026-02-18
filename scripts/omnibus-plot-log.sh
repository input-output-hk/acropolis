# This script produces a series of charts given an omnibus log file
# Requirements: awk, gnuplot
#!/bin/sh
set -e

LOG_FILE="$1"

EPOCH_TIME_FILE="/tmp/epochs-time.dat.$$"
UTXO_TIME_FILE="/tmp/utxos-time.dat.$$"
UTXO_EPOCH_FILE="/tmp/utxos-epoch.dat.$$"
SPO_TIME_FILE="/tmp/spos-time.dat.$$"
SPO_EPOCH_FILE="/tmp/spos-epoch.dat.$$"
ALLOCATED_TIME_FILE="/tmp/allocated-time.dat.$$"
ALLOCATED_EPOCH_FILE="/tmp/allocated-epoch.dat.$$"
ERR_TIME_FILE="/tmp/errors-time.dat.$$"
ERR_EPOCH_FILE="/tmp/errors-epoch.dat.$$"
REWARDERR_TIME_FILE="/tmp/rewards-mismatch-time.dat.$$"
REWARDERR_EPOCH_FILE="/tmp/rewards-mismatch-epoch.dat.$$"
VALERR_TIME_FILE="/tmp/validation-errors-time.dat.$$"
VALERR_EPOCH_FILE="/tmp/validation-errors-epoch.dat.$$"

cleanup() {
	rm -f "$EPOCH_FILE" \
          "$UTXO_TIME_FILE" "$UTXO_EPOCH_FILE" \
          "$SPO_TIME_FILE" "$SPO_EPOCH_FILE" \
          "$ALLOCATED_TIME_FILE" "$ALLOCATION_EPOCH_FILE" \
          "$ERR_TIME_FILE" "$ERROR_EPOCH_FILE" \
          "$REWARDERR_TIME_FILE" "$REWARDERR_EPOCH_FILE" \
          "$VALERR_TIME_FILE" "$VALERR_EPOCH_FILE"
}
trap cleanup EXIT

eval "$(sed -r 's/\x1B\[[0-9;]*[A-Za-z]//g' "$LOG_FILE" \
| awk '
BEGIN { epoch = 0 }
{
  ts = $1
  sub(/\.[0-9]+Z$/, "Z", ts)
  if (NR == 1) {
    start_time = ts
  }

  if ($0 ~ /acropolis_module_mithril_snapshot_fetcher.*New epoch/) {
    if (match($0, /epoch=[0-9]+/)) {
      epoch = substr($0, RSTART+6, RLENGTH-6)
      print ts, epoch > "'"$EPOCH_TIME_FILE"'"
    }
  }

  if ($0 ~ /acropolis_module_utxo_state::state/) {
    if (match($0, /valid_utxos=[0-9]+/)) {
        utxos = substr($0, RSTART+12, RLENGTH-12)
        print ts, utxos > "'"$UTXO_TIME_FILE"'"
        print epoch, utxos > "'"$UTXO_EPOCH_FILE"'"
    }
  }

  if ($0 ~ /acropolis_module_spo_state::state/) {
    if (match($0, /num_spos=[0-9]+/)) {
        utxos = substr($0, RSTART+9, RLENGTH-9)
        print ts, utxos > "'"$SPO_TIME_FILE"'"
        print epoch, utxos > "'"$SPO_EPOCH_FILE"'"
    }
  }

  if ($0 ~ /acropolis_module_stats/) {
    if (match($0, /allocated=[0-9]+/)) {
        allocated = substr($0, RSTART+10, RLENGTH-10)
        print ts, allocated > "'"$ALLOCATED_TIME_FILE"'"
        print epoch, allocated > "'"$ALLOCATED_EPOCH_FILE"'"
    }
  }

  if ($0 ~ /ERROR/) {
    print ts, 1 > "'"$ERR_TIME_FILE"'"
    print epoch, 1 > "'"$ERR_EPOCH_FILE"'"
  }

  if ($0 ~ /acropolis_module_accounts_state::verifier: Verification mismatch/) {
    print ts, 1 > "'"$REWARDERR_TIME_FILE"'"
    print epoch, 1 > "'"$REWARDERR_EPOCH_FILE"'"
  }

  if ($0 ~ /acropolis_module_consensus.*Validation failure/) {
    print ts, 1 > "'"$VALERR_TIME_FILE"'"
    print epoch, 1 > "'"$VALERR_EPOCH_FILE"'"
  }
}
END {
  printf "START_TIME=%s\n", start_time
  printf "END_TIME=%s\n", ts
  printf "START_EPOCH=%s\n", 0
  printf "END_EPOCH=%s\n", epoch
}
')"
gnuplot <<EOF
set terminal pngcairo size 2400,2800
set output 'omnibus.png'

set multiplot
set grid
set key off
set lmargin 12

rowh = 1.0/7.0

set size 0.5, rowh
set xdata time
set timefmt '%Y-%m-%dT%H:%M:%SZ'
set xrange ["$START_TIME":"$END_TIME"]
set xlabel "Time"

set origin 0.0, 1.0 - 1.0 * rowh
set ylabel "Epoch"
plot "$EPOCH_TIME_FILE" using 1:2 with linespoints lw 2 pt 7 lc rgb "blue" title "Epoch"

set origin 0.0, 1.0 - 2.0 * rowh
set ylabel "UTxOs"
plot "$UTXO_TIME_FILE" using 1:2 with lines lw 2 lc rgb "orange" title "UTxOs"

set origin 0.0, 1.0 - 3.0 * rowh
set ylabel "SPOs"
plot "$SPO_TIME_FILE" using 1:2 with lines lw 2 lc rgb "purple" title "SPOs"

set origin 0.0, 1.0 - 4.0 * rowh
set ylabel "Memory Usage (allocated)"
plot "$ALLOCATED_TIME_FILE" using 1:2 with lines lw 2 lc rgb "green" title "Allocated"

set origin 0.0, 1.0 - 5.0 * rowh
set ylabel "Errors"
plot "$ERR_TIME_FILE" using 1:2 with impulses lw 1 lc rgb "red" title "Errors"

set origin 0.0, 1.0 - 6.0 * rowh
set ylabel "Rewards Mismatches"
plot "$REWARDERR_TIME_FILE" using 1:2 with impulses lw 1 lc rgb "red" title "Rewards Mismatches"

set origin 0.0, 1.0 - 7.0 * rowh
set ylabel "Validation Errors"
plot "$VALERR_TIME_FILE" using 1:2 with impulses lw 1 lc rgb "red" title "Validation Errors"

unset xdata
set xrange [$START_EPOCH:$END_EPOCH]
set format x "%g"
set xlabel "Epoch"

set origin 0.5, 1.0 - 2.0 * rowh
set ylabel "UTxOs"
plot "$UTXO_EPOCH_FILE" using 1:2 with lines lw 2 lc rgb "orange" title "UTxOs"

set origin 0.5, 1.0 - 3.0 * rowh
set ylabel "SPOs"
plot "$SPO_EPOCH_FILE" using 1:2 with lines lw 2 lc rgb "purple" title "SPOs"

set origin 0.5, 1.0 - 4.0 * rowh
set ylabel "Memory Usage (allocated)"
plot "$ALLOCATED_EPOCH_FILE" using 1:2 with lines lw 2 lc rgb "green" title "Allocated"

set origin 0.5, 1.0 - 5.0 * rowh
set ylabel "Errors"
plot "$ERR_EPOCH_FILE" using 1:2 with impulses lw 1 lc rgb "red" title "Errors"

set origin 0.5, 1.0 - 6.0 * rowh
set ylabel "Rewards Mismatches"
plot "$REWARDERR_EPOCH_FILE" using 1:2 with impulses lw 1 lc rgb "red" title "Rewards Mismatches"

set origin 0.5, 1.0 - 7.0 * rowh
set ylabel "Validation Errors"
plot "$VALERR_EPOCH_FILE" using 1:2 with impulses lw 1 lc rgb "red" title "Validation Errors"

EOF
