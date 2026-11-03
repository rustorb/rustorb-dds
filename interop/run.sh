#!/usr/bin/env bash
# Runs hello world between rustorb-dds and other DDS implementations, in
# both directions, on this host.
#
#   interop/run.sh [peer...]        peers: cyclonedds fastdds (default: both)
#
# The peers come from distribution packages (see interop/README.md) and
# are used as black boxes through their public APIs. Results, peer
# versions and, for failed cases, a packet capture go to $OUT
# (default target/interop).
#
# Environment:
#   OUT        where to write results
#   CAPTURE    1 to capture packets with tcpdump (default: 1 when tcpdump
#              is there and we are root or have sudo)
#   COUNT      samples the subscriber must receive (default 3)
#   SEND       samples the publisher sends (default 20)
#   INTERVAL   milliseconds between samples (default 100)
#   TIMEOUT    seconds a subscriber waits (default 30)

set -u
cd "$(dirname "$0")/.."
ROOT=$(pwd)
OUT=${OUT:-$ROOT/target/interop}
BUILD=$OUT/build
COUNT=${COUNT:-3}
SEND=${SEND:-20}
INTERVAL=${INTERVAL:-100}
TIMEOUT=${TIMEOUT:-30}
PEERS=("$@")
[ ${#PEERS[@]} -eq 0 ] && PEERS=(cyclonedds fastdds)
mkdir -p "$OUT" "$BUILD"

SUDO=
if [ "$(id -u)" -ne 0 ] && command -v sudo >/dev/null; then SUDO=sudo; fi
if [ -z "${CAPTURE:-}" ]; then
  CAPTURE=0
  if command -v tcpdump >/dev/null && { [ "$(id -u)" -eq 0 ] || [ -n "$SUDO" ]; }; then
    CAPTURE=1
  fi
fi

log() { echo "[interop] $*"; }

# --- versions --------------------------------------------------------------

{
  echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host: $(uname -srm)"
  [ -r /etc/os-release ] && . /etc/os-release && echo "os: $PRETTY_NAME"
  echo "rustorb-dds: $(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
  echo "rustc: $(rustc --version)"
  for p in cyclonedds-dev libfastrtps-dev libfastcdr-dev fastddsgen; do
    v=$(dpkg-query -W -f='${Version}' "$p" 2>/dev/null) && echo "$p: $v"
  done
} > "$OUT/versions.txt"
cat "$OUT/versions.txt"

# --- build -----------------------------------------------------------------

log "building rustorb-dds hello_world"
cargo build --quiet --example hello_world || exit 1
RUST="$ROOT/target/debug/examples/hello_world"

build_peer() {
  local peer=$1 src=$ROOT/interop/$1 dir=$BUILD/$1
  mkdir -p "$dir"
  case $peer in
    cyclonedds)
      cmake -S "$src" -B "$dir" -DCMAKE_BUILD_TYPE=Release >"$dir/cmake.log" 2>&1 &&
        cmake --build "$dir" >>"$dir/cmake.log" 2>&1
      ;;
    fastdds)
      mkdir -p "$dir/gen" &&
        (cd "$dir/gen" && fastddsgen -replace -d . "$ROOT/interop/idl/HelloWorld.idl") \
          >"$dir/cmake.log" 2>&1 &&
        cmake -S "$src" -B "$dir" -DCMAKE_BUILD_TYPE=Release -DGEN_DIR="$dir/gen" \
          >>"$dir/cmake.log" 2>&1 &&
        cmake --build "$dir" >>"$dir/cmake.log" 2>&1
      ;;
    *) echo "unknown peer $peer" >&2; return 1 ;;
  esac || { cat "$dir/cmake.log"; return 1; }
}

# --- cases -----------------------------------------------------------------

DOMAIN=60
FAILED=()
RESULTS=()

# run_case <name> <publisher command...> -- <subscriber command...>
run_case() {
  local name=$1; shift
  local pub=() sub=()
  while [ "$1" != "--" ]; do pub+=("$1"); shift; done
  shift
  sub=("$@")
  DOMAIN=$((DOMAIN + 1))
  local dir=$OUT/$name
  rm -rf "$dir"; mkdir -p "$dir"
  local cap_pid=
  if [ "$CAPTURE" = 1 ]; then
    $SUDO tcpdump -i any -U -w "$dir/capture.pcap" udp >/dev/null 2>"$dir/tcpdump.log" &
    cap_pid=$!
    sleep 1
  fi
  log "$name (domain $DOMAIN)"
  "${sub[@]/@D@/$DOMAIN}" >"$dir/sub.log" 2>&1 &
  local sub_pid=$!
  sleep 0.5
  timeout $((TIMEOUT + 10)) "${pub[@]/@D@/$DOMAIN}" >"$dir/pub.log" 2>&1
  local pub_rc=$?
  wait $sub_pid
  local sub_rc=$?
  if [ -n "$cap_pid" ]; then
    sleep 0.5
    $SUDO kill -INT "$cap_pid" 2>/dev/null
    wait "$cap_pid" 2>/dev/null
  fi
  if [ $sub_rc -eq 0 ] && [ $pub_rc -eq 0 ]; then
    RESULTS+=("PASS $name")
    rm -f "$dir/capture.pcap"
  else
    RESULTS+=("FAIL $name (publisher $pub_rc, subscriber $sub_rc)")
    FAILED+=("$name")
    tail -n 5 "$dir/pub.log" "$dir/sub.log"
  fi
}

for peer in "${PEERS[@]}"; do
  log "building $peer"
  if ! build_peer "$peer"; then
    RESULTS+=("FAIL $peer (build)")
    FAILED+=("$peer-build")
    continue
  fi
  P=$BUILD/$peer/hello
  run_case "$peer-to-rustorb" \
    "$P" pub @D@ "$SEND" "$INTERVAL" -- \
    timeout $((TIMEOUT + 5)) "$RUST" sub @D@ "$COUNT" "$TIMEOUT"
  run_case "rustorb-to-$peer" \
    "$RUST" pub @D@ "$SEND" "$INTERVAL" -- \
    timeout $((TIMEOUT + 5)) "$P" sub @D@ "$COUNT" "$TIMEOUT"
done

printf '%s\n' "${RESULTS[@]}" | tee "$OUT/results.txt"
[ ${#FAILED[@]} -eq 0 ]
