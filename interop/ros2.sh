#!/usr/bin/env bash
# Runs ROS 2's demo talker and listener (demo_nodes_cpp) against the
# rustorb-dds `ros2` example, in both directions, with each ROS 2
# middleware given, on this host.
#
#   interop/ros2.sh
#
# Needs a ROS 2 installation sourced (setup.bash), with demo_nodes_cpp
# and the middlewares in RMWS; see interop/README.md. ROS 2 is used as a
# black box: its programs, as distributed.
#
# Environment:
#   RMWS     the ROS 2 middlewares (RMW_IMPLEMENTATION) to run with
#            (default: rmw_fastrtps_cpp rmw_cyclonedds_cpp)
#   RUST     the rustorb-dds `ros2` example to run (default: built with
#            cargo)
#   OUT      where to write the logs (default target/interop-ros2)
#   COUNT    messages each listener must hear (default 5)
#   TIMEOUT  seconds a listener waits (default 30)
#   ONLY     run only the cases whose name matches this extended regular
#            expression (default: all)

set -u
cd "$(dirname "$0")/.."
ROOT=$(pwd)
OUT=${OUT:-$ROOT/target/interop-ros2}
COUNT=${COUNT:-5}
TIMEOUT=${TIMEOUT:-30}
read -r -a RMW_LIST <<<"${RMWS:-rmw_fastrtps_cpp rmw_cyclonedds_cpp}"
mkdir -p "$OUT"

log() { echo "[ros2] $*"; }

if [ -z "${AMENT_PREFIX_PATH:-}" ]; then
  echo "ROS 2 is not sourced: source /opt/ros/<distro>/setup.bash first" >&2
  exit 2
fi
# ROS_DISTRO comes from the package ros_environment, which a minimal
# installation may not have: else take it from /opt/ros/<distro>.
if [ -z "${ROS_DISTRO:-}" ]; then
  ROS_DISTRO=$(echo "$AMENT_PREFIX_PATH" | tr ':' '\n' | sed -n 's|^/opt/ros/\([^/]*\)$|\1|p' | head -n 1)
fi
DEMO=$(echo "$AMENT_PREFIX_PATH" | tr ':' '\n' | while read -r p; do
  [ -x "$p/lib/demo_nodes_cpp/talker" ] && echo "$p/lib/demo_nodes_cpp" && break
done)
if [ -z "$DEMO" ]; then
  echo "demo_nodes_cpp is not installed" >&2
  exit 2
fi

{
  echo "date: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "host: $(uname -srm)"
  [ -r /etc/os-release ] && . /etc/os-release && echo "os: $PRETTY_NAME"
  echo "rustorb-dds: $(git rev-parse --short HEAD 2>/dev/null || echo unknown)"
  echo "ros: $ROS_DISTRO"
  for p in demo-nodes-cpp rmw-fastrtps-cpp rmw-cyclonedds-cpp fastrtps cyclonedds; do
    v=$(dpkg-query -W -f='${Version}' "ros-$ROS_DISTRO-$p" 2>/dev/null) && [ -n "$v" ] && echo "ros-$ROS_DISTRO-$p: $v"
  done
} > "$OUT/versions.txt"
cat "$OUT/versions.txt"

if [ -z "${RUST:-}" ]; then
  log "building the rustorb-dds ros2 example"
  cargo build --quiet --example ros2 || exit 1
  RUST="$ROOT/target/debug/examples/ros2"
fi

DOMAIN=110
FAILED=()
RESULTS=()

result() {
  local name=$1 ok=$2
  if [ "$ok" = 1 ]; then
    RESULTS+=("PASS $name")
  else
    RESULTS+=("FAIL $name")
    FAILED+=("$name")
    tail -n 20 "$OUT/$name"/*.log
  fi
}

# A ROS 2 talker, a rustorb-dds listener: the listener must hear COUNT
# messages "Hello World: <n>".
ros_to_rustorb() {
  local rmw=$1 name="$1-to-rustorb" dir="$OUT/$1-to-rustorb"
  DOMAIN=$((DOMAIN + 1))
  if [ -n "${ONLY:-}" ] && ! [[ $name =~ $ONLY ]]; then return; fi
  log "$name (domain $DOMAIN)"
  mkdir -p "$dir"
  RMW_IMPLEMENTATION=$rmw ROS_DOMAIN_ID=$DOMAIN \
    timeout -s INT $((TIMEOUT + 10)) "$DEMO/talker" >"$dir/talker.log" 2>&1 &
  local talker=$!
  timeout $((TIMEOUT + 5)) "$RUST" listener "$DOMAIN" "$COUNT" "$TIMEOUT" >"$dir/listener.log" 2>&1
  local rc=$?
  kill -INT "$talker" 2>/dev/null
  wait "$talker" 2>/dev/null
  local heard
  heard=$(grep -c "I heard: \[Hello World: [0-9]*\]" "$dir/listener.log")
  [ "$rc" -eq 0 ] && [ "$heard" -ge "$COUNT" ] && result "$name" 1 || result "$name" 0
}

# A rustorb-dds talker, a ROS 2 listener: the listener must hear all
# COUNT messages, 1 to COUNT, in order.
rustorb_to_ros() {
  local rmw=$1 name="rustorb-to-$1" dir="$OUT/rustorb-to-$1"
  DOMAIN=$((DOMAIN + 1))
  if [ -n "${ONLY:-}" ] && ! [[ $name =~ $ONLY ]]; then return; fi
  log "$name (domain $DOMAIN)"
  mkdir -p "$dir"
  RMW_IMPLEMENTATION=$rmw ROS_DOMAIN_ID=$DOMAIN RCUTILS_LOGGING_BUFFERED_STREAM=0 \
    timeout -s INT $((TIMEOUT + 10)) "$DEMO/listener" >"$dir/listener.log" 2>&1 &
  local listener=$!
  timeout $((TIMEOUT + 5)) "$RUST" talker "$DOMAIN" "$COUNT" 200 >"$dir/talker.log" 2>&1
  local rc=$?
  sleep 1
  kill -INT "$listener" 2>/dev/null
  wait "$listener" 2>/dev/null
  local heard want
  heard=$(grep -o "I heard: \[Hello World: [0-9]*\]" "$dir/listener.log" | grep -o "[0-9]*\]" | tr -d ']' | tr '\n' ' ')
  want=$(seq -s ' ' 1 "$COUNT")
  [ "$rc" -eq 0 ] && [ "$heard" = "$want " ] && result "$name" 1 || result "$name" 0
}

for rmw in "${RMW_LIST[@]}"; do
  ros_to_rustorb "$rmw"
  rustorb_to_ros "$rmw"
done

printf '%s\n' "${RESULTS[@]}" | tee "$OUT/results.txt"
[ ${#FAILED[@]} -eq 0 ]
