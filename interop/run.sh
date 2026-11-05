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
#   PYTHON     the Python with the rustorb_dds package installed, for
#              the Python cases (default python3; they are skipped when
#              it cannot import rustorb_dds)
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

log "building rustorb-dds hello_world, keyed, liveliness, evolve and dynamic"
cargo build --quiet --example hello_world --example keyed --example liveliness \
  --example evolve --example dynamic || exit 1
RUST="$ROOT/target/debug/examples/hello_world"
RUST_KEYED="$ROOT/target/debug/examples/keyed"
RUST_LIVELINESS="$ROOT/target/debug/examples/liveliness"
RUST_EVOLVE="$ROOT/target/debug/examples/evolve"
RUST_DYNAMIC="$ROOT/target/debug/examples/dynamic"
log "building the C++ binding's hello_world and keyed"
CPP_BUILD=$BUILD/cpp
mkdir -p "$CPP_BUILD"
if cmake -S "$ROOT/cpp" -B "$CPP_BUILD" -DCMAKE_BUILD_TYPE=Release >"$CPP_BUILD/cmake.log" 2>&1 &&
  cmake --build "$CPP_BUILD" --target hello_world keyed >>"$CPP_BUILD/cmake.log" 2>&1; then
  CPP="$CPP_BUILD/hello_world"
  CPP_KEYED="$CPP_BUILD/keyed"
else
  cat "$CPP_BUILD/cmake.log"
  exit 1
fi
PYTHON=${PYTHON:-python3}
PY="$ROOT/python/examples"
if "$PYTHON" -c "import rustorb_dds" 2>/dev/null; then
  HAVE_PYTHON=1
  echo "rustorb-dds (Python): $("$PYTHON" -c "import rustorb_dds; print(rustorb_dds.__version__)")" \
    >> "$OUT/versions.txt"
else
  HAVE_PYTHON=0
  log "no rustorb_dds in $PYTHON: skipping the Python cases (pip install ./python)"
fi

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
  # XTypes. rustorb announces the TypeInformation of its types (and both
  # sides match by it when the peer does too); without it, by type name.
  run_case "$peer-to-rustorb-no-type-information" \
    "$P" pub @D@ "$SEND" "$INTERVAL" -- \
    env RUSTORB_NO_TYPE_INFORMATION=1 \
    timeout $((TIMEOUT + 5)) "$RUST" sub @D@ "$COUNT" "$TIMEOUT"
  run_case "rustorb-to-$peer-no-type-information" \
    env RUSTORB_NO_TYPE_INFORMATION=1 "$RUST" pub @D@ "$SEND" "$INTERVAL" -- \
    timeout $((TIMEOUT + 5)) "$P" sub @D@ "$COUNT" "$TIMEOUT"
  # XCDR2 data (DATA_REPRESENTATION). The Fast DDS peer requests XCDR
  # only.
  if [ "$peer" = cyclonedds ]; then
    run_case "$peer-to-rustorb-xcdr2" \
      env HELLO_XCDR2=1 "$P" pub @D@ "$RELIABLE" 20 reliable -- \
      timeout $((TIMEOUT + 5)) "$RUST" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
    run_case "rustorb-to-$peer-xcdr2" \
      env RUSTORB_XCDR2=1 "$RUST" pub @D@ "$RELIABLE" 20 reliable -- \
      timeout $((TIMEOUT + 5)) "$P" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
  fi
  # Type evolution through TypeLookup: the two sides have different
  # versions of the type, and rustorb must get the peer's TypeObject from
  # it to match. Fast DDS 2.11.2 has no TypeLookup service.
  if [ "$peer" = cyclonedds ]; then
    E=$BUILD/$peer/evolve
    for kind in appendable mutable; do
      for v in "v1 v2" "v2 v1"; do
        set -- $v
        run_case "$peer-$1-to-rustorb-$2-$kind" \
          "${E}_$1" pub @D@ "$COUNT" "$kind" -- \
          env RUSTORB_EXPECT_TYPE_LOOKUP=1 \
          timeout $((TIMEOUT + 5)) "$RUST_EVOLVE" "$2" sub @D@ "$COUNT" "$TIMEOUT" "$kind"
        run_case "rustorb-$1-to-$peer-$2-$kind" \
          env RUSTORB_EXPECT_TYPE_LOOKUP=1 "$RUST_EVOLVE" "$1" pub @D@ "$COUNT" "$kind" -- \
          timeout $((TIMEOUT + 5)) "${E}_$2" sub @D@ "$COUNT" "$TIMEOUT" "$kind"
      done
      # DynamicData: rustorb is not built with the type, and gets it
      # from the peer's endpoint by TypeLookup.
      for v in v1 v2; do
        run_case "$peer-$v-to-rustorb-dynamic-$kind" \
          "${E}_$v" pub @D@ "$COUNT" "$kind" -- \
          timeout $((TIMEOUT + 5)) "$RUST_DYNAMIC" sub @D@ "$COUNT" "$TIMEOUT" "$kind"
        run_case "rustorb-dynamic-to-$peer-$v-$kind" \
          "$RUST_DYNAMIC" pub @D@ "$COUNT" "$kind" -- \
          timeout $((TIMEOUT + 5)) "${E}_$v" sub @D@ "$COUNT" "$TIMEOUT" "$kind"
      done
    done
  fi
  # The Python binding: hello world (best effort and reliable), keyed
  # instances, and DynamicData with the type from the peer.
  if [ "$HAVE_PYTHON" = 1 ]; then
    run_case "$peer-to-python" \
      "$P" pub @D@ "$SEND" "$INTERVAL" -- \
      timeout $((TIMEOUT + 5)) "$PYTHON" "$PY/hello_world.py" sub @D@ "$COUNT" "$TIMEOUT"
    run_case "python-to-$peer" \
      "$PYTHON" "$PY/hello_world.py" pub @D@ "$SEND" "$INTERVAL" -- \
      timeout $((TIMEOUT + 5)) "$P" sub @D@ "$COUNT" "$TIMEOUT"
    run_case "$peer-to-python-reliable" \
      "$P" pub @D@ "$RELIABLE" 20 reliable -- \
      timeout $((TIMEOUT + 5)) "$PYTHON" "$PY/hello_world.py" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
    run_case "python-to-$peer-reliable" \
      "$PYTHON" "$PY/hello_world.py" pub @D@ "$RELIABLE" 20 reliable -- \
      timeout $((TIMEOUT + 5)) "$P" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
    run_case "$peer-to-python-keyed" \
      "$BUILD/$peer/keyed" pub @D@ "$KEYED" 20 -- \
      timeout $((TIMEOUT + 5)) "$PYTHON" "$PY/keyed.py" sub @D@ "$KEYED" "$TIMEOUT"
    run_case "python-to-$peer-keyed" \
      "$PYTHON" "$PY/keyed.py" pub @D@ "$KEYED" 20 -- \
      timeout $((TIMEOUT + 5)) "$BUILD/$peer/keyed" sub @D@ "$KEYED" "$TIMEOUT"
    if [ "$peer" = cyclonedds ]; then
      for v in v1 v2; do
        run_case "$peer-$v-to-python-dynamic-mutable" \
          "$BUILD/$peer/evolve_$v" pub @D@ "$COUNT" mutable -- \
          timeout $((TIMEOUT + 5)) "$PYTHON" "$PY/dynamic.py" sub @D@ "$COUNT" "$TIMEOUT" mutable
        run_case "python-dynamic-to-$peer-$v-mutable" \
          "$PYTHON" "$PY/dynamic.py" pub @D@ "$COUNT" mutable -- \
          timeout $((TIMEOUT + 5)) "$BUILD/$peer/evolve_$v" sub @D@ "$COUNT" "$TIMEOUT" mutable
      done
    fi
  fi
  # The C++ binding: hello world (best effort and reliable) and keyed
  # instances.
  run_case "$peer-to-cpp" \
    "$P" pub @D@ "$SEND" "$INTERVAL" -- \
    timeout $((TIMEOUT + 5)) "$CPP" sub @D@ "$COUNT" "$TIMEOUT"
  run_case "cpp-to-$peer" \
    "$CPP" pub @D@ "$SEND" "$INTERVAL" -- \
    timeout $((TIMEOUT + 5)) "$P" sub @D@ "$COUNT" "$TIMEOUT"
  run_case "$peer-to-cpp-reliable" \
    "$P" pub @D@ "$RELIABLE" 20 reliable -- \
    timeout $((TIMEOUT + 5)) "$CPP" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
  run_case "cpp-to-$peer-reliable" \
    "$CPP" pub @D@ "$RELIABLE" 20 reliable -- \
    timeout $((TIMEOUT + 5)) "$P" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
  run_case "$peer-to-cpp-keyed" \
    "$BUILD/$peer/keyed" pub @D@ "$KEYED" 20 -- \
    timeout $((TIMEOUT + 5)) "$CPP_KEYED" sub @D@ "$KEYED" "$TIMEOUT"
  run_case "cpp-to-$peer-keyed" \
    "$CPP_KEYED" pub @D@ "$KEYED" 20 -- \
    timeout $((TIMEOUT + 5)) "$BUILD/$peer/keyed" sub @D@ "$KEYED" "$TIMEOUT"
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
