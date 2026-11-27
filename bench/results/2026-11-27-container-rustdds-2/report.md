Run: `rustdds2`
on Intel(R) Xeon(R) Processor @ 2.10GHz (4 CPUs), Linux-6.18.44-fc-v880-x86_64-with-glibc2.39, in a container; rustorb-dds a5a563b; rustorb 0.3.0, rustdds 0.15.1.
Warm-up 1.0 s, 5.0 s measured, median of 3 runs.

### Latency, reliable: round trip, median (µs)

| size | rustorb udp | rustdds udp | rustorb↔rustdds udp | rustdds↔rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 88.4 | 100.0 | 96.9 | 101.0 |
| 64 B | 85.4 | 99.4 | 96.3 | 100.6 |
| 128 B | 84.3 | 97.0 | 91.6 | 99.7 |
| 256 B | 83.8 | 98.0 | 93.1 | 98.5 |
| 512 B | 85.6 | 97.5 | 96.6 | 97.6 |
| 1 KiB | 88.9 | 99.5 | 97.7 | 101.7 |
| 2 KiB | 89.5 | 108.1 | 126.0 | 135.3 |
| 4 KiB | 89.7 | 125.0 | 130.4 | 140.5 |
| 8 KiB | 97.5 | 152.8 | 142.5 | 150.3 |
| 16 KiB | 105.5 | 210.0 | 171.5 | 179.6 |
| 32 KiB | 124.7 | 411.6 | 251.4 | 278.3 |
| 64 KiB | 199.7 | 586.4 | 408.1 | 470.8 |
| 128 KiB | 454.4 | 944.4 | 705.9 | 728.8 |
| 256 KiB | 737.5 | 1,681 | 1,296 | 1,473 |
| 512 KiB | 1,955 | 3,041 | 2,719 | 2,703 |
| 1 MiB | 2,093 | 5,829 | 5,006 | 4,650 |

In this table the runs of a cell differed by 9 % (median over the cells), 40 % at most.

### Latency, reliable: round trip, 99th percentile (µs)

| size | rustorb udp | rustdds udp | rustorb↔rustdds udp | rustdds↔rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 231.2 | 320.1 | 246.3 | 264.9 |
| 64 B | 186.3 | 276.4 | 252.1 | 247.1 |
| 128 B | 191.6 | 271.7 | 222.9 | 292.6 |
| 256 B | 210.0 | 259.3 | 214.8 | 222.7 |
| 512 B | 203.6 | 268.1 | 247.2 | 255.1 |
| 1 KiB | 201.3 | 232.3 | 258.7 | 275.0 |
| 2 KiB | 183.2 | 333.9 | 302.5 | 335.9 |
| 4 KiB | 186.6 | 368.7 | 305.2 | 330.8 |
| 8 KiB | 290.7 | 539.9 | 313.6 | 368.4 |
| 16 KiB | 259.2 | 804.0 | 412.0 | 431.3 |
| 32 KiB | 389.1 | 1,091 | 730.8 | 800.0 |
| 64 KiB | 449.1 | 1,413 | 936.9 | 1,138 |
| 128 KiB | 875.2 | 1,941 | 1,587 | 1,714 |
| 256 KiB | 1,233 | 3,577 | 3,008 | 2,917 |
| 512 KiB | 2,873 | 7,685 | 5,586 | 5,154 |
| 1 MiB | 3,197 | 10,583 | 10,163 | 10,016 |

In this table the runs of a cell differed by 22 % (median over the cells), 63 % at most.

### Throughput, reliable (Mbit/s of payload received)

| size | rustorb udp | rustdds udp | rustorb→rustdds udp | rustdds→rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 27.7 | 32.3 | 16.3 | 73.0 |
| 256 B | 217.9 | 201.9 | 138.4 | 356.8 |
| 1 KiB | 814.5 | 415.1 | 543.9 | 540.8 |
| 4 KiB | 3,115 | 929.4 | 2,000 | 1,159 |
| 16 KiB | 9,766 | 2,287 | 7,216 | 2,040 |
| 64 KiB | 9,872 | 1,196 | 7,881 | 0.0 |
| 256 KiB | 8,873 | 622.4 | 9,482 | 50.8 |
| 1 MiB | 10,001 | 5.0 | 266.8 | 365.7 |

In this table the runs of a cell differed by 13 % (median over the cells), 3333 % at most.

Samples lost, reliable:

| size | rustorb udp | rustdds udp | rustorb→rustdds udp | rustdds→rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 0 | 3298 | 1041 | 0 |
| 256 B | 0 | 1749 | 652 | 0 |
| 1 KiB | 0 | 1402 | 457 | 0 |
| 4 KiB | 0 | 67 | 667 | 0 |
| 16 KiB | 0 | 0 | 932 | 0 |
| 64 KiB | 0 | 79 | 696 | 0 |
| 256 KiB | 0 | 38 | 953 | 0 |
| 1 MiB | 0 | 0 | 90 | 0 |

### Latency, best-effort: round trip, median (µs)

| size | rustorb udp | rustdds udp | rustorb↔rustdds udp | rustdds↔rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 82.8 | 118.0 | 96.9 | 98.0 |
| 64 B | 86.5 | 118.6 | 100.2 | 98.2 |
| 128 B | 85.3 | 116.6 | 103.3 | 97.2 |
| 256 B | 82.1 | 119.3 | 102.0 | 97.6 |
| 512 B | 84.2 | 118.9 | 103.0 | 97.6 |
| 1 KiB | 82.7 | 121.3 | 103.0 | 99.2 |
| 2 KiB | 84.2 | 131.9 | 128.4 | 120.1 |
| 4 KiB | 86.8 | 149.1 | 134.0 | 126.2 |
| 8 KiB | 92.2 | 163.4 | 142.2 | 131.7 |
| 16 KiB | 100.7 | 207.5 | 175.8 | 159.0 |
| 32 KiB | 119.9 | 356.2 | 260.9 | 240.0 |
| 64 KiB | 203.6 | 559.6 | 466.2 | 409.2 |
| 128 KiB | 425.7 | 853.8 | 753.1 | 712.5 |
| 256 KiB | 945.9 | 1,661 | 1,390 | 1,471 |
| 512 KiB | 1,602 | 2,904 | 2,887 | 2,421 |
| 1 MiB | 2,081 | 5,046 | 4,561 | 4,292 |

In this table the runs of a cell differed by 8 % (median over the cells), 33 % at most.

### Latency, best-effort: round trip, 99th percentile (µs)

| size | rustorb udp | rustdds udp | rustorb↔rustdds udp | rustdds↔rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 190.2 | 240.1 | 233.1 | 246.4 |
| 64 B | 189.8 | 290.2 | 214.3 | 246.9 |
| 128 B | 190.9 | 250.3 | 223.5 | 241.6 |
| 256 B | 205.1 | 280.1 | 216.9 | 218.6 |
| 512 B | 201.0 | 234.4 | 214.8 | 269.0 |
| 1 KiB | 221.3 | 288.9 | 236.4 | 235.0 |
| 2 KiB | 190.5 | 331.9 | 294.8 | 320.0 |
| 4 KiB | 232.9 | 383.0 | 321.9 | 378.3 |
| 8 KiB | 234.9 | 402.0 | 313.1 | 344.2 |
| 16 KiB | 352.8 | 510.8 | 459.9 | 385.0 |
| 32 KiB | 459.9 | 798.7 | 621.8 | 542.1 |
| 64 KiB | 620.3 | 1,062 | 1,051 | 873.3 |
| 128 KiB | 924.3 | 1,488 | 1,598 | 1,541 |
| 256 KiB | 1,579 | 2,394 | 3,129 | 3,019 |
| 512 KiB | 3,007 | 4,547 | 5,388 | 5,317 |
| 1 MiB | 3,431 | 8,384 | 7,692 | 8,256 |

In this table the runs of a cell differed by 26 % (median over the cells), 132 % at most.

### Throughput, best-effort (Mbit/s of payload received)

| size | rustorb udp | rustdds udp | rustorb→rustdds udp | rustdds→rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 48.4 | 2.1 | 53.3 | 5.2 |
| 256 B | 546.3 | 24.7 | 418.6 | 30.7 |
| 1 KiB | 1,610 | 65.9 | 1,033 | 40.2 |
| 4 KiB | 5,286 | 241.9 | 4,004 | 131.4 |
| 16 KiB | 9,946 | 2,715 | 8,784 | 2,471 |
| 64 KiB | 8,993 | 3,644 | 7,399 | 2,653 |
| 256 KiB | 10,415 | 4,006 | – | 2,148 |
| 1 MiB | 11,117 | 3,178 | 12,278 | 1,143 |

In this table the runs of a cell differed by 15 % (median over the cells), 1106 % at most.

Samples lost, best-effort:

| size | rustorb udp | rustdds udp | rustorb→rustdds udp | rustdds→rustorb udp |
|---:|---:|---:|---:|---:|
| 32 B | 766747 | 2217290 | 853952 | 2375763 |
| 256 B | 258925 | 1643329 | 622396 | 2362893 |
| 1 KiB | 319165 | 2543856 | 917942 | 2250342 |
| 4 KiB | 352413 | 2732206 | 861169 | 2527839 |
| 16 KiB | 271591 | 2513217 | 608190 | 2483090 |
| 64 KiB | 119454 | 613962 | 207243 | 707088 |
| 256 KiB | 25210 | 185114 | – | 184801 |
| 1 MiB | 1278 | 24967 | 2718 | 25038 |

### Killed for memory

These runs were stopped when a side's resident set grew past the limit; they have no numbers.

- `throughput-rustdds-rustorb-udp-best-effort-16384`: the measuring side, over 3072 MiB
- `throughput-rustdds-rustorb-udp-best-effort-262144`: the measuring side, over 3072 MiB
- `throughput-rustdds-rustorb-udp-best-effort-65536`: the measuring side, over 3072 MiB

### CPU time and memory, latency runs

| case | CPU s (measuring side) | CPU s (other side) | max RSS MiB (measuring) | max RSS MiB (other) |
|---|---:|---:|---:|---:|
| rustdds udp best-effort | 55.7 | 56.0 | 84.6 | 81.9 |
| rustdds udp reliable | 70.8 | 71.3 | 1,045 | 1,042 |
| rustdds↔rustorb udp best-effort | 55.3 | 55.9 | 85.5 | 11.6 |
| rustdds↔rustorb udp reliable | 61.7 | 64.8 | 1,285 | 12.0 |
| rustorb udp best-effort | 46.6 | 47.2 | 11.9 | 12.2 |
| rustorb udp reliable | 49.6 | 52.3 | 12.2 | 12.1 |
| rustorb↔rustdds udp best-effort | 55.9 | 56.2 | 11.6 | 84.8 |
| rustorb↔rustdds udp reliable | 65.5 | 63.1 | 11.7 | 1,245 |

### CPU time and memory, throughput runs

| case | CPU s (measuring side) | CPU s (other side) | max RSS MiB (measuring) | max RSS MiB (other) |
|---|---:|---:|---:|---:|
| rustdds udp best-effort | 5.0 | 27.3 | 29.8 | 55.2 |
| rustdds udp reliable | 6.9 | 7.1 | 22.9 | 382.4 |
| rustdds→rustorb udp best-effort | 4.5 | 21.6 | 9.2 | 49.7 |
| rustdds→rustorb udp reliable | 5.6 | 7.2 | 9.2 | 532.9 |
| rustorb udp best-effort | 9.0 | 5.9 | 34.2 | 4.9 |
| rustorb udp reliable | 7.9 | 6.5 | 18.7 | 9.8 |
| rustorb→rustdds udp best-effort | 8.3 | 6.0 | 128.6 | 4.9 |
| rustorb→rustdds udp reliable | 8.1 | 6.8 | 17.9 | 8.6 |
