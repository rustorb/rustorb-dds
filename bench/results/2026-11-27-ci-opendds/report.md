The `bench/report.py` output of CI job `bench-opendds`, run 45934917671
(rustorb-dds f01bc33, PR #123), copied from the job's log: GitHub's
ubuntu-24.04 runner (4 CPUs), OpenDDS 3.31 of ghcr.io/opendds/opendds:DDS-3.31
built in its image and run on the runner (bench/peers.py `opendds`).

    bench/run.py --peers rustorb,opendds --transports udp --interop \
      --sizes 32,1024,16384,65536,1048576 --warmup 0.5 --duration 3 --repeat 1

Warm-up 0.5 s, 3.0 s measured, one run of each.

### Latency, reliable: round trip, median (µs)

| size | rustorb udp | opendds udp | rustorb↔opendds udp | opendds↔rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 67.2 | 296.9 | 728.3 | 695.2 |
| 1 KiB | 71.1 | 295.7 | 1,991 | 2,048 |
| 16 KiB | 84.6 | 336.8 | 2,431 | 2,520 |
| 64 KiB | 154.5 | 598.1 | 1,743 | 1,776 |
| 1 MiB | 1,371 | 3,172 | 5,029 | 5,124 |

### Latency, reliable: round trip, 99th percentile (µs)

| size | rustorb udp | opendds udp | rustorb↔opendds udp | opendds↔rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 90.2 | 361.9 | 1,760 | 1,801 |
| 1 KiB | 93.6 | 366.0 | 3,272 | 2,503 |
| 16 KiB | 119.2 | 411.6 | 3,945 | 3,019 |
| 64 KiB | 207.8 | 696.6 | 3,001 | 2,897 |
| 1 MiB | 1,591 | 824,421 | 8,077 | 7,871 |

### Throughput, reliable (Mbit/s of payload received)

| size | rustorb udp | opendds udp | rustorb→opendds udp | opendds→rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 43.3 | 0.0 | 0.4 | 0.0 |
| 1 KiB | 1,251 | 0.8 | 11.6 | 0.8 |
| 16 KiB | 13,447 | 16.3 | 185.1 | 13.1 |
| 64 KiB | 12,671 | 61.0 | 742.0 | 52.4 |
| 1 MiB | 11,898 | 813.7 | 1,448 | 838.9 |

### Latency, best-effort: round trip, median (µs)

| size | rustorb udp | opendds udp | rustorb↔opendds udp | opendds↔rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 46.9 | 267.4 | 140.0 | 142.3 |
| 1 KiB | 49.7 | 275.6 | 141.0 | 145.8 |
| 16 KiB | 71.1 | 303.5 | 165.1 | 161.6 |
| 64 KiB | 155.1 | 508.5 | 339.8 | 327.1 |
| 1 MiB | 1,468 | lost | 4,077 | 3,483 |

### Latency, best-effort: round trip, 99th percentile (µs)

| size | rustorb udp | opendds udp | rustorb↔opendds udp | opendds↔rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 84.0 | 315.9 | 184.2 | 185.2 |
| 1 KiB | 87.4 | 351.4 | 189.1 | 192.5 |
| 16 KiB | 114.3 | 371.9 | 214.8 | 216.7 |
| 64 KiB | 213.5 | 574.8 | 416.4 | 467.8 |
| 1 MiB | 1,631 | lost | 4,377 | 3,730 |

### Throughput, best-effort (Mbit/s of payload received)

| size | rustorb udp | opendds udp | rustorb→opendds udp | opendds→rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 59.1 | 3.3 | 3.5 | 7.0 |
| 1 KiB | 1,747 | 104.5 | 119.5 | 267.4 |
| 16 KiB | 15,262 | 1,561 | 1,915 | 3,108 |
| 64 KiB | 14,302 | 4,988 | 4,248 | 5,385 |
| 1 MiB | 11,582 | 4,052 | 2,254 | 8,087 |

Samples lost, best-effort:

| size | rustorb udp | opendds udp | rustorb→opendds udp | opendds→rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 0 | 45127 | 823359 | 0 |
| 1 KiB | 0 | 26588 | 767830 | 0 |
| 16 KiB | 600 | 29602 | 508992 | 0 |
| 64 KiB | 3497 | 4359 | 101847 | 0 |
| 1 MiB | 0 | 1489 | 5881 | 0 |

### CPU time and memory, latency runs

| case | CPU s (measuring side) | CPU s (other side) | max RSS MiB (measuring) | max RSS MiB (other) |
|---|---:|---:|---:|---:|
| opendds udp best-effort | 9.8 | 9.9 | 42.9 | 42.6 |
| opendds udp reliable | 9.3 | 9.1 | 217.8 | 210.9 |
| opendds↔rustorb udp best-effort | 15.7 | 4.2 | 44.8 | 11.7 |
| opendds↔rustorb udp reliable | 18.0 | 1.4 | 243.0 | 12.8 |
| rustorb udp best-effort | 10.4 | 10.5 | 11.2 | 11.5 |
| rustorb udp reliable | 10.9 | 11.3 | 12.0 | 11.7 |
| rustorb↔opendds udp best-effort | 4.3 | 15.6 | 11.6 | 43.6 |
| rustorb↔opendds udp reliable | 1.3 | 18.1 | 12.4 | 154.3 |

### CPU time and memory, throughput runs

| case | CPU s (measuring side) | CPU s (other side) | max RSS MiB (measuring) | max RSS MiB (other) |
|---|---:|---:|---:|---:|
| opendds udp best-effort | 4.4 | 5.0 | 37.9 | 37.2 |
| opendds udp reliable | 0.1 | 0.1 | 39.2 | 39.0 |
| opendds→rustorb udp best-effort | 1.6 | 3.5 | 6.1 | 37.1 |
| opendds→rustorb udp reliable | 0.0 | 0.1 | 5.0 | 39.0 |
| rustorb udp best-effort | 4.5 | 3.5 | 33.9 | 5.1 |
| rustorb udp reliable | 4.5 | 4.0 | 7.6 | 9.4 |
| rustorb→opendds udp best-effort | 4.3 | 3.5 | 44.2 | 5.0 |
| rustorb→opendds udp reliable | 3.8 | 0.1 | 40.1 | 6.5 |
