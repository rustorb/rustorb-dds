# Interoperability tests

`run.sh` runs a hello world between rustorb-dds and other DDS
implementations on one host, in both directions, and checks that the
subscriber receives the samples (rustorb-dds#8), best-effort and reliable
(rustorb-dds#16), keyed instances with dispose and unregister
(rustorb-dds#17), samples too large for a datagram, sent as DATA_FRAG
(rustorb-dds#18), TRANSIENT_LOCAL late joiners and LIVELINESS
(rustorb-dds#19).

| Peer | Where it comes from | Program |
|---|---|---|
| Cyclone DDS | Ubuntu 24.04 packages `cyclonedds-dev`, `cyclonedds-tools` | `cyclonedds/hello.c`, type code from `idlc` |
| Fast DDS | Ubuntu 24.04 packages `libfastrtps-dev`, `fastddsgen` | `fastdds/hello.cpp`, type code from `fastddsgen` |

The peers are black boxes: the programs use their public APIs and the
code their generators make from `idl/HelloWorld.idl` and `idl/Keyed.idl`.
Their source code is not read (see [CONTRIBUTING.md](../CONTRIBUTING.md)).
The rustorb-dds side is the `hello_world` and `keyed` examples of the
`rustorb-dds` crate (`cyclonedds/keyed.c` and `fastdds/keyed.cpp` on the
peers' side).

## Running

```sh
sudo apt-get install cmake g++ tcpdump \
  cyclonedds-dev cyclonedds-tools libfastrtps-dev fastddsgen
interop/run.sh              # every peer
interop/run.sh fastdds      # one peer
```

Each case is a publisher and a subscriber in their own processes on a
domain of its own. The publisher waits until it sees the subscriber, waits
half a second more, then writes `SEND` samples (default 20) every
`INTERVAL` ms (default 100). The case passes when the subscriber gets
`COUNT` samples (default 3) with the right content within `TIMEOUT`
seconds (default 30). In these cases everything is best-effort, so the
first few samples may be lost while the subscriber is still matching the
writer.

The `-reliable` cases make both sides RELIABLE with KEEP_ALL history. The
publisher sends `RELIABLE` samples (default 50) every 20 ms and waits until
they are acknowledged; the subscriber must get all of them, 1, 2, 3... in
order. On the rustorb-dds side, one in `DROP` (default 5) user datagrams
received is dropped at random, so that lost DATA, HEARTBEAT and ACKNACK
must be repaired: when rustorb-dds subscribes, the peer's writer repairs;
when it publishes, its own writer does. `DROP=0` turns this off.

The `-transient-local` cases are like the reliable ones, with both
sides TRANSIENT_LOCAL, but the publisher writes its `RELIABLE` samples at
once, before there is a subscriber, then waits for one and for its
acknowledgments. The subscriber creates its reader two seconds late and
must still get all of them, in order.

The `-v<N>-to-<peer>-v<M>-<kind>` cases (Cyclone DDS only: Fast DDS
2.11.2 has no TypeLookup service) use the `evolve` programs (the `evolve`
example on the rustorb-dds side): the publisher has version N of the type
and the subscriber version M, from `idl/EvolveV1.idl` and
`idl/EvolveV2.idl`, where version 2 has one more member. `kind` is
`appendable` or `mutable`. Both sides write and take XCDR2, RELIABLE and
KEEP_ALL; the publisher writes `COUNT` samples and the subscriber checks
the members both versions have. The rustorb-dds side must have got the
peer's TypeObject with TypeLookup (`RUSTORB_EXPECT_TYPE_LOOKUP`), and the
peer gets rustorb-dds's from it.

The `-liveliness-<kind>` cases use the `liveliness` programs (the
`liveliness` example on the rustorb-dds side) with the LIVELINESS kind
`automatic`, `participant` (MANUAL_BY_PARTICIPANT) or `topic`
(MANUAL_BY_TOPIC) and a lease of `LEASE` ms (default 1000) on both
sides. The publisher writes sample 1, asserts its liveliness every
quarter lease for three leases (on the participant or on the writer;
nothing for automatic), writes sample 2, keeps silent for four leases
and writes sample 3. The subscriber checks its reader's
LIVELINESS_CHANGED status: the writer stays alive up to sample 2; after
it a manual writer becomes not alive and an automatic one stays alive;
after sample 3 the writer is alive. `ONLY=liveliness` runs only these.

The `-large` cases send hello world samples whose message is
`LARGE_SIZE` octets (default 300000), more than a UDP datagram holds, so
both sides send them as DATA_FRAG. Each message is letters that depend on
the index, and the subscriber checks it. `-large-best-effort` is like the
best-effort case (`COUNT` of `SEND` samples must arrive whole);
`-large` is like the reliable case (`LARGE` samples, default 10, every
one in order, with `DROP` losses on the rustorb-dds side, so lost
fragments are asked for again with NACK_FRAG).

The `-keyed` cases use `Keyed::Shape`, whose key is `id`, RELIABLE and
KEEP_ALL. The publisher writes `KEYED` samples (default 30) to the
instances id 1, 2, 3 in turn, then disposes id 2 and unregisters id 3
(without autodispose). The subscriber must get every sample, each
instance's in order, then see id 2 NOT_ALIVE_DISPOSED and id 3
NOT_ALIVE_NO_WRITERS.

Results go to `target/interop` (or `$OUT`):

- `versions.txt`: the versions of rustc and of the peers' packages
- `results.txt`: PASS or FAIL for each case
- `<case>/pub.log`, `<case>/sub.log`: the output of both sides
- `<case>/capture.pcap`: the UDP traffic, kept only when the case fails.
  It is taken with tcpdump when it runs as root or with sudo (`CAPTURE=0`
  turns it off). Wireshark decodes RTPS.

The CI job `interop` runs the script on Ubuntu 24.04 and keeps the results
as an artifact.

## Results

| Date | rustorb-dds | Cyclone DDS | Fast DDS | Result |
|---|---|---|---|---|
| 2026-11-03 | rustorb-dds#8 | 0.10.4 | 2.11.2 (Fast CDR 1.1.0, fastddsgen 2.3.0) | all 4 cases pass |
| 2026-11-03 | rustorb-dds#16 | 0.10.4 | 2.11.2 (Fast CDR 1.1.0, fastddsgen 2.3.0) | all 8 cases pass (DROP 5, four runs; DROP 2 once) |
| 2026-11-03 | rustorb-dds#17 | 0.10.4 | 2.11.2 (Fast CDR 1.1.0, fastddsgen 2.3.0) | all 12 cases pass |
| 2026-11-03 | rustorb-dds#18 | 0.10.4 | 2.11.2 (Fast CDR 1.1.0, fastddsgen 2.3.0) | all 20 cases pass |
| 2026-11-03 | rustorb-dds#19 (part 1) | 0.10.4 | 2.11.2 (Fast CDR 1.1.0, fastddsgen 2.3.0) | all 24 cases pass |
| 2026-11-03 | rustorb-dds#19 (part 2) | 0.10.4 | 2.11.2 (Fast CDR 1.1.0, fastddsgen 2.3.0) | all 36 cases pass |
| 2026-11-03 | rustorb-dds#20 | 0.10.4 | 2.11.2 (Fast CDR 1.1.0, fastddsgen 2.3.0) | all 36 cases pass |
| 2026-11-03 | rustorb-dds#32 | 0.10.4 | 2.11.2 (Fast CDR 1.1.0, fastddsgen 2.3.0) | all 42 cases pass |
| 2026-11-04 | rustorb-dds#33 | 0.10.4 | 2.11.2 (Fast CDR 1.1.0, fastddsgen 2.3.0) | all 50 cases pass |

Found on the way:

- Fast DDS 2.11 sends its first ACKNACK with `readerSNState.base = 0`
  and no bits, which 8.3.5.5 calls invalid. rustorb-rtps used to drop
  the message, so the SEDP data was only repaired at the next periodic
  HEARTBEAT and the first second of samples was lost. ACKNACK now takes
  an empty set with base 0 as base 1.
- Both peers accept `VENDORID_UNKNOWN` in the RTPS header and the
  `PID_KEY_HASH` that rustorb-dds sends in-line in SEDP.
- Reliable (rustorb-dds#16): Cyclone DDS 0.10.4 puts a HEARTBEAT after
  each DATA and answers an ACKNACK at once. Fast DDS 2.11.2 sends DATA
  without a HEARTBEAT and a periodic HEARTBEAT about every 3 s, so a loss
  lost DATA is repaired only after that; with heavy losses (`DROP=3`) a
  run can take long enough for its `wait_for_acknowledgments` to time
  out. Both accept DATA and HEARTBEAT addressed to their reader's
  entity id, and the INFO_TS that rustorb-dds sends before each DATA.
- A drop pattern that repeats (every n-th datagram) lines up with how the
  peers send (Cyclone DDS sends new data and a repair in two datagrams in
  a row) and loses the same kind each time, so the drops are random.
- Keyed (rustorb-dds#17): neither peer puts `PID_KEY_HASH` in a DATA
  with a value, so rustorb-dds finds the instance from the value's key
  members. For a dispose or an unregister, Cyclone DDS 0.10.4 sends a
  DATA with the K flag, `PID_STATUS_INFO` and a key payload (the key
  holder in XCDR1, `CDR_LE`) and no key hash; rustorb-dds matches the key
  payload against the instances it knows. Fast DDS 2.11.2 sends
  `PID_KEY_HASH` and `PID_STATUS_INFO` in-line with no payload. Fast DDS
  takes the wrong instance for a dispose that carries a key payload, even
  with the key hash in-line, so rustorb-dds sends the key hash and the
  status alone, which both peers accept.
- DATA_FRAG (rustorb-dds#18), seen in captures of the `-large` cases:
  Cyclone DDS 0.10.4 sends fragments of 1344 octets, ten in each
  DATA_FRAG, each followed by a HEARTBEAT_FRAG; rustorb-dds answers a
  HEARTBEAT_FRAG with a NACK_FRAG for the fragments it misses. Fast DDS
  2.11.2 sends fragments of 65324 octets, one per DATA_FRAG, followed by
  a HEARTBEAT, and no HEARTBEAT_FRAG. Both pad the last DATA_FRAG of a
  sample to a multiple of four octets, which is not part of the sample;
  rustorb-dds drops octets past `sampleSize`. Both take rustorb-dds's
  4096-octet fragments, three in a DATA_FRAG, and neither sends in-line
  QoS with fragments of an unkeyed type. Fast DDS asked with NACK_FRAG
  for fragments its socket dropped, and rustorb-dds resent only those.
- rustorb-dds asks for a 4 MiB receive buffer on its sockets: with the
  Linux default (208 KiB), a burst of fragments of one 200 KB sample
  overflowed it and best-effort samples were lost. It also asks for a
  1 MiB send buffer: on macOS the send buffer bounds the largest
  datagram, 9216 octets by default, and larger sends failed with
  `EMSGSIZE`.
- TRANSIENT_LOCAL (rustorb-dds#19): a Cyclone DDS 0.10.4 writer with
  KEEP_ALL history and TRANSIENT_LOCAL durability sent a late reader only
  its last sample (50 of 50), until its DURABILITY_SERVICE history was set
  to KEEP_ALL too: that policy, KEEP_LAST 1 by default, bounds what it
  keeps for late joiners. `cyclonedds/hello.c` sets it. Fast DDS 2.11.2
  sends the writer's whole history.
- `dds_wait_for_acks` of Cyclone DDS 0.10.4 does not wait for a reader
  that has not yet answered a HEARTBEAT; the ACKNACK that rustorb-dds
  sends when it matches a writer (before any HEARTBEAT) does not count.
  The transient-local publisher called it right after the reader matched
  and, about one run in three, it returned before any sample was sent and
  the publisher exited (rustorb-dds#20). `cyclonedds/hello.c` now waits
  half a second first, as the other cases do.
- LIVELINESS (rustorb-dds#19): Fast DDS 2.11.2 marks the payload of its
  participant messages (`ParticipantMessageData`, 9.6.3.1) `PL_CDR_LE`,
  though what follows is plain CDR; Cyclone DDS 0.10.4 marks it `CDR_LE`.
  rustorb-dds reads both and sends `CDR`. Fast DDS writes an automatic
  liveliness message every `announcement_period` (a Fast DDS setting,
  set to a third of the lease here). The locators it announces include
  some that are not UDPv4; a MANUAL_BY_TOPIC liveliness HEARTBEAT that
  cannot be sent to those is skipped, as a write's DATA is.
- XTypes in discovery (rustorb-dds#32): Cyclone DDS 0.10.4 announces
  `PID_TYPE_INFORMATION` (XCDR2, no encapsulation header, in the byte
  order of the parameter list) and `PID_DATA_REPRESENTATION` `[XCDR,
  XCDR2]` for its writers and readers, and no
  `PID_TYPE_CONSISTENCY_ENFORCEMENT` (its default). Its TypeIdentifiers
  for `HelloWorld::Greeting` and `Keyed::Shape` are those rustorb-dds
  computes, so they match by TypeInformation. Fast DDS 2.11.2 announces no
  TypeInformation and no DATA_REPRESENTATION; its readers announce
  `PID_TYPE_CONSISTENCY_ENFORCEMENT` with the default values. rustorb-dds
  takes a reader that announces its TypeInformation but no policy to have
  the default policy (DDS-XTypes 7.6.3.4.1 says to assume
  DISALLOW_TYPE_COERCION, meant for implementations without XTypes).
- XCDR2 samples (rustorb-dds#32): Cyclone DDS 0.10.4 drops, as a
  malformed message, a DATA whose encapsulation identifier is `0x0011`
  (PLAIN_CDR2 little endian in DDS-XTypes Table 39 and DDSI-RTPS 2.5
  Table 10.3), and writes `0x0007` (`CDR2_LE`, Table 60) itself.
  rustorb-cdr 0.2 writes the Table 39 values, so rustorb-dds rewrites
  them to those of Table 60 when it writes; it reads both.
- TypeLookup (rustorb-dds#33): Cyclone DDS 0.10.4 announces the four
  TypeLookup endpoints (bits 12 to 15 of `availableBuiltinEndpoints`);
  Fast DDS 2.11.2 does not, and has no TypeLookup service. Cyclone asks
  for the TypeObject of a remote type whose TypeIdentifier it does not
  know with getTypes, sent to all the request readers it matched, with the
  `instanceName` `dds.builtin.TOS.<GUID of the participant asked>` (the
  32 hexadecimal digits of the whole GUID). The payload is XCDR2 with the
  encapsulation identifier `0x0007` (Table 60); the request and reply
  structs and their headers have no DHEADER and the unions have one; the
  sequences in the MUTABLE `_In` and `_Out` structs have an EMHEADER with
  length code 5. It writes the writer GUID of a SampleIdentity as four
  32-bit integers, so its octets come byte-swapped in a little-endian
  payload, and in a reply it does not put the identity of the request it
  answers, so rustorb-dds takes a reply that matches none of its requests
  for its oldest request of the same operation to the participant that
  answers. It answers rustorb-dds's requests, and takes rustorb-dds's
  replies.

## ROS 2

`ros2.sh` runs ROS 2's demo talker and listener (`demo_nodes_cpp`, topic
`/chatter` of type `std_msgs/msg/String`) against the `ros2` example of
the `rustorb-dds` crate, in both directions, with each ROS 2 middleware:
`rmw_fastrtps_cpp` (Fast DDS) and `rmw_cyclonedds_cpp` (Cyclone DDS)
(rustorb-dds#21). ROS 2 is a black box: its programs as distributed.

```sh
source /opt/ros/jazzy/setup.bash
interop/ros2.sh
RMWS=rmw_fastrtps_cpp interop/ros2.sh    # one middleware
```

ROS 2 Jazzy comes from the ROS 2 packages for Ubuntu 24.04
(`ros-jazzy-demo-nodes-cpp`, `ros-jazzy-rmw-fastrtps-cpp`,
`ros-jazzy-rmw-cyclonedds-cpp` from packages.ros.org), or from the
`osrf/ros:jazzy-desktop` container image, which has rmw_fastrtps_cpp only.
In a container, run with `--network host` and give the example built on
the host in `RUST`. A rustorb-dds listener must hear `COUNT` messages
(default 5) from the ROS 2 talker; the ROS 2 listener must hear all
`COUNT` messages of the rustorb-dds talker, in order. The CI job `ros2`
runs the script and keeps the logs as an artifact.

What a ROS 2 (Jazzy) node puts on DDS, seen from outside (with
`ros2 topic info -v` and in captures):

- A topic `/name` is the DDS topic `rt/name` (services use `rq/` and
  `rr/`). A message type `pkg/msg/Name` is the DDS type
  `pkg::msg::dds_::Name_`; `idl/String.idl` declares `std_msgs/msg/String`
  in that form, and the example's type is what `rustorb-idl -brust
  -Wbdds_only` generates from it. The type is not keyed.
- The demo talker and listener use the default profile: RELIABLE,
  VOLATILE, KEEP_LAST(10). History depth is not sent over DDS:
  `ros2 topic info -v` shows `UNKNOWN` for it.
- Each participant announces `enclave=/;` in its USER_DATA. Each writer
  and reader announces `typehash=RIHS01_<sha256>;` in its USER_DATA, the
  hash of its type's description (`RIHS01_df668c74...6b1a18` for
  `std_msgs/msg/String`). Matching does not need either: without them,
  ROS 2 shows the endpoint's type hash as `INVALID` and its node as
  `_CREATED_BY_BARE_DDS_APP_`; with them, the hash, and the node as
  `_NODE_NAME_UNKNOWN_`. The node name and namespace come from the topic
  `ros_discovery_info` (type `rmw_dds_common::msg::dds_::ParticipantEntitiesInfo_`),
  which the example does not write.
- The demo talker starts publishing at once, so a listener that matches
  it late misses the first messages; the example's talker waits for a
  listener first.

| Date | rustorb-dds | ROS 2 | Result |
|---|---|---|---|
| 2026-11-03 | rustorb-dds#21 | Jazzy, rmw_fastrtps_cpp 8.4.4 (Fast DDS 2.14.6), `osrf/ros:jazzy-desktop` | both cases pass |
| 2026-11-03 | rustorb-dds#21 | Jazzy, rmw_fastrtps_cpp 8.4.4 (Fast DDS 2.14.7) and rmw_cyclonedds_cpp 2.2.4 (Cyclone DDS 0.10.5), CI job `ros2` | all 4 cases pass |
