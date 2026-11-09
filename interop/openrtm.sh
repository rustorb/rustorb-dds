#!/usr/bin/env bash
# Runs OpenRTM-aist 2.0's example components ConsoleIn and ConsoleOut,
# connected with its Fast DDS transport (interface_type=fast-rtps),
# against the rustorb-dds `openrtm` examples (Rust, C++, Python), in both
# directions, on this host.
#
#   interop/openrtm.sh
#
# Needs OpenRTM-aist 2.0 built with FASTRTPS_ENABLE=ON and installed (its
# examples ConsoleInComp and ConsoleOutComp, and FastRTPSTransport.so);
# see interop/README.md. OpenRTM-aist is used as a black box: its
# programs, as distributed, configured by rtc.conf.
#
# Environment:
#   RTM_PREFIX  where OpenRTM-aist is installed (default /usr/local)
#   RUST        the rustorb-dds `openrtm` example (default: built with cargo)
#   CPP         the C++ example `openrtm` (default: none, its cases are skipped)
#   PYTHON      a Python with rustorb_dds installed (default: none, skipped)
#   OUT         where to write the logs (default target/interop-openrtm)
#   COUNT       samples each side must get (default 5)
#   TIMEOUT     seconds a reader waits (default 30)
#   CAPTURE     if set, capture each case's UDP with tcpdump (cap.pcap) and
#               show its RTPS with tshark when the case fails
#   ONLY        run only the cases whose name matches this extended regular
#               expression (default: all)

set -u
cd "$(dirname "$0")/.."
ROOT=$(pwd)
OUT=${OUT:-$ROOT/target/interop-openrtm}
COUNT=${COUNT:-5}
TIMEOUT=${TIMEOUT:-30}
RTM_PREFIX=${RTM_PREFIX:-/usr/local}
mkdir -p "$OUT"

log() { echo "[openrtm] $*"; }

find_one() { find "$RTM_PREFIX" -name "$1" -type f 2>/dev/null | head -n 1; }
TRANSPORT=$(find_one FastRTPSTransport.so)
CONSOLE_IN=$(find_one ConsoleInComp)
CONSOLE_OUT=$(find_one ConsoleOutComp)
if [ -z "$TRANSPORT" ] || [ -z "$CONSOLE_IN" ] || [ -z "$CONSOLE_OUT" ]; then
  echo "OpenRTM-aist with FastRTPSTransport and its examples is not installed in $RTM_PREFIX" >&2
  exit 2
fi

{
  echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host: $(uname -srm)"
  [ -r /etc/os-release ] && . /etc/os-release && echo "os: $PRETTY_NAME"
  echo "rustorb-dds: $(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
  echo "openrtm: $(pkg-config --modversion openrtm-aist 2>/dev/null || echo unknown)"
  echo "transport: $TRANSPORT"
} > "$OUT/versions.txt"
cat "$OUT/versions.txt"

if [ -z "${RUST:-}" ]; then
  log "building the rustorb-dds openrtm example"
  cargo build --quiet --example openrtm || exit 1
  RUST="$ROOT/target/debug/examples/openrtm"
fi
declare -A PROGRAMS=([rust]="$RUST")
[ -n "${CPP:-}" ] && PROGRAMS[cpp]="$CPP"
[ -n "${PYTHON:-}" ] && PROGRAMS[python]="$PYTHON $ROOT/python/examples/openrtm.py"

# OpenRTM-aist 2.0.2's ports join domain 0 and the topic `chatter`:
# fast-rtps.domain.id in rtc.conf and fast-rtps.topic in the preconnect
# are not applied (seen in captures). So every case uses them, one after
# the other.
DOMAIN=0
TOPIC=chatter
FAILED=()
RESULTS=()

capture_start() {
  CAP_PID=
  if [ -n "${CAPTURE:-}" ] && command -v tcpdump >/dev/null; then
    tcpdump -i any -U -w "$1/cap.pcap" udp >/dev/null 2>&1 &
    CAP_PID=$!
    sleep 1
  fi
}
capture_stop() {
  if [ -n "$CAP_PID" ]; then kill "$CAP_PID" 2>/dev/null; wait "$CAP_PID" 2>/dev/null; fi
}

result() {
  local name=$1 ok=$2
  if [ "$ok" = 1 ]; then
    RESULTS+=("PASS $name")
  else
    RESULTS+=("FAIL $name")
    FAILED+=("$name")
    tail -n 20 "$OUT/$name"/*.log "$OUT/$name"/*.txt 2>/dev/null
    if [ -s "$OUT/$name/cap.pcap" ] && command -v tshark >/dev/null; then
      tshark -r "$OUT/$name/cap.pcap" -Y rtps -T fields -e frame.time_relative \
        -e ip.src -e ip.dst -e udp.dstport -e rtps.domain_id -e rtps.sm.id \
        -e rtps.param.topicName -e rtps.param.typeName 2>/dev/null | head -n 120
    fi
  fi
}

# The rtc.conf of a component `comp` (ConsoleIn or ConsoleOut) whose port
# `port` is connected to the DDS topic `topic`.
rtc_conf() {
  local dir=$1 comp=$2 port=$3 topic=$4
  cat > "$dir/rtc.conf" <<EOF
corba.nameservers:
naming.enable: NO
logger.enable: YES
logger.file_name: $dir/rtc.log
logger.log_level: DEBUG
manager.modules.load_path: $(dirname "$TRANSPORT")
manager.preload.modules: FastRTPSTransport.so
manager.components.precreate: $comp
manager.components.preconnect: ${comp}0.$port?interface_type=fast-rtps&fast-rtps.topic=$topic
manager.components.preactivation: ${comp}0
EOF
}

# ConsoleIn (an OutPort of RTC::TimedLong), a rustorb-dds reader: the
# reader must get COUNT of the numbers typed into ConsoleIn.
openrtm_to() {
  local impl=$1 name="openrtm-to-$1" dir="$OUT/openrtm-to-$1"
  if [ -n "${ONLY:-}" ] && ! [[ $name =~ $ONLY ]]; then return; fi
  log "$name"
  mkdir -p "$dir"
  rtc_conf "$dir" ConsoleIn out "$TOPIC"
  capture_start "$dir"
  (cd "$dir" && for i in $(seq 1001 $((1000 + TIMEOUT * 5))); do echo "$i"; sleep 0.2; done |
    timeout -s INT $((TIMEOUT + 10)) "$CONSOLE_IN" -f rtc.conf >console_in.txt 2>&1) &
  local console=$!
  # shellcheck disable=SC2086
  timeout $((TIMEOUT + 5)) ${PROGRAMS[$impl]} in "$DOMAIN" "$COUNT" "$TIMEOUT" "$TOPIC" >"$dir/reader.log" 2>&1
  local rc=$?
  pkill -INT -f "$CONSOLE_IN -f rtc.conf" 2>/dev/null
  wait "$console" 2>/dev/null
  capture_stop
  local got
  got=$(grep -cE "read 1[0-9]{3} " "$dir/reader.log")
  [ "$rc" -eq 0 ] && [ "$got" -ge "$COUNT" ] && result "$name" 1 || result "$name" 0
}

# A rustorb-dds writer, ConsoleOut (an InPort of RTC::TimedLong): ConsoleOut
# must receive COUNT of the samples, with their data. Both sides are best
# effort, as OpenRTM-aist's ports are by default, so the writer sends more.
to_openrtm() {
  local impl=$1 name="$1-to-openrtm" dir="$OUT/$1-to-openrtm"
  if [ -n "${ONLY:-}" ] && ! [[ $name =~ $ONLY ]]; then return; fi
  log "$name"
  mkdir -p "$dir"
  rtc_conf "$dir" ConsoleOut in "$TOPIC"
  capture_start "$dir"
  (cd "$dir" && timeout -s INT $((TIMEOUT + 10)) "$CONSOLE_OUT" -f rtc.conf >console_out.txt 2>&1) &
  local console=$!
  # shellcheck disable=SC2086
  timeout $((TIMEOUT + 5)) ${PROGRAMS[$impl]} out "$DOMAIN" $((COUNT * 4)) 200 "$TOPIC" >"$dir/writer.log" 2>&1
  local rc=$?
  sleep 1
  pkill -INT -f "$CONSOLE_OUT -f rtc.conf" 2>/dev/null
  wait "$console" 2>/dev/null
  capture_stop
  local got
  got=$(grep -cE "^Received: [0-9]+" "$dir/console_out.txt")
  [ "$rc" -eq 0 ] && [ "$got" -ge "$COUNT" ] && result "$name" 1 || result "$name" 0
}

for impl in rust cpp python; do
  [ -n "${PROGRAMS[$impl]:-}" ] || continue
  openrtm_to "$impl"
  to_openrtm "$impl"
done

printf '%s\n' "${RESULTS[@]}" | tee "$OUT/results.txt"
[ ${#FAILED[@]} -eq 0 ]
