# Performance measurements

The measurements of rustorb-dds#100: round-trip latency and throughput
of rustorb-dds and other DDS implementations on one host. The results
and how to read them are in [docs/benchmark.md](../docs/benchmark.md).

| Peer | Program | Where it comes from |
|---|---|---|
| `rustorb` | `rustorb-dds-perf` ([crates/rustorb-dds-perf](../crates/rustorb-dds-perf)) | this repository |
| `cyclonedds` | [`cyclonedds/perf.c`](cyclonedds/perf.c), type code from `idlc` | Ubuntu 24.04 `cyclonedds-dev` (0.10.4) |
| `fastdds` | [`fastdds/perf.cpp`](fastdds/perf.cpp), type code from `fastddsgen` | Ubuntu 24.04 `libfastrtps-dev`, `fastddsgen` (2.11.2) |
| `cyclonedds-ddsperf` | Cyclone DDS's own `ddsperf`, topic `KS` | Ubuntu 24.04 `cyclonedds-tools` |

The first three take the same options, follow the same steps and print
the same JSON (see [crates/rustorb-dds-perf/README.md](../crates/rustorb-dds-perf/README.md)),
for the type of [`idl/Perf.idl`](idl/Perf.idl), so they are measured
alike and can be paired with each other (`--interop`). As in
`interop/`, the other implementations are black boxes: the programs use
their public APIs only.

## Running

```sh
sudo apt-get install cmake g++ cyclonedds-dev cyclonedds-tools \
  libfastrtps-dev fastddsgen iceoryx
bench/run.py                                 # every peer found, every case, 3 times
bench/run.py --peers rustorb --kinds latency # some of them
bench/run.py --interop                       # also rustorb with each other peer
bench/run.py --only 'throughput-.*-shm-'     # the cases whose name matches
bench/run.py --list                          # the case names
bench/report.py bench/results/<run> --svg docs/benchmark
```

`run.py` builds the programs (into `target/bench`), then runs each case
as two processes, the measuring one (ping, sub) on one half of the CPUs
and the other (pong, pub) on the other half (`--no-pin` not to), and
appends a JSON line for each result to `<out>/results.jsonl`, with the
user and system CPU time and the largest resident set of both processes
(from `wait4`). `<out>/machine.json` records the CPU, the kernel, the
versions and the options, and `<out>/log.txt` every command and its
output. `report.py` prints Markdown tables of the medians over the
repeats, and how much the repeats differed.

- Transports: `udp` (rustorb-dds and Fast DDS without their shared
  memory; Cyclone DDS's default), `shm` (rustorb-dds's shared memory;
  Fast DDS's default builtin transports; Cyclone DDS with iceoryx, for
  which `run.py` starts `iox-roudi`), `loan` (rustorb-dds's loaned
  samples). Between two implementations only `udp` is measured.
- Latency is the round trip, ping to pong and back, for 32 B to 1 MiB of
  payload (2× steps) in one process pair; ddsperf takes one size per
  process and gives only per-second percentiles, so its rows are fewer
  and their percentiles are medians of those.
- Throughput is what the subscriber received over `--duration` seconds,
  best-effort or reliable (KEEP_ALL, 100 samples), for 32 B to 1 MiB.

The numbers of a shared machine (a container, a CI runner) vary between
runs: compare the cases of one run with each other, not with another
run. The CI job `bench` runs every case for one second to check that
everything still works, and does not look at the numbers.
