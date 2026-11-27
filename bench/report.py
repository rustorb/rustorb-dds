#!/usr/bin/env python3
"""Makes Markdown tables (and SVG charts) of a bench/run.py result.

    bench/report.py bench/results/<run>            # tables on stdout
    bench/report.py bench/results/<run> --svg DIR  # also latency.svg, throughput.svg

Each value is the median over the repeats of a case. Columns are
"<implementation>[→<peer>] <transport>"; latency is the round trip
(ping to pong and back) in µs, throughput what the subscriber received.
"""

import argparse
import json
import math
import statistics
import sys
from collections import defaultdict
from pathlib import Path


def load(run):
    rows = []
    for line in (run / "results.jsonl").read_text().splitlines():
        if line.strip():
            rows.append(json.loads(line))
    machine = {}
    if (run / "machine.json").exists():
        machine = json.loads((run / "machine.json").read_text())
    return rows, machine


def group_name(r):
    """The column: the implementation, or for two of them who sends to
    whom: "ping↔pong" for latency, "pub→sub" for throughput (the
    measuring side is ping or sub, `impl`; the other is `peer`)."""
    if r["impl"] == r["peer"]:
        pair = r["impl"]
    elif r["kind"] == "latency":
        pair = f"{r['impl']}↔{r['peer']}"
    else:
        pair = f"{r['peer']}→{r['impl']}"
    t = r["transport"]
    if r.get("security"):
        # bench/run.py --security: the setting after the transport.
        t += "+" + r["security"]
    return f"{pair} {t}"


def medians(rows, kind, reliability, field):
    """{group: {size: median of field}}"""
    acc = defaultdict(lambda: defaultdict(list))
    for r in rows:
        if r["kind"] != kind or r["reliability"] != reliability:
            continue
        v = r.get(field)
        if v is None:
            continue
        if kind == "latency" and field.startswith("rtt_") and not r.get("count"):
            # Every round trip lost: no time to show.
            v = math.nan
        acc[group_name(r)][r["size"]].append(v)
    def med(v):
        ok = [x for x in v if not math.isnan(x)]
        # More runs with nothing than with something: nothing.
        return statistics.median(ok) if len(ok) * 2 > len(v) else math.nan

    return {g: {s: med(v) for s, v in by.items()} for g, by in acc.items()}


def spread(rows, kind, reliability, field):
    """The largest (max - min) / median over the repeats, in %: how much
    the runs differed."""
    acc = defaultdict(list)
    for r in rows:
        if r["kind"] == kind and r["reliability"] == reliability and r.get(field) is not None:
            acc[(group_name(r), r["size"])].append(r[field])
    spreads = []
    for v in acc.values():
        if len(v) > 1 and statistics.median(v) > 0:
            spreads.append((max(v) - min(v)) / statistics.median(v) * 100)
    if not spreads:
        return "one run of each"
    return (
        f"the runs of a cell differed by {statistics.median(spreads):.0f} % "
        f"(median over the cells), {max(spreads):.0f} % at most"
    )


def order(groups):
    rank = {"rustorb": 0, "rustorb-listener": 1, "cyclonedds": 2, "cyclonedds-ddsperf": 3, "fastdds": 4}
    tr = {"udp": 0, "shm": 1, "loan": 2}

    def key(g):
        pair, t = g.rsplit(" ", 1)
        first = pair.split("→")[0]
        first = first.split("↔")[0]
        mixed = "→" in pair or "↔" in pair
        t, _, sec = t.partition("+")
        security = {"": 0, "none": 0, "auth": 1, "sign": 2, "encrypt": 3}
        return (mixed, rank.get(first, 9), pair, tr.get(t, 9), security.get(sec, 9))

    return sorted(groups, key=key)


def fmt(v, digits=1):
    if v is None:
        return "–"
    if isinstance(v, float) and math.isnan(v):
        return "lost"
    if v >= 1000:
        return f"{v:,.0f}"
    return f"{v:.{digits}f}"


def size_label(s):
    if s >= 1 << 20 and s % (1 << 20) == 0:
        return f"{s >> 20} MiB"
    if s >= 1024 and s % 1024 == 0:
        return f"{s >> 10} KiB"
    return f"{s} B"


def table(data, value_fmt=fmt):
    groups = order(data)
    sizes = sorted({s for g in groups for s in data[g]})
    out = ["| size | " + " | ".join(groups) + " |", "|---:|" + "---:|" * len(groups)]
    for s in sizes:
        out.append(
            f"| {size_label(s)} | "
            + " | ".join(value_fmt(data[g].get(s)) for g in groups)
            + " |"
        )
    return "\n".join(out)


def resources(rows, kind):
    acc = defaultdict(lambda: defaultdict(list))
    seen = set()
    for r in rows:
        if r["kind"] != kind:
            continue
        # One record per process run, not per size.
        k = (r["case"], r.get("repeat"))
        if k in seen:
            continue
        seen.add(k)
        g = f"{group_name(r)} {r['reliability']}"
        for side in ("measure", "other"):
            cpu = (r.get(f"{side}_cpu_user_s") or 0) + (r.get(f"{side}_cpu_sys_s") or 0)
            acc[g][f"{side}_cpu"].append(cpu)
            if r.get(f"{side}_max_rss_kib"):
                acc[g][f"{side}_rss"].append(r[f"{side}_max_rss_kib"] / 1024)
    if not acc:
        return ""
    out = [
        "| case | CPU s (measuring side) | CPU s (other side) | max RSS MiB (measuring) | max RSS MiB (other) |",
        "|---|---:|---:|---:|---:|",
    ]
    for g in sorted(acc):
        a = acc[g]
        m = lambda k: fmt(statistics.median(a[k])) if a.get(k) else "–"  # noqa: E731
        out.append(f"| {g} | {m('measure_cpu')} | {m('other_cpu')} | {m('measure_rss')} | {m('other_rss')} |")
    return "\n".join(out)


PALETTE = [
    "#2a6fdb", "#e0663a", "#2e9e5b", "#9b59b6", "#c99a06",
    "#17a2b8", "#d63384", "#6c757d", "#8c564b", "#111111",
]


def svg_chart(data, title, ylabel, path):
    """A log-log line chart: x the size, y the value, a line a group."""
    # One implementation each: the pairs would crowd it (see the tables).
    data = {g: v for g, v in data.items() if "→" not in g and "↔" not in g}
    groups = order(data)
    pts = [(s, v) for g in groups for s, v in data[g].items() if v and v > 0 and not math.isnan(v)]
    if not pts:
        return
    W, H, L, R, T, B = 760, 420, 70, 220, 40, 50
    xs = [math.log2(s) for s, _ in pts]
    ys = [math.log10(v) for _, v in pts]
    x0, x1 = min(xs), max(xs)
    y0, y1 = math.floor(min(ys)), math.ceil(max(ys))
    if x1 == x0:
        x1 += 1
    if y1 == y0:
        y1 += 1

    def X(s):
        return L + (math.log2(s) - x0) / (x1 - x0) * (W - L - R)

    def Y(v):
        return H - B - (math.log10(v) - y0) / (y1 - y0) * (H - T - B)

    e = [
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" '
        'font-family="sans-serif" font-size="12">',
        f'<rect width="{W}" height="{H}" fill="#fff"/>',
        f'<text x="{L}" y="22" font-size="14" font-weight="bold">{title}</text>',
    ]
    for d in range(y0, y1 + 1):
        y = Y(10**d)
        e.append(f'<line x1="{L}" x2="{W - R}" y1="{y:.1f}" y2="{y:.1f}" stroke="#ddd"/>')
        e.append(f'<text x="{L - 6}" y="{y + 4:.1f}" text-anchor="end">{10**d:g}</text>')
    for p in range(int(x0), int(x1) + 1, 2):
        x = X(2**p)
        e.append(f'<line x1="{x:.1f}" x2="{x:.1f}" y1="{T}" y2="{H - B}" stroke="#eee"/>')
        e.append(f'<text x="{x:.1f}" y="{H - B + 16}" text-anchor="middle">{size_label(2**p)}</text>')
    e.append(f'<text x="{(L + W - R) / 2}" y="{H - 10}" text-anchor="middle">sample size</text>')
    e.append(
        f'<text x="16" y="{(T + H - B) / 2}" text-anchor="middle" '
        f'transform="rotate(-90 16 {(T + H - B) / 2})">{ylabel}</text>'
    )
    for i, g in enumerate(groups):
        c = PALETTE[i % len(PALETTE)]
        dash = ' stroke-dasharray="5 3"' if ("→" in g or "↔" in g) else ""
        line = [(X(s), Y(v)) for s, v in sorted(data[g].items()) if v and v > 0 and not math.isnan(v)]
        if not line:
            continue
        d = " ".join(f"{x:.1f},{y:.1f}" for x, y in line)
        e.append(f'<polyline points="{d}" fill="none" stroke="{c}" stroke-width="2"{dash}/>')
        for x, y in line:
            e.append(f'<circle cx="{x:.1f}" cy="{y:.1f}" r="2.5" fill="{c}"/>')
        ly = T + 10 + i * 18
        e.append(f'<line x1="{W - R + 12}" x2="{W - R + 36}" y1="{ly}" y2="{ly}" stroke="{c}" stroke-width="2"{dash}/>')
        e.append(f'<text x="{W - R + 42}" y="{ly + 4}">{g}</text>')
    e.append("</svg>")
    path.write_text("\n".join(e) + "\n")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("run", type=Path)
    ap.add_argument("--svg", type=Path, help="directory for the charts")
    args = ap.parse_args()
    rows, machine = load(args.run)
    if not rows:
        print("no results", file=sys.stderr)
        return 1
    print(f"Run: `{args.run.name}`")
    if machine:
        peers = ", ".join(f"{k} {v}" for k, v in machine.get("peers", {}).items() if v)
        print(
            f"on {machine.get('cpu', '?')} ({machine.get('cpus', '?')} CPUs), "
            f"{machine.get('platform', '?')}"
            + (", in a container" if machine.get("container") else "")
            + f"; rustorb-dds {machine.get('rustorb_dds_commit', '?')}; {peers}."
        )
        a = machine.get("args", {})
        print(
            f"Warm-up {a.get('warmup')} s, {a.get('duration')} s measured, "
            f"median of {a.get('repeat')} runs."
        )
    for rel in ("reliable", "best-effort"):
        for field, what in (("rtt_p50_us", "median"), ("rtt_p99_us", "99th percentile")):
            d = medians(rows, "latency", rel, field)
            if d:
                print(f"\n### Latency, {rel}: round trip, {what} (µs)\n")
                print(table(d))
                print(f"\nIn this table {spread(rows, 'latency', rel, field)}.")
        d = medians(rows, "throughput", rel, "mbit_per_s")
        if d:
            print(f"\n### Throughput, {rel} (Mbit/s of payload received)\n")
            print(table(d))
            print(f"\nIn this table {spread(rows, 'throughput', rel, 'mbit_per_s')}.")
            lost = medians(rows, "throughput", rel, "lost")
            if any(v for g in lost.values() for v in g.values()):
                print(f"\nSamples lost, {rel}:\n")
                print(table(lost, lambda v: "–" if v is None else f"{v:.0f}"))
    over = sorted({(r["case"], r["over_rss_side"], r["over_rss_mib"]) for r in rows if r.get("over_rss_mib")})
    if over:
        print("\n### Killed for memory\n")
        print("These runs were stopped when a side's resident set grew past the limit; they have no numbers.\n")
        for case, side, mib in over:
            who = "the measuring side" if side == "measure" else "the other side"
            print(f"- `{case}`: {who}, over {mib} MiB")
    for kind in ("latency", "throughput"):
        t = resources(rows, kind)
        if t:
            print(f"\n### CPU time and memory, {kind} runs\n")
            print(t)
    if args.svg:
        args.svg.mkdir(parents=True, exist_ok=True)
        svg_chart(
            medians(rows, "latency", "reliable", "rtt_p50_us"),
            "Round trip, reliable, median", "µs", args.svg / "latency.svg",
        )
        svg_chart(
            medians(rows, "throughput", "reliable", "mbit_per_s"),
            "Throughput, reliable", "Mbit/s", args.svg / "throughput.svg",
        )
    return 0


if __name__ == "__main__":
    sys.exit(main())
