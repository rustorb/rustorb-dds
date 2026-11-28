#!/usr/bin/env bash
# Traces a reliable rustorb-dds-perf ping with an OpenDDS pong
# (rustorb-dds#124) and sums it up with trace_summary.py. Run from the
# repository root after bench/run.py built both tools (CI job
# bench-opendds).
#
#   bench/trace_opendds.sh NAME [OPENDDS-ARGS...]
#
# SIZES, WARMUP, DURATION: the sizes to ping (32,1024), the warm-up (0 s)
# and the time of each size (3 s).
# OPENDDS_INI_EXTRA: lines added to [transport/rtps] of rtps.ini.
# OPENDDS_LOG=1: keep OpenDDS's own log (NAME-opendds.log) and count
# what its reader says about each DATA (with -DCPSTransportDebugLevel 6).
set -u
name=$1
shift
out=target/bench/results-opendds
ini=bench/opendds/rtps.ini
if [ -n "${OPENDDS_INI_EXTRA:-}" ]; then
  ini=$out/$name.ini
  { cat bench/opendds/rtps.ini; printf '%s\n' "$OPENDDS_INI_EXTRA"; } > "$ini"
fi
a="--domain 60 --reliability reliable --transport udp --warmup ${WARMUP:-0} --duration ${DURATION:-3} --json --sizes ${SIZES:-32,1024}"
LD_LIBRARY_PATH=$PWD/target/bench/opendds/lib target/bench/opendds/perf pong $a \
  -DCPSConfigFile "$ini" "$@" > "$out/$name-opendds.log" 2>&1 &
pong=$!
sudo tcpdump -i any -U -w "$out/$name.pcap" udp > /dev/null 2>&1 &
sleep 2
target/release/rustorb-dds-perf ping $a || true
sleep 1
sudo pkill tcpdump || true
kill $pong 2> /dev/null || true
wait || true
sudo chmod a+r "$out/$name.pcap"
python3 interop/rtps_dump.py "$out/$name.pcap" 60 --trace --time | python3 bench/trace_summary.py
if [ "${OPENDDS_LOG:-}" = 1 ]; then
  log=$out/$name-opendds.log
  echo "OpenDDS log: $(wc -l < "$log") lines"
  for k in "OK to deliver" "EXPECTING more data" "WITHHOLD" "expecting heartbeat" \
           "ALREADY received" "unknown writer" "deliver sequence" "Count indicates duplicate"; do
    echo "  $k: $(grep -c "$k" "$log")"
  done
  grep -B 3 -A 3 "EXPECTING more data\|WITHHOLD\|expecting heartbeat" "$log" | cut -c1-300 | tail -60 || true
fi
