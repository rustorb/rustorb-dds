Run: `rustdds`
on Intel(R) Xeon(R) Processor @ 2.10GHz (4 CPUs), Linux-6.18.44-fc-v880-x86_64-with-glibc2.39, in a container; rustorb-dds 391bd97; rustorb 0.3.0, rustdds 0.15.1, cyclonedds 0.10.4-1.1build3.
Warm-up 1.0 s, 5.0 s measured, median of 3 runs.

### Latency, reliable: round trip, median (µs)

| size | rustorb udp | cyclonedds udp | rustdds udp | rustorb↔cyclonedds udp | rustorb↔rustdds udp | cyclonedds↔rustorb udp | rustdds↔rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 73.9 | 61.1 | 83.0 | 65.3 | 93.7 | 69.3 | 86.7 |
| 64 B | 76.5 | 62.2 | 83.6 | 67.1 | 97.6 | 70.5 | 84.9 |
| 128 B | 74.1 | 62.9 | 83.8 | 65.5 | 96.1 | 67.4 | 83.1 |
| 256 B | 75.4 | 61.8 | 83.6 | 67.2 | 91.3 | 68.7 | 84.6 |
| 512 B | 76.5 | 61.7 | 84.9 | 68.0 | 94.3 | 69.1 | 86.5 |
| 1 KiB | 76.6 | 61.6 | 83.9 | 67.7 | 94.1 | 68.9 | 86.9 |
| 2 KiB | 77.8 | 63.5 | 87.6 | 68.6 | 156.2 | 70.9 | 140.7 |
| 4 KiB | 79.6 | 65.9 | 96.4 | 70.4 | 186.6 | 72.5 | 192.5 |
| 8 KiB | 83.1 | 67.0 | 120.0 | 74.7 | 255.5 | 76.7 | 223.4 |
| 16 KiB | 88.3 | 76.5 | 166.4 | 108.1 | 361.5 | 109.1 | 385.1 |
| 32 KiB | 106.5 | 88.4 | 341.6 | 119.2 | 538.2 | 126.0 | 755.9 |
| 64 KiB | 182.6 | 130.2 | 520.5 | 159.4 | 983.8 | 164.7 | 1,282 |
| 128 KiB | 291.0 | 218.3 | 826.3 | 235.6 | 1,750 | 248.0 | 2,381 |
| 256 KiB | 652.8 | 491.5 | 1,419 | 477.6 | 3,451 | 707.0 | 4,822 |
| 512 KiB | 1,718 | 840.0 | 2,648 | 879.5 | 7,615 | 1,226 | 9,842 |
| 1 MiB | 1,756 | 1,769 | 5,212 | 2,633 | 15,426 | 2,669 | 20,362 |

In this table the runs of a cell differed by 3 % (median over the cells), 56 % at most.

### Latency, reliable: round trip, 99th percentile (µs)

| size | rustorb udp | cyclonedds udp | rustdds udp | rustorb↔cyclonedds udp | rustorb↔rustdds udp | cyclonedds↔rustorb udp | rustdds↔rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 274.3 | 375.7 | 212.5 | 123.7 | 183.7 | 221.4 | 181.1 |
| 64 B | 387.7 | 176.6 | 233.2 | 136.2 | 171.4 | 222.4 | 196.8 |
| 128 B | 220.4 | 182.7 | 221.0 | 124.5 | 177.2 | 174.2 | 199.7 |
| 256 B | 319.6 | 180.2 | 204.6 | 130.0 | 177.0 | 160.5 | 197.0 |
| 512 B | 331.6 | 143.9 | 235.2 | 134.1 | 181.1 | 162.9 | 189.3 |
| 1 KiB | 252.8 | 143.3 | 232.8 | 134.1 | 184.2 | 206.8 | 194.3 |
| 2 KiB | 327.2 | 203.3 | 244.3 | 125.6 | 269.1 | 182.2 | 277.2 |
| 4 KiB | 248.5 | 191.1 | 234.3 | 150.4 | 347.0 | 194.8 | 370.6 |
| 8 KiB | 385.9 | 201.8 | 431.0 | 140.0 | 439.2 | 307.5 | 530.4 |
| 16 KiB | 452.2 | 219.0 | 513.6 | 212.6 | 609.5 | 248.7 | 780.0 |
| 32 KiB | 381.7 | 264.3 | 619.7 | 227.0 | 938.5 | 399.6 | 1,419 |
| 64 KiB | 593.2 | 344.0 | 1,125 | 290.2 | 1,554 | 421.8 | 2,353 |
| 128 KiB | 807.0 | 613.3 | 1,581 | 475.4 | 3,333 | 558.5 | 4,167 |
| 256 KiB | 1,272 | 800.9 | 2,644 | 771.6 | 6,673 | 1,584 | 8,059 |
| 512 KiB | 2,595 | 1,332 | 4,669 | 1,441 | 12,891 | 2,316 | 15,151 |
| 1 MiB | 3,462 | 3,207 | 8,460 | 4,669 | 25,514 | 3,908 | 29,787 |

In this table the runs of a cell differed by 21 % (median over the cells), 138 % at most.

### Throughput, reliable (Mbit/s of payload received)

| size | rustorb udp | cyclonedds udp | rustdds udp | rustorb→cyclonedds udp | rustorb→rustdds udp | cyclonedds→rustorb udp | rustdds→rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 29.1 | 30.9 | 0.1 | 33.5 | 2.4 | 69.3 | 62.7 |
| 256 B | 225.3 | 254.7 | 0.4 | 274.7 | 8.1 | 497.9 | 219.8 |
| 1 KiB | 818.1 | 836.8 | 422.5 | 1,004 | 559.3 | 1,641 | 250.7 |
| 4 KiB | 3,164 | 2,632 | 1,130 | 3,589 | 26.9 | 4,376 | 327.3 |
| 16 KiB | 10,216 | 5,536 | 2,653 | 9,922 | 7,771 | 7,435 | 552.5 |
| 64 KiB | 9,493 | 12,294 | 2,823 | 9,698 | 9,189 | 8,775 | 535.3 |
| 256 KiB | 8,764 | 11,000 | 3,481 | 11,208 | 988.6 | 11,320 | 368.7 |
| 1 MiB | 11,907 | 8,711 | 2,855 | 6,983 | 1.7 | 10,972 | 845.6 |

In this table the runs of a cell differed by 5 % (median over the cells), 141876 % at most.

Samples lost, reliable:

| size | rustorb udp | cyclonedds udp | rustdds udp | rustorb→cyclonedds udp | rustorb→rustdds udp | cyclonedds→rustorb udp | rustdds→rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 0 | 0 | 0 | 0 | 167 | 0 | 0 |
| 256 B | 0 | 0 | 0 | 0 | 0 | 0 | 0 |
| 1 KiB | 0 | 0 | 106 | 0 | 839 | 0 | 0 |
| 4 KiB | 0 | 0 | 16 | 0 | 9 | 0 | 0 |
| 16 KiB | 0 | 0 | 255 | 0 | 541 | 0 | 0 |
| 64 KiB | 0 | 0 | 19 | 0 | 734 | 0 | 0 |
| 256 KiB | 0 | 0 | 9 | 0 | 104 | 0 | 0 |
| 1 MiB | 0 | 0 | 51 | 0 | 0 | 0 | 0 |

### Latency, best-effort: round trip, median (µs)

| size | rustorb udp | cyclonedds udp | rustdds udp | rustorb↔cyclonedds udp | rustorb↔rustdds udp | cyclonedds↔rustorb udp | rustdds↔rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 73.5 | 59.0 | 97.9 | 63.1 | 91.0 | 67.6 | 82.2 |
| 64 B | 70.1 | 59.7 | 97.9 | 63.5 | 89.9 | 66.6 | 86.6 |
| 128 B | 72.0 | 59.7 | 98.0 | 63.6 | 90.3 | 66.2 | 86.8 |
| 256 B | 70.6 | 59.7 | 99.9 | 64.8 | 93.3 | 65.6 | 84.5 |
| 512 B | 71.4 | 60.0 | 99.7 | 65.1 | 94.3 | 65.5 | 84.2 |
| 1 KiB | 70.8 | 60.3 | 101.3 | 64.8 | 90.8 | 67.6 | 85.6 |
| 2 KiB | 71.2 | 61.7 | 110.3 | 66.1 | 137.3 | 68.0 | 109.7 |
| 4 KiB | 74.6 | 62.4 | 125.4 | 67.3 | 178.8 | 69.9 | 154.4 |
| 8 KiB | 78.5 | 65.8 | 143.4 | 72.1 | 232.7 | 72.5 | 224.0 |
| 16 KiB | 86.1 | 74.1 | 171.6 | 93.1 | 340.0 | 95.8 | 369.7 |
| 32 KiB | 101.3 | 86.7 | 325.4 | 105.8 | 531.3 | 107.7 | 664.9 |
| 64 KiB | 171.7 | 127.1 | 486.8 | 142.3 | 825.9 | 143.4 | 1,250 |
| 128 KiB | 275.1 | 209.6 | 790.4 | 213.8 | 1,637 | 214.0 | 2,292 |
| 256 KiB | 819.9 | 455.1 | 1,379 | 461.8 | 3,085 | 620.8 | 4,830 |
| 512 KiB | 1,408 | 780.3 | 2,609 | 825.5 | 5,551 | 827.0 | 11,194 |
| 1 MiB | 1,798 | 2,310 | 4,967 | 2,369 | 12,300 | 1,796 | 25,793 |

In this table the runs of a cell differed by 4 % (median over the cells), 58 % at most.

### Latency, best-effort: round trip, 99th percentile (µs)

| size | rustorb udp | cyclonedds udp | rustdds udp | rustorb↔cyclonedds udp | rustorb↔rustdds udp | cyclonedds↔rustorb udp | rustdds↔rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 364.5 | 114.0 | 206.0 | 127.2 | 194.2 | 187.3 | 170.8 |
| 64 B | 203.4 | 123.0 | 256.1 | 127.1 | 175.6 | 188.2 | 165.8 |
| 128 B | 273.1 | 116.5 | 189.9 | 144.3 | 214.2 | 130.4 | 185.0 |
| 256 B | 171.8 | 120.1 | 262.8 | 147.9 | 226.5 | 137.1 | 188.4 |
| 512 B | 211.4 | 119.4 | 229.9 | 148.5 | 246.0 | 150.0 | 151.4 |
| 1 KiB | 189.5 | 113.9 | 235.2 | 124.0 | 189.8 | 155.8 | 162.7 |
| 2 KiB | 155.2 | 124.6 | 239.3 | 134.3 | 283.6 | 146.1 | 216.0 |
| 4 KiB | 322.2 | 119.0 | 299.7 | 149.3 | 376.4 | 175.3 | 320.2 |
| 8 KiB | 192.3 | 122.7 | 411.1 | 155.1 | 444.4 | 160.3 | 502.3 |
| 16 KiB | 290.4 | 149.4 | 389.9 | 192.9 | 617.6 | 243.9 | 808.3 |
| 32 KiB | 323.9 | 176.2 | 586.3 | 243.2 | 997.2 | 199.6 | 1,357 |
| 64 KiB | 525.0 | 242.2 | 858.5 | 307.5 | 1,540 | 333.9 | 2,262 |
| 128 KiB | 760.3 | 381.9 | 1,307 | 469.0 | 2,937 | 452.5 | 3,689 |
| 256 KiB | 1,313 | 703.2 | 2,185 | 771.8 | 5,135 | 1,043 | 8,734 |
| 512 KiB | 2,122 | 1,151 | 4,234 | 1,249 | 10,427 | 1,409 | 15,881 |
| 1 MiB | 2,603 | 3,710 | 8,104 | 3,217 | 19,430 | 3,008 | 29,423 |

In this table the runs of a cell differed by 20 % (median over the cells), 260 % at most.

### Throughput, best-effort (Mbit/s of payload received)

| size | rustorb udp | cyclonedds udp | rustdds udp | rustorb→cyclonedds udp | rustorb→rustdds udp | cyclonedds→rustorb udp | rustdds→rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 55.4 | 34.3 | 4.7 | 35.9 | 3.3 | 24.4 | 6.4 |
| 256 B | 410.0 | 250.7 | 92.2 | 252.9 | 22.3 | 216.3 | 27.5 |
| 1 KiB | 1,475 | 830.2 | 118.4 | 823.7 | 79.3 | 876.4 | 48.2 |
| 4 KiB | 5,472 | 2,708 | 483.5 | 2,738 | 305.3 | 2,725 | 147.9 |
| 16 KiB | 12,449 | 7,057 | 2,391 | 7,977 | 749.1 | 7,096 | 405.2 |
| 64 KiB | 13,561 | 12,740 | 3,755 | 12,118 | 11,064 | 10,335 | 703.1 |
| 256 KiB | 12,885 | 16,314 | 3,827 | 12,058 | 14,623 | 9,986 | 1,055 |
| 1 MiB | 12,662 | 12,994 | 3,652 | 11,333 | 13,132 | 9,746 | 934.5 |

In this table the runs of a cell differed by 8 % (median over the cells), 60 % at most.

Samples lost, best-effort:

| size | rustorb udp | cyclonedds udp | rustdds udp | rustorb→cyclonedds udp | rustorb→rustdds udp | cyclonedds→rustorb udp | rustdds→rustorb udp |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 32 B | 677 | 63789 | 3355831 | 683587 | 1280353 | 0 | 3054692 |
| 256 B | 0 | 215440 | 2836752 | 785437 | 1272713 | 0 | 3486905 |
| 1 KiB | 215 | 318883 | 3029331 | 710234 | 1164462 | 0 | 3508556 |
| 4 KiB | 0 | 378737 | 3393352 | 693235 | 1059087 | 3020 | 3185131 |
| 16 KiB | 127431 | 41771 | 2560127 | 380569 | 932351 | 8363 | 2827255 |
| 64 KiB | 16674 | 523 | 798479 | 3489 | 82333 | 62671 | 828016 |
| 256 KiB | 9607 | 390 | 222550 | 314 | 9609 | 35454 | 225550 |
| 1 MiB | 16 | 4383 | 30563 | 120 | 904 | 8391 | 31340 |

### Killed for memory

These runs were stopped when a side's resident set grew past the limit; they have no numbers.

- `throughput-rustdds-rustorb-udp-best-effort-1048576`: the measuring side, over 3072 MiB
- `throughput-rustdds-rustorb-udp-best-effort-16384`: the measuring side, over 3072 MiB
- `throughput-rustdds-rustorb-udp-best-effort-262144`: the measuring side, over 3072 MiB
- `throughput-rustdds-rustorb-udp-best-effort-65536`: the measuring side, over 3072 MiB

### CPU time and memory, latency runs

| case | CPU s (measuring side) | CPU s (other side) | max RSS MiB (measuring) | max RSS MiB (other) |
|---|---:|---:|---:|---:|
| cyclonedds udp best-effort | 47.5 | 49.1 | 13.7 | 13.0 |
| cyclonedds udp reliable | 48.1 | 47.9 | 14.8 | 14.0 |
| cyclonedds↔rustorb udp best-effort | 42.3 | 55.9 | 13.6 | 13.0 |
| cyclonedds↔rustorb udp reliable | 42.7 | 61.4 | 13.8 | 13.0 |
| rustdds udp best-effort | 58.5 | 59.1 | 83.9 | 83.3 |
| rustdds udp reliable | 75.6 | 75.7 | 1,177 | 1,168 |
| rustdds↔rustorb udp best-effort | 82.4 | 31.1 | 89.1 | 11.4 |
| rustdds↔rustorb udp reliable | 88.5 | 36.5 | 386.5 | 11.4 |
| rustorb udp best-effort | 47.8 | 47.5 | 11.7 | 11.6 |
| rustorb udp reliable | 48.8 | 52.2 | 12.0 | 11.8 |
| rustorb↔cyclonedds udp best-effort | 54.6 | 44.8 | 12.7 | 12.9 |
| rustorb↔cyclonedds udp reliable | 60.1 | 47.1 | 12.8 | 12.9 |
| rustorb↔rustdds udp best-effort | 54.7 | 74.1 | 11.6 | 84.2 |
| rustorb↔rustdds udp reliable | 57.0 | 79.7 | 11.8 | 501.1 |

### CPU time and memory, throughput runs

| case | CPU s (measuring side) | CPU s (other side) | max RSS MiB (measuring) | max RSS MiB (other) |
|---|---:|---:|---:|---:|
| cyclonedds udp best-effort | 6.7 | 7.1 | 11.8 | 7.9 |
| cyclonedds udp reliable | 5.9 | 2.7 | 9.2 | 8.6 |
| cyclonedds→rustorb udp best-effort | 7.0 | 6.9 | 24.7 | 7.7 |
| cyclonedds→rustorb udp reliable | 8.3 | 5.3 | 11.5 | 8.4 |
| rustdds udp best-effort | 6.2 | 41.2 | 24.5 | 65.4 |
| rustdds udp reliable | 7.1 | 7.4 | 29.9 | 1,084 |
| rustdds→rustorb udp best-effort | 1.8 | 39.5 | 5.9 | 96.9 |
| rustdds→rustorb udp reliable | 4.7 | 6.3 | 5.8 | 351.2 |
| rustorb udp best-effort | 8.3 | 5.9 | 32.4 | 4.9 |
| rustorb udp reliable | 7.6 | 6.6 | 10.8 | 9.9 |
| rustorb→cyclonedds udp best-effort | 6.8 | 5.9 | 12.4 | 4.9 |
| rustorb→cyclonedds udp reliable | 8.0 | 6.4 | 10.0 | 10.8 |
| rustorb→rustdds udp best-effort | 9.8 | 6.0 | 273.6 | 4.9 |
| rustorb→rustdds udp reliable | 8.1 | 6.7 | 65.5 | 10.7 |
