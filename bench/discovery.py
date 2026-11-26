#!/usr/bin/env python3
"""Discovery time and how it grows with the participants (rustorb-dds#106).

Each run starts N processes of the `disc` mode of the perf programs at
once, on one host: each creates a participant with a writer and a reader
of one topic and says, in seconds from the common start, when it created
the participant and when its writer and reader matched those of all N
participants. The run's time is that of the last one to match. With
tcpdump (and the right to capture), the UDP packets and octets of the
run are counted too: with no samples written, they are the discovery
traffic (SPDP, SEDP and their HEARTBEATs and ACKNACKs, liveliness).

    bench/discovery.py                                  # everything, 3 times
    bench/discovery.py --peers rustorb --counts 2,10    # some of it
    bench/discovery.py --mixed rustorb,cyclonedds       # half and half

Every run is one line of JSON in <out>/discovery.jsonl, and a table of
medians is printed (`--report <out>` prints it again).
"""

import argparse
import datetime
import json
import os
import shutil
import signal
import socket
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BENCH = ROOT / "bench"
sys.path.insert(0, str(BENCH))
import peers  # noqa: E402

COUNTS = [1, 2, 5, 10, 20, 50]
PEERS = ["rustorb", "cyclonedds", "fastdds"]
MIXED = ["rustorb,cyclonedds"]


def stay(n):
    """How long every process stays: long enough for the last to match."""
    return 5 if n <= 10 else 10 if n <= 20 else 20


def argv_of(peer, n, domain, start, seconds):
    a = [str(peer.program()), "disc", "--domain", str(domain), "--participants", str(n),
         "--transport", "udp", "--duration", str(seconds), "--start", f"{start:.6f}", "--json"]
    return a


class Capture:
    """tcpdump of the UDP traffic of all interfaces during a run."""

    def __init__(self, path):
        self.path = path
        self.proc = None
        if shutil.which("tcpdump"):
            self.proc = subprocess.Popen(
                ["tcpdump", "-i", "any", "-w", str(path), "-U", "udp"],
                stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
            time.sleep(0.5)
            if self.proc.poll() is not None:
                self.proc = None

    def stop(self):
        """(packets, octets), or None without a capture."""
        if not self.proc:
            return None
        time.sleep(0.2)
        self.proc.send_signal(signal.SIGINT)
        self.proc.wait(10)
        out = subprocess.run(["tcpdump", "-r", str(self.path), "-nn", "-q"],
                             capture_output=True, text=True).stdout
        packets = octets = 0
        for line in out.splitlines():
            packets += 1
            tail = line.rsplit(" ", 1)[-1]
            if tail.isdigit():
                octets += int(tail)
        return packets, octets


def run_once(group, n, domain, tmp):
    """One run of n processes: `group` gives the peer of each, in turn."""
    seconds = stay(n)
    cap = Capture(Path(tmp) / f"d{domain}.pcap")
    start = time.time()
    procs = []
    for i in range(n):
        p = group[i % len(group)]
        out = open(Path(tmp) / f"out{i}", "w+")
        procs.append((p, out, subprocess.Popen(argv_of(p, n, domain, start, seconds),
                                               stdout=out, stderr=subprocess.STDOUT,
                                               env={**os.environ, **p.env("udp")})))
    cpu = 0.0
    peak = {}
    status = {}
    deadline = time.time() + seconds + 60
    # One loop for all of them: the largest resident set (VmHWM) is read
    # while each runs (wait4's ru_maxrss would be Python's, see peers).
    while len(status) < len(procs):
        for i, (p, out, proc) in enumerate(procs):
            if i in status:
                continue
            kib = peers.peak_rss_kib(proc.pid)
            if kib is not None:
                peak[i] = kib
            pid, st, ru = os.wait4(proc.pid, os.WNOHANG)
            if pid:
                status[i] = st
                cpu += ru.ru_utime + ru.ru_stime
                proc.returncode = os.waitstatus_to_exitcode(st)
            elif time.time() > deadline:
                proc.kill()
        time.sleep(0.05)
    results = []
    for p, out, proc in procs:
        out.seek(0)
        got = peers.json_lines(out.read())
        out.close()
        results.append(got[0] if got else {"impl": p.name, "error": proc.returncode})
    rss = [k / 1024 for k in peak.values()]
    traffic = cap.stop()
    matched = [r.get("match_s") for r in results]
    r = {
        "participants": n,
        "peers": [p.name for p in group],
        "all_matched": all(m is not None for m in matched),
        "match_s_max": max((m for m in matched if m is not None), default=None),
        "match_s_median": statistics.median(m for m in matched if m is not None)
        if any(m is not None for m in matched) else None,
        "create_s_max": max((x.get("create_s", 0) for x in results), default=None),
        "cpu_s": round(cpu, 3),
        "max_rss_mib": round(max(rss), 1) if rss else None,
        "seconds": seconds,
        "processes": results,
    }
    if traffic:
        r["packets"], r["octets"] = traffic
    return r


def label(r):
    return "+".join(r["peers"]) if len(r["peers"]) > 1 else r["peers"][0]


def report(rows):
    by = {}
    for r in rows:
        by.setdefault((label(r), r["participants"]), []).append(r)
    names = list(dict.fromkeys(label(r) for r in rows))
    counts = sorted({r["participants"] for r in rows})

    def med(rs, key):
        v = [x[key] for x in rs if x.get(key) is not None]
        return statistics.median(v) if v else None

    def table(title, key, fmt):
        print(f"\n### {title}\n")
        print("| participants | " + " | ".join(names) + " |")
        print("|---:|" + "---:|" * len(names))
        for n in counts:
            cells = []
            for name in names:
                rs = by.get((name, n), [])
                v = med(rs, key)
                miss = sum(not x["all_matched"] for x in rs)
                c = "" if v is None else fmt(v)
                if miss:
                    c += f" ({miss} of {len(rs)} not all matched)"
                cells.append(c)
            print(f"| {n} | " + " | ".join(cells) + " |")

    table("Until all matched, the last participant (ms, median of runs)", "match_s_max",
          lambda v: f"{v * 1000:,.0f}")
    table("Until matched, the median participant (ms)", "match_s_median",
          lambda v: f"{v * 1000:,.0f}")
    table("CPU time of all the processes over the run (s)", "cpu_s", lambda v: f"{v:.2f}")
    table("Largest resident set of one process (MiB)", "max_rss_mib", lambda v: f"{v:.0f}")
    if any("packets" in r for r in rows):
        table("UDP packets of the run (all of its seconds)", "packets", lambda v: f"{v:,.0f}")
        table("UDP octets of the run (KiB)", "octets", lambda v: f"{v / 1024:,.0f}")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--peers", default=",".join(PEERS))
    ap.add_argument("--mixed", default=";".join(MIXED),
                    help="groups whose processes alternate, ';' between groups ('' for none)")
    ap.add_argument("--counts", default=",".join(map(str, COUNTS)))
    ap.add_argument("--repeat", type=int, default=3)
    ap.add_argument("--domain", type=int, default=60)
    ap.add_argument("--out", type=Path)
    ap.add_argument("--report", type=Path, help="print the tables of an earlier run")
    ap.add_argument("--strict", action="store_true",
                    help="exit with 1 if some run did not all match (for CI)")
    args = ap.parse_args()
    if args.report:
        rows = [json.loads(l) for l in (args.report / "discovery.jsonl").read_text().splitlines() if l]
        report(rows)
        return 0
    groups = [[n] for n in args.peers.split(",") if n]
    groups += [g.split(",") for g in args.mixed.split(";") if g]
    made = {}
    for g in groups:
        for n in g:
            if n not in made:
                p = peers.make(n)
                why = p.missing()
                if why:
                    print(f"[disc] {n}: skipped ({why})", file=sys.stderr)
                    made[n] = None
                else:
                    p.build()
                    made[n] = p
    groups = [[made[n] for n in g] for g in groups if all(made[n] for n in g)]
    stamp = datetime.datetime.now().strftime("%Y-%m-%d-%H%M")
    out = args.out or BENCH / "results" / f"{stamp}-{socket.gethostname()}-discovery"
    out.mkdir(parents=True, exist_ok=True)
    rows = []
    counts = [int(c) for c in args.counts.split(",")]
    k = 0
    with tempfile.TemporaryDirectory() as tmp, open(out / "discovery.jsonl", "a") as f:
        for rep in range(1, args.repeat + 1):
            for g in groups:
                for n in counts:
                    if len(g) > 1 and n < 2:
                        continue
                    # A domain for each run: nothing left of the one before.
                    domain = args.domain + k % 40
                    k += 1
                    r = run_once(g, n, domain, tmp)
                    r["repeat"] = rep
                    r["domain"] = domain
                    rows.append(r)
                    f.write(json.dumps(r) + "\n")
                    f.flush()
                    m = r["match_s_max"]
                    print(f"[disc] {label(r)} x{n} #{rep}: "
                          + (f"all matched at {m * 1000:.0f} ms" if r["all_matched"] else "NOT all matched"),
                          file=sys.stderr)
    report(rows)
    print(f"\n[disc] results in {out}", file=sys.stderr)
    return 1 if args.strict and not all(r["all_matched"] for r in rows) else 0


if __name__ == "__main__":
    sys.exit(main())
