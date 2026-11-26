#!/usr/bin/env python3
"""Runs the performance measurements of rustorb-dds#100 on one host.

Each case runs two processes, a measuring one (ping, sub) and its other
side (pong, pub), of one DDS implementation or of two (interop), with
the same sizes, QoS, warm-up and duration. Every result is one line of
JSON in <out>/results.jsonl, with the CPU time and the largest resident
set of both processes (CPU from wait4, memory from /proc VmHWM), and <out>/machine.json says where
and with what it ran. bench/report.py makes tables of them.

    bench/run.py                          # every peer found, every case
    bench/run.py --peers rustorb          # one implementation
    bench/run.py --only 'latency.*shm'    # the cases whose name matches
    bench/run.py --list                   # the case names, without running

The numbers of a shared machine (a container, a CI runner) vary from run
to run: compare cases of one run with each other, not with another run.
"""

import argparse
import datetime
import json
import os
import platform
import re
import shutil
import signal
import socket
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BENCH = ROOT / "bench"

sys.path.insert(0, str(BENCH))
import peers  # noqa: E402

LATENCY_SIZES = [1 << e for e in range(5, 21)]  # 32 B to 1 MiB
THROUGHPUT_SIZES = [32, 256, 1024, 4096, 16384, 65536, 262144, 1048576]


def cpu_sets(pin):
    """The CPUs of the two sides: halves of those this process may use."""
    if not pin or not hasattr(os, "sched_getaffinity"):
        return None, None
    cpus = sorted(os.sched_getaffinity(0))
    if len(cpus) < 2:
        return None, None
    half = len(cpus) // 2
    return set(cpus[:half]), set(cpus[half:])


class Proc:
    """A process of a case, with its rusage once it ends."""

    def __init__(self, argv, env, cpus, log):
        self.argv = argv
        full_env = dict(os.environ)
        full_env.update(env or {})

        def pre():
            os.setsid()
            if cpus:
                os.sched_setaffinity(0, cpus)

        self.log = log
        self.p = subprocess.Popen(
            argv,
            env=full_env,
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
            text=True,
            preexec_fn=pre,
        )
        self.usage = None
        self.status = None
        self.out = ""
        self.peak_kib = None

    def wait(self, timeout):
        deadline = time.monotonic() + timeout
        chunks = []
        os.set_blocking(self.p.stdout.fileno(), False)
        while True:
            try:
                data = self.p.stdout.read()
                if data:
                    chunks.append(data)
            except (BlockingIOError, TypeError):
                pass
            peak = peers.peak_rss_kib(self.p.pid)
            if peak is not None:
                self.peak_kib = peak
            pid, status, usage = os.wait4(self.p.pid, os.WNOHANG)
            if pid:
                break
            if time.monotonic() > deadline:
                self.kill()
                pid, status, usage = os.wait4(self.p.pid, 0)
                chunks.append(f"\n[bench] killed after {timeout:.0f} s\n")
                break
            time.sleep(0.05)
        rest = self.p.stdout.read()
        if rest:
            chunks.append(rest)
        self.p.returncode = os.waitstatus_to_exitcode(status)
        self.status = self.p.returncode
        self.usage = usage
        self.out = "".join(chunks)
        self.log.write(f"$ {' '.join(self.argv)}\n{self.out}\n[exit {self.status}]\n")
        return self.status

    def kill(self):
        try:
            os.killpg(self.p.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass

    def resources(self, prefix):
        if self.usage is None:
            return {}
        # VmHWM read while it ran (see peers.peak_rss_kib), not ru_maxrss.
        return {
            f"{prefix}_cpu_user_s": round(self.usage.ru_utime, 3),
            f"{prefix}_cpu_sys_s": round(self.usage.ru_stime, 3),
            f"{prefix}_max_rss_kib": self.peak_kib,
        }


def cases(args, available):
    """Every case: (name, kind, measuring peer, other peer, transport,
    reliability, sizes)."""
    out = []
    reliabilities = args.reliability.split(",")
    for kind in args.kinds.split(","):
        sizes = args.sizes or (LATENCY_SIZES if kind == "latency" else THROUGHPUT_SIZES)
        for a, b in pairs(args, available):
            if kind != "latency" and (
                getattr(a, "latency_only", False) or getattr(b, "latency_only", False)
            ):
                continue
            transports = [t for t in a.transports if t in b.transports]
            if a is not b:
                # Between implementations only UDP is common.
                transports = [t for t in transports if t == "udp"]
            for t in transports:
                if args.transports and t not in args.transports.split(","):
                    continue
                for r in reliabilities:
                    pair = a.name if a is b else f"{a.name}-{b.name}"
                    one = getattr(a, "one_size_latency", False) or getattr(b, "one_size_latency", False)
                    if kind == "latency" and one:
                        # A process for each size: a few of them.
                        few = args.sizes or [32, 1024, 16384, 65536, 1048576]
                        for s in few:
                            out.append((f"latency-{pair}-{t}-{r}-{s}", kind, a, b, t, r, [s]))
                    elif kind == "latency":
                        out.append((f"latency-{pair}-{t}-{r}", kind, a, b, t, r, sizes))
                    else:
                        for s in sizes:
                            out.append(
                                (f"throughput-{pair}-{t}-{r}-{s}", kind, a, b, t, r, [s])
                            )
    if args.only:
        rx = re.compile(args.only)
        out = [c for c in out if rx.search(c[0])]
    return out


def pairs(args, available):
    """(measuring, other) peers: each with itself, and with --interop the
    rustorb-dds tool with each other one, both ways."""
    out = [(p, p) for p in available]
    if args.interop:
        rustorb = next((p for p in available if p.name == "rustorb"), None)
        if rustorb:
            for p in available:
                if p is not rustorb and getattr(p, "interop", True):
                    out += [(rustorb, p), (p, rustorb)]
    return out


def run_case(case, args, log, cpus):
    name, kind, a, b, transport, reliability, sizes = case
    common = dict(
        domain=args.domain,
        transport=transport,
        reliability=reliability,
        warmup=args.warmup,
        duration=args.duration,
        sizes=sizes,
    )
    if kind == "latency":
        measure = a.ping(**common)
        other = b.pong(**common)
        limit = (args.warmup + args.duration + 2) * len(sizes) + 60
    else:
        measure = a.sub(**common)
        other = b.pub(**common)
        limit = args.warmup + args.duration + 90
    log.write(f"\n=== {name}\n")
    # The other side first: a pong (or pub) waits for its ping (or sub).
    o = Proc(other.argv, other.env, cpus[1], log)
    time.sleep(0.2)
    m = Proc(measure.argv, measure.env, cpus[0], log)
    m_status = m.wait(limit)
    o_status = o.wait(30)
    results = []
    if m_status in a.ok_status:
        for r in a.parse(kind, m.out, sizes):
            r.update(
                case=name,
                kind=kind,
                impl=a.name,
                peer=b.name,
                transport=transport,
                reliability=reliability,
            )
            r.update(m.resources("measure"))
            r.update(o.resources("other"))
            results.append(r)
    ok = m_status in a.ok_status and bool(results)
    return ok, results, (m_status, o_status)


def machine(available):
    info = {
        "date": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "host": socket.gethostname(),
        "platform": platform.platform(),
        "python": platform.python_version(),
        "cpus": os.cpu_count(),
    }
    try:
        for line in Path("/proc/cpuinfo").read_text().splitlines():
            if line.startswith("model name"):
                info["cpu"] = line.split(":", 1)[1].strip()
                break
    except OSError:
        pass
    info["container"] = Path("/.dockerenv").exists() or bool(os.environ.get("CLAUDE_CODE_REMOTE"))
    try:
        info["rustorb_dds_commit"] = subprocess.run(
            ["git", "-C", str(ROOT), "rev-parse", "--short", "HEAD"],
            capture_output=True, text=True, check=True,
        ).stdout.strip()
    except (OSError, subprocess.CalledProcessError):
        pass
    info["peers"] = {p.name: p.version() for p in available}
    return info


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--peers", default="all", help="comma-separated: " + ",".join(peers.NAMES))
    ap.add_argument("--kinds", default="latency,throughput")
    ap.add_argument("--transports", default="", help="udp,shm,loan (all of a peer's)")
    ap.add_argument("--reliability", default="reliable,best-effort")
    ap.add_argument("--sizes", type=lambda s: [int(x) for x in s.split(",")])
    ap.add_argument("--warmup", type=float, default=1.0)
    ap.add_argument("--duration", type=float, default=5.0)
    ap.add_argument("--repeat", type=int, default=3, help="runs of each case (report.py takes the median)")
    ap.add_argument("--domain", type=int, default=42)
    ap.add_argument("--interop", action="store_true", help="also rustorb with each other peer")
    ap.add_argument("--only", help="regex of the case names to run")
    ap.add_argument("--no-pin", action="store_true", help="do not pin the sides to CPUs")
    ap.add_argument("--out", type=Path)
    ap.add_argument("--list", action="store_true")
    args = ap.parse_args()

    wanted = peers.NAMES if args.peers == "all" else args.peers.split(",")
    available = []
    for n in wanted:
        p = peers.make(n)
        why = p.missing()
        if why:
            print(f"[bench] {n}: skipped ({why})", file=sys.stderr)
        else:
            available.append(p)
    todo = cases(args, available)
    if args.list:
        for c in todo:
            print(c[0])
        return 0
    if not todo:
        print("[bench] nothing to run", file=sys.stderr)
        return 1
    for p in available:
        p.build()
    stamp = datetime.datetime.now().strftime("%Y-%m-%d-%H%M")
    out = args.out or BENCH / "results" / f"{stamp}-{socket.gethostname()}"
    out.mkdir(parents=True, exist_ok=True)
    info = machine(available)
    info["args"] = {k: v for k, v in vars(args).items() if k not in ("out", "list")}
    old = out / "machine.json"
    if old.exists():
        # Another run into the same directory: keep what the first did.
        before = json.loads(old.read_text())
        info["earlier_runs"] = before.pop("earlier_runs", []) + [before]
    old.write_text(json.dumps(info, indent=2) + "\n")
    cpus = cpu_sets(not args.no_pin)
    failed = []
    roudi = None
    if any(c[4] == "shm" and (c[2].name.startswith("cyclonedds") or c[3].name.startswith("cyclonedds")) for c in todo):
        # Cyclone DDS's shared memory goes through iceoryx's daemon.
        if shutil.which("iox-roudi"):
            roudi = subprocess.Popen(["iox-roudi"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            time.sleep(1)
        else:
            print("[bench] no iox-roudi: Cyclone DDS's shm cases will fail", file=sys.stderr)
    with open(out / "results.jsonl", "a") as res, open(out / "log.txt", "a") as log:
        for i, case in enumerate(todo, 1):
            for rep in range(args.repeat):
                t0 = time.monotonic()
                ok, results, statuses = run_case(case, args, log, cpus)
                for r in results:
                    r["repeat"] = rep
                    res.write(json.dumps(r) + "\n")
                res.flush()
                log.flush()
                state = "ok" if ok else f"FAILED {statuses}"
                print(f"[{i}/{len(todo)}] {case[0]} #{rep + 1}: {state} ({time.monotonic() - t0:.0f} s)")
                if not ok:
                    failed.append(f"{case[0]} #{rep + 1}")
    if roudi:
        roudi.terminate()
        roudi.wait()
    print(f"[bench] results in {out}")
    if failed:
        print("[bench] failed: " + ", ".join(failed), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
