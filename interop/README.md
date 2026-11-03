# Interoperability tests

`run.sh` runs a hello world between rustorb-dds and other DDS
implementations on one host, in both directions, and checks that the
subscriber receives the samples (rustorb-dds#8), best-effort and reliable
(rustorb-dds#16).

| Peer | Where it comes from | Program |
|---|---|---|
| Cyclone DDS | Ubuntu 24.04 packages `cyclonedds-dev`, `cyclonedds-tools` | `cyclonedds/hello.c`, type code from `idlc` |
| Fast DDS | Ubuntu 24.04 packages `libfastrtps-dev`, `fastddsgen` | `fastdds/hello.cpp`, type code from `fastddsgen` |

The peers are black boxes: the programs use their public APIs and the
code their generators make from `idl/HelloWorld.idl`. Their source code is
not read (see [CONTRIBUTING.md](../CONTRIBUTING.md)). The rustorb-dds side
is the `hello_world` example of the `rustorb-dds` crate.

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
