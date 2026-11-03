# Interoperability tests

`run.sh` runs a hello world between rustorb-dds and other DDS
implementations on one host, in both directions, and checks that the
subscriber receives the samples (rustorb-dds#8), best-effort and reliable
(rustorb-dds#16), keyed instances with dispose and unregister
(rustorb-dds#17), samples too large for a datagram, sent as DATA_FRAG
(rustorb-dds#18), and TRANSIENT_LOCAL late joiners (rustorb-dds#19).

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
