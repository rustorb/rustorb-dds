#!/usr/bin/env python3
"""Sums up `rtps_dump.py --trace --time` output of a ping/pong: per
sender, how many of each submessage it sent, and the median and 90th
percentile of its turnaround (a DATA it sent after a DATA it received);
then the lines around the slowest turnarounds.

    rtps_dump.py capture.pcap DOMAIN --trace --time | trace_summary.py
"""
import re
import statistics
import sys
from collections import Counter, defaultdict

LINE = re.compile(r"\s*(\d+)us domain \d+ (\d+)->(\d+) vendor (\w+) prefix \w+ (\w+)")
lines = [l.rstrip() for l in sys.stdin if "us domain" in l and " SEDP " not in l]
events = []
for l in lines:
    m = LINE.match(l)
    if m:
        events.append((int(m.group(1)), m.group(4), m.group(5), l))
kinds = Counter((v, k) for _, v, k, _ in events)
for (v, k), n in sorted(kinds.items()):
    print(f"vendor {v} sent {n} {k}")
turn = defaultdict(list)
last_data = None
for i, (t, v, k, l) in enumerate(events):
    if k not in ("DATA", "DATA_FRAG") or " writer 000" not in l or "c2 " in l:
        continue
    if last_data and last_data[1] != v:
        turn[v].append((t - last_data[0], i))
    last_data = (t, v)
for v, xs in turn.items():
    d = sorted(x for x, _ in xs)
    print(f"vendor {v} turnaround: n {len(d)} median {statistics.median(d)} us"
          f" p90 {d[int(len(d) * 0.9)]} us max {d[-1]} us")
for v, xs in turn.items():
    for dt, i in sorted(xs)[-3:]:
        print(f"--- vendor {v} turnaround {dt} us")
        for _, _, _, l in events[max(0, i - 12):i + 2]:
            print(l)
