#!/usr/bin/env bash
# Runs hello world between rustorb-dds and other DDS implementations, in
# both directions, on this host.
#
#   interop/run.sh [peer...]        peers: cyclonedds fastdds opendds
#                                   (default: cyclonedds fastdds)
#
# The peers come from distribution packages, and OpenDDS from its Docker
# image or an installation (see interop/README.md); they are
# used as black boxes through their public APIs. Results, peer
# versions and, for failed cases, a packet capture go to $OUT
# (default target/interop).
#
# Environment:
#   OUT        where to write results
#   DUMP       1 to print an RTPS trace of each failed case's capture
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
#   MVN        Maven, to build the Java examples (default mvn; the Java
#              cases are skipped without it or java, 22 or newer)
#   PYTHON     the Python with the rustorb_dds package installed, for
#              the Python cases (default python3; they are skipped when
#              it cannot import rustorb_dds)
#   SECURITY   0 to skip the DDS Security cases (default 1; they need
#              openssl, and peers built with their security plugins)
#   OPENDDS_IMAGE  the OpenDDS Docker image to build and run the OpenDDS
#              programs in (default ghcr.io/opendds/opendds:DDS-3.31);
#              OPENDDS_IMAGE= with OpenDDS installed (DDS_ROOT, ACE_ROOT
#              set) builds and runs them here instead
#   DROP       in the reliable cases, rustorb-dds drops one in DROP user
#              datagrams it receives, at random, so that repairs are
#              exercised in both directions (default 5; 0 drops nothing)

set -u
cd "$(dirname "$0")/.."
ROOT=$(pwd)
OUT=${OUT:-$ROOT/target/interop}
case $OUT in /*) ;; *) OUT=$ROOT/$OUT ;; esac
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
cargo build --quiet --features rustorb-dds/security --example hello_world --example keyed \
  --example liveliness --example evolve --example dynamic || exit 1
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
MVN=${MVN:-mvn}
if command -v "$MVN" >/dev/null && command -v java >/dev/null; then
  log "building the Java binding's examples"
  if (cd "$ROOT/java" && "$MVN" -B -q -DskipTests test-compile) >"$BUILD/java.log" 2>&1; then
    HAVE_JAVA=1
    case $(uname -s) in
      Darwin) FFI_FILE=librustorb_dds_ffi.dylib ;;
      MINGW* | MSYS* | CYGWIN*) FFI_FILE=rustorb_dds_ffi.dll ;;
      *) FFI_FILE=librustorb_dds_ffi.so ;;
    esac
    JAVA_RUN=(java --enable-native-access=ALL-UNNAMED
      "-Drustorb.dds.ffi.library=$ROOT/target/release/$FFI_FILE"
      -cp "$ROOT/java/target/classes:$ROOT/java/target/test-classes")
    JAVA_HELLO=("${JAVA_RUN[@]}" examples.HelloWorldExample)
    JAVA_KEYED=("${JAVA_RUN[@]}" examples.KeyedExample)
    echo "java: $(java -version 2>&1 | head -n 1)" >> "$OUT/versions.txt"
  else
    cat "$BUILD/java.log"
    exit 1
  fi
else
  HAVE_JAVA=0
  log "no $MVN or java: skipping the Java cases"
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

# DDS Security: certificates, governance and permissions, and each side's
# configuration (interop/security/gen.sh), with RTPS protection NONE and
# ENCRYPT.
SECURITY=${SECURITY:-1}
if [ "$SECURITY" = 1 ] && ! command -v openssl >/dev/null; then
  log "no openssl: skipping the DDS Security cases"
  SECURITY=0
fi
if [ "$SECURITY" = 1 ]; then
  for k in NONE ENCRYPT; do
    "$ROOT/interop/security/gen.sh" "$OUT/security-$k" "$k" || exit 1
  done
fi
# Prefixes that run a command with DDS Security on.
secure_env() {
  echo "RUSTORB_DDS_SECURITY=$1/rustorb.properties"
  echo "CYCLONEDDS_URI=file://$1/cyclonedds.xml"
  echo "FASTRTPS_DEFAULT_PROFILES_FILE=$1/fastdds.xml"
}
mapfile -t SECURE < <(secure_env "$OUT/security-NONE")
SECURE=(env "${SECURE[@]}")
mapfile -t SECURE_RTPS < <(secure_env "$OUT/security-ENCRYPT")
SECURE_RTPS=(env "${SECURE_RTPS[@]}")

# OpenDDS: built (and run) in its Docker image, with cmake and g++ added,
# or with the OpenDDS installed here when OPENDDS_IMAGE is empty. The
# programs go to $dir/cmake; $dir/<name> runs one with the RTPS
# configuration (rtps.ini).
OPENDDS_IMAGE=${OPENDDS_IMAGE-ghcr.io/opendds/opendds:DDS-3.31}
build_opendds() {
  local src=$1 dir=$2 run=()
  if [ -n "$OPENDDS_IMAGE" ]; then
    printf 'FROM %s\nRUN (command -v cmake && command -v g++) >/dev/null || (apt-get update -qq && apt-get install -y -qq cmake g++ >/dev/null)\n' \
      "$OPENDDS_IMAGE" | docker build -q -t rustorb-interop-opendds - || return 1
    run=(docker run --rm --init --network host -e HELLO_XCDR2 -v "$ROOT:$ROOT" -v "$dir:$dir"
      rustorb-interop-opendds)
  fi
  "${run[@]}" sh -c 'env | grep -E "^(DDS_ROOT|ACE_ROOT|TAO_ROOT)=" ;
    cmake -S "$1" -B "$2" -DCMAKE_BUILD_TYPE=Release -DCMAKE_PREFIX_PATH="${DDS_ROOT:-/usr/local}" &&
    cmake --build "$2" -j "$(nproc)"' sh "$src" "$dir/cmake" || return 1
  local name
  for name in hello keyed evolve_v1 evolve_v2; do
    {
      echo '#!/bin/sh'
      printf 'exec'
      printf ' %q' "${run[@]}" "$dir/cmake/$name" -DCPSConfigFile "$src/rtps.ini"
      echo ' "$@"'
    } >"$dir/$name"
    chmod +x "$dir/$name"
  done
  if [ -n "$OPENDDS_IMAGE" ]; then
    echo "opendds: $OPENDDS_IMAGE $(docker image inspect -f '{{index .RepoDigests 0}}' "$OPENDDS_IMAGE" 2>/dev/null)" \
      >>"$OUT/versions.txt"
  fi
}

build_peer() {
  local peer=$1 src=$ROOT/interop/$1 dir=$BUILD/$1
  mkdir -p "$dir"
  case $peer in
    cyclonedds)
      cmake -S "$src" -B "$dir" -DCMAKE_BUILD_TYPE=Release >"$dir/cmake.log" 2>&1 &&
        cmake --build "$dir" >>"$dir/cmake.log" 2>&1
      ;;
    opendds)
      build_opendds "$src" "$dir" >"$dir/cmake.log" 2>&1
      ;;
    fastdds)
      mkdir -p "$dir/gen" &&
        (cd "$dir/gen" && fastddsgen -replace -d . "$ROOT/interop/idl/HelloWorld.idl" "$ROOT/interop/idl/Keyed.idl") \
          >"$dir/cmake.log" 2>&1 &&
        mkdir -p "$dir/gen-typeobject" &&
        (cd "$dir/gen-typeobject" && fastddsgen -typeobject -replace -d . "$ROOT/interop/idl/HelloWorld.idl") \
          >>"$dir/cmake.log" 2>&1 &&
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
    if [ "${DUMP:-0}" = 1 ] && [ -f "$dir/capture.pcap" ]; then
      echo "--- $name: RTPS trace (domain $DOMAIN)"
      python3 "$ROOT/interop/rtps_dump.py" "$dir/capture.pcap" "$DOMAIN" --trace 2>/dev/null | uniq | head -n "${DUMP_LINES:-200}"
      echo "--- $name: first user DATA"
      python3 "$ROOT/interop/rtps_dump.py" "$dir/capture.pcap" "$DOMAIN" --max 2 2>/dev/null | head -n 40
    fi
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
  if [ "$SECURITY" = 1 ] && [ "$peer" != opendds ]; then
    # DDS Security: authentication, access control, and discovery,
    # liveliness and data encrypted; then reliable with whole RTPS
    # messages encrypted too.
    run_case "$peer-to-rustorb-secure" \
      "${SECURE[@]}" "$P" pub @D@ "$SEND" "$INTERVAL" -- \
      "${SECURE[@]}" timeout $((TIMEOUT + 5)) "$RUST" sub @D@ "$COUNT" "$TIMEOUT"
    run_case "rustorb-to-$peer-secure" \
      "${SECURE[@]}" "$RUST" pub @D@ "$SEND" "$INTERVAL" -- \
      "${SECURE[@]}" timeout $((TIMEOUT + 5)) "$P" sub @D@ "$COUNT" "$TIMEOUT"
    run_case "$peer-to-rustorb-secure-rtps-reliable" \
      "${SECURE_RTPS[@]}" "$P" pub @D@ "$RELIABLE" 20 reliable -- \
      "${SECURE_RTPS[@]}" timeout $((TIMEOUT + 5)) "$RUST" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
    run_case "rustorb-to-$peer-secure-rtps-reliable" \
      "${SECURE_RTPS[@]}" "$RUST" pub @D@ "$RELIABLE" 20 reliable -- \
      "${SECURE_RTPS[@]}" timeout $((TIMEOUT + 5)) "$P" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
  fi
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
  # Content filtered topics (rustorb-dds#56): the subscriber reads
  # "index > N/2"; the publisher writes 1..N, reliably. A peer writer
  # that filters for the rustorb reader (from the filter it announces)
  # and one that does not must both work. Cyclone DDS 0.10.4 has no SQL
  # filters, so only its writer takes part.
  # Fast DDS's hello_filtered has the type's TypeObject, so that its
  # writer can filter for the rustorb reader.
  PF=$P; [ "$peer" = fastdds ] && PF=${P}_filtered
  # The OpenDDS peer has no filter.
  if [ "$peer" != opendds ]; then
    run_case "$peer-to-rustorb-filtered" \
      "$PF" pub @D@ "$RELIABLE" 20 reliable -- \
      env RUSTORB_DROP_ONE_IN="$DROP" \
      timeout $((TIMEOUT + 5)) "$RUST" sub @D@ "$RELIABLE" "$TIMEOUT" filtered
  fi
  if [ "$peer" = fastdds ]; then
    run_case "rustorb-to-$peer-filtered" \
      env RUSTORB_DROP_ONE_IN="$DROP" "$RUST" pub @D@ "$RELIABLE" 20 filtered -- \
      timeout $((TIMEOUT + 5)) "${P}_filtered" sub @D@ "$RELIABLE" "$TIMEOUT" filtered
  fi
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
  if [ "$peer" != fastdds ]; then
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
  if [ "$peer" != fastdds ]; then
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
    # DDS Security, configured by the PROPERTY QoS of the participant.
    if [ "$SECURITY" = 1 ] && [ "$peer" != opendds ]; then
      run_case "$peer-to-python-secure" \
        "${SECURE[@]}" "$P" pub @D@ "$SEND" "$INTERVAL" -- \
        "${SECURE[@]}" timeout $((TIMEOUT + 5)) "$PYTHON" "$PY/hello_world.py" sub @D@ "$COUNT" "$TIMEOUT"
      run_case "python-to-$peer-secure" \
        "${SECURE[@]}" "$PYTHON" "$PY/hello_world.py" pub @D@ "$SEND" "$INTERVAL" -- \
        "${SECURE[@]}" timeout $((TIMEOUT + 5)) "$P" sub @D@ "$COUNT" "$TIMEOUT"
    fi
    if [ "$peer" != fastdds ]; then
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
  if [ "$SECURITY" = 1 ] && [ "$peer" != opendds ]; then
    run_case "$peer-to-cpp-secure-rtps-reliable" \
      "${SECURE_RTPS[@]}" "$P" pub @D@ "$RELIABLE" 20 reliable -- \
      "${SECURE_RTPS[@]}" timeout $((TIMEOUT + 5)) "$CPP" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
    run_case "cpp-to-$peer-secure-rtps-reliable" \
      "${SECURE_RTPS[@]}" "$CPP" pub @D@ "$RELIABLE" 20 reliable -- \
      "${SECURE_RTPS[@]}" timeout $((TIMEOUT + 5)) "$P" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
  fi
  # The Java binding: the same.
  if [ "$HAVE_JAVA" = 1 ]; then
    run_case "$peer-to-java" \
      "$P" pub @D@ "$SEND" "$INTERVAL" -- \
      timeout $((TIMEOUT + 5)) "${JAVA_HELLO[@]}" sub @D@ "$COUNT" "$TIMEOUT"
    run_case "java-to-$peer" \
      "${JAVA_HELLO[@]}" pub @D@ "$SEND" "$INTERVAL" -- \
      timeout $((TIMEOUT + 5)) "$P" sub @D@ "$COUNT" "$TIMEOUT"
    run_case "$peer-to-java-reliable" \
      "$P" pub @D@ "$RELIABLE" 20 reliable -- \
      timeout $((TIMEOUT + 5)) "${JAVA_HELLO[@]}" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
    run_case "java-to-$peer-reliable" \
      "${JAVA_HELLO[@]}" pub @D@ "$RELIABLE" 20 reliable -- \
      timeout $((TIMEOUT + 5)) "$P" sub @D@ "$RELIABLE" "$TIMEOUT" reliable
    run_case "$peer-to-java-keyed" \
      "$BUILD/$peer/keyed" pub @D@ "$KEYED" 20 -- \
      timeout $((TIMEOUT + 5)) "${JAVA_KEYED[@]}" sub @D@ "$KEYED" "$TIMEOUT"
    run_case "java-to-$peer-keyed" \
      "${JAVA_KEYED[@]}" pub @D@ "$KEYED" 20 -- \
      timeout $((TIMEOUT + 5)) "$BUILD/$peer/keyed" sub @D@ "$KEYED" "$TIMEOUT"
  fi
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
  # The OpenDDS peer has none.
  L=$BUILD/$peer/liveliness
  [ "$peer" = opendds ] && continue
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
