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
#   RELIABLE   samples the reliable cases send; the subscriber must get
#              all of them, in order (default 50)
#   KEYED      samples the keyed cases send (default 30)
#   LARGE      samples the large cases send (default 10)
#   LARGE_SIZE the size of their message, in octets: more than a datagram
#              holds, so that they go as DATA_FRAG (default 300000)
#   ONLY       run only the cases whose name matches this extended
#              regular expression (default: all)
#   LEASE      the liveliness lease of the liveliness cases, in ms
#              (default 1000)
#   DROP       in the reliable cases, rustorb-dds drops one in DROP user
#              datagrams it receives, at random, so that repairs are
#              exercised in both directions (default 5; 0 drops nothing)

set -u
cd "$(dirname "$0")/.."
ROOT=$(pwd)
OUT=${OUT:-$ROOT/target/interop}
BUILD=$OUT/build
COUNT=${COUNT:-3}
SEND=${SEND:-20}
INTERVAL=${INTERVAL:-100}
TIMEOUT=${TIMEOUT:-30}
RELIABLE=${RELIABLE:-50}
KEYED=${KEYED:-30}
LARGE=${LARGE:-10}
LARGE_SIZE=${LARGE_SIZE:-300000}
LEASE=${LEASE:-1000}
DROP=${DROP:-5}
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

log "building rustorb-dds hello_world, keyed and liveliness"
cargo build --quiet --example hello_world --example keyed --example liveliness || exit 1
RUST="$ROOT/target/debug/examples/hello_world"
RUST_KEYED="$ROOT/target/debug/examples/keyed"
RUST_LIVELINESS="$ROOT/target/debug/examples/liveliness"

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
        (cd "$dir/gen" && fastddsgen -replace -d . "$ROOT/interop/idl/HelloWorld.idl" "$ROOT/interop/idl/Keyed.idl") \
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
  if [ -n "${ONLY:-}" ] && ! [[ $name =~ $ONLY ]]; then return; fi
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
  # Reliable: every sample, in order, with losses on the rustorb side.
  run_case "$peer-to-rustorb-reliable" \
    "$P" pub @D@ "$RELIABLE" 20 reliable -- \
    env RUSTORB_DROP_ONE_IN="$DROP" \
    timeout $((TIMEOUT + 5)) "$RUST" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
  run_case "rustorb-to-$peer-reliable" \
    env RUSTORB_DROP_ONE_IN="$DROP" "$RUST" pub @D@ "$RELIABLE" 20 reliable -- \
    timeout $((TIMEOUT + 5)) "$P" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
  # TRANSIENT_LOCAL: written before the subscriber's reader exists.
  run_case "$peer-to-rustorb-transient-local" \
    "$P" pub @D@ "$RELIABLE" 0 transient_local -- \
    timeout $((TIMEOUT + 5)) "$RUST" sub @D@ "$RELIABLE" "$TIMEOUT" transient_local
  run_case "rustorb-to-$peer-transient-local" \
    "$RUST" pub @D@ "$RELIABLE" 0 transient_local -- \
    timeout $((TIMEOUT + 5)) "$P" sub @D@ "$RELIABLE" "$TIMEOUT" transient_local
  # Large samples, as DATA_FRAG. Best effort: some of them must arrive
  # whole.
  run_case "$peer-to-rustorb-large-best-effort" \
    "$P" pub @D@ "$SEND" "$INTERVAL" best_effort "$LARGE_SIZE" -- \
    timeout $((TIMEOUT + 5)) "$RUST" sub @D@ "$COUNT" "$TIMEOUT" best_effort "$LARGE_SIZE"
  run_case "rustorb-to-$peer-large-best-effort" \
    "$RUST" pub @D@ "$SEND" "$INTERVAL" best_effort "$LARGE_SIZE" -- \
    timeout $((TIMEOUT + 5)) "$P" sub @D@ "$COUNT" "$TIMEOUT" best_effort "$LARGE_SIZE"
  # Reliable, with losses on the rustorb side: lost fragments are
  # repaired.
  run_case "$peer-to-rustorb-large" \
    "$P" pub @D@ "$LARGE" 100 reliable "$LARGE_SIZE" -- \
    env RUSTORB_DROP_ONE_IN="$DROP" \
    timeout $((TIMEOUT + 5)) "$RUST" sub @D@ "$LARGE" "$TIMEOUT" reliable "$LARGE_SIZE"
  run_case "rustorb-to-$peer-large" \
    env RUSTORB_DROP_ONE_IN="$DROP" "$RUST" pub @D@ "$LARGE" 100 reliable "$LARGE_SIZE" -- \
    timeout $((TIMEOUT + 5)) "$P" sub @D@ "$LARGE" "$TIMEOUT" reliable "$LARGE_SIZE"
  # Keyed instances: writes to three instances, a dispose, an unregister.
  K=$BUILD/$peer/keyed
  run_case "$peer-to-rustorb-keyed" \
    "$K" pub @D@ "$KEYED" 20 -- \
    timeout $((TIMEOUT + 5)) "$RUST_KEYED" sub @D@ "$KEYED" "$TIMEOUT"
  run_case "rustorb-to-$peer-keyed" \
    "$RUST_KEYED" pub @D@ "$KEYED" 20 -- \
    timeout $((TIMEOUT + 5)) "$K" sub @D@ "$KEYED" "$TIMEOUT"
  # LIVELINESS: the writer stays alive while asserted, is lost when
  # silent (manual kinds) and comes back when it writes.
  L=$BUILD/$peer/liveliness
  for kind in automatic participant topic; do
    run_case "$peer-to-rustorb-liveliness-$kind" \
      "$L" pub @D@ "$kind" "$LEASE" -- \
      timeout $((TIMEOUT + 5)) "$RUST_LIVELINESS" sub @D@ "$kind" "$LEASE" "$TIMEOUT"
    run_case "rustorb-to-$peer-liveliness-$kind" \
      "$RUST_LIVELINESS" pub @D@ "$kind" "$LEASE" -- \
      timeout $((TIMEOUT + 5)) "$L" sub @D@ "$kind" "$LEASE" "$TIMEOUT"
  done
done

printf '%s\n' "${RESULTS[@]}" | tee "$OUT/results.txt"
[ ${#FAILED[@]} -eq 0 ]
