# rustorb-dds-perf

Latency and throughput measurements of rustorb-dds, in the manner of
Cyclone DDS's `ddsperf` (rustorb-dds#101). It is a tool of this
repository, used by `bench/run.py` (see `bench/README.md` and
`docs/benchmark.md`), and is not published.

```sh
cargo build --release -p rustorb-dds-perf
target/release/rustorb-dds-perf pong &
target/release/rustorb-dds-perf ping --sizes 32,1024,65536 --duration 3

target/release/rustorb-dds-perf sub --size 4096 &
target/release/rustorb-dds-perf pub --size 4096
```

- `ping` writes a sample on `RustorbPerf_ping`, waits for `pong` to send
  it back on `RustorbPerf_pong`, and records the round trip, for each
  size in turn: a warm-up (`--warmup`, left out), then `--duration`
  seconds. A round trip without an answer within `--reply-timeout` is
  counted as lost.
- `pub` writes on `RustorbPerf_data` as fast as it can (or at `--rate`);
  a reliable writer is KEEP_ALL with at most `--depth` (100) samples, so
  it is held back by the acknowledgments. `sub` counts what it receives
  in `--duration` seconds after a warm-up, and the samples lost (gaps in
  the sequence numbers). It takes at most 256 samples at a time; a
  best-effort sub keeps at most 1000 not yet taken (KEEP_ALL with
  `max_samples`), so one slower than its pub loses the rest.
- `--transport udp` turns the shared memory off, `shm` (the default)
  uses it to the rustorb-dds participants of the host, `loan` writes
  loaned samples of `RustorbPerf::FixedN` (N a power of two, 32 to
  1 MiB) whose payload is not touched.
- `--json` prints one line of JSON for each result.

The type is `RustorbPerf::Sample` of `bench/idl/Perf.idl`; the programs
in `bench/cyclonedds` and `bench/fastdds` take the same options and
print the same JSON, so that they can be measured the same way and talk
to this one.
