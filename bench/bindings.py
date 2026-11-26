#!/usr/bin/env python3
"""The cost of the language bindings (rustorb-dds#107).

Each binding's ping (bench/bindings/: C++, Java, Python, .NET, Ruby)
measures the round trip through the Rust `rustorb-dds-perf pong`, as
`rustorb-dds-perf ping` does; what a binding adds is its round trip
less the Rust one. The bindings have no switch for the shared memory, so
every run uses it (`--transport shm` on the Rust side).

    bench/bindings.py                         # every binding found, 3 times
    bench/bindings.py --bindings rust,cpp,python --sizes 32,4096
    bench/bindings.py --report <out>          # the table again

Needs: cargo; cmake and a C++ compiler (cpp); a JDK 22 or newer and
Maven (java); the .NET 8 SDK (dotnet); Ruby 3 with its headers and
rake (ruby); Python with `pip install ./python` (python, or --python).
The types are what rustorb-idl wrote for bench/idl/Perf.idl
(bench/bindings/generated, checked in). Every run is one line of JSON in
<out>/bindings.jsonl.
"""

import argparse
import datetime
import json
import os
import shutil
import socket
import statistics
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BENCH = ROOT / "bench"
sys.path.insert(0, str(BENCH))
import peers  # noqa: E402

B = BENCH / "bindings"
BUILD = peers.BUILD / "bindings"
FFI = ROOT / "target" / "release" / "librustorb_dds_ffi.so"
NAMES = ["rust", "cpp", "java", "dotnet", "python", "ruby"]


def cargo(*args):
    subprocess.run(["cargo", "build", "--release", "--locked", *args], cwd=ROOT, check=True)


def which(tool):
    return shutil.which(tool) is not None


class Binding:
    def __init__(self, name, args):
        self.name = name
        self.args = args

    def missing(self):
        n, a = self.name, self.args
        need = {
            "rust": ["cargo"],
            "cpp": ["cargo", "cmake"],
            "java": ["cargo", a.mvn, "java", "javac"],
            "dotnet": ["cargo", a.dotnet],
            "python": [a.python],
            "ruby": ["cargo", a.ruby],
        }[n]
        for t in need:
            if not which(t):
                return f"no {t}"
        if n == "python":
            ok = subprocess.run([a.python, "-c", "import rustorb_dds"], capture_output=True).returncode == 0
            if not ok:
                return f"no rustorb_dds in {a.python} (pip install ./python)"
        return None

    def build(self):
        n, a = self.name, self.args
        if n == "cpp":
            subprocess.run(["cmake", "-S", str(B / "cpp"), "-B", str(BUILD / "cpp"),
                            "-DCMAKE_BUILD_TYPE=Release"], check=True, stdout=subprocess.DEVNULL)
            subprocess.run(["cmake", "--build", str(BUILD / "cpp"), "-j"], check=True, stdout=subprocess.DEVNULL)
        elif n in ("java", "dotnet", "ruby"):
            cargo("-p", "rustorb-dds-ffi")
            if n == "java":
                subprocess.run([a.mvn, "-B", "-q", "-DskipTests", "compile"], cwd=ROOT / "java", check=True)
                out = BUILD / "java"
                out.mkdir(parents=True, exist_ok=True)
                sources = [str(B / "java" / "Ping.java")] + [str(p) for p in (B / "generated" / "java").rglob("*.java")]
                subprocess.run(["javac", "-d", str(out), "-cp", str(ROOT / "java" / "target" / "classes"), *sources],
                               check=True)
            elif n == "dotnet":
                subprocess.run([a.dotnet, "build", str(B / "dotnet"), "-c", "Release", "-o", str(BUILD / "dotnet")],
                               check=True, stdout=subprocess.DEVNULL)
            else:
                subprocess.run([a.ruby, "-S", "rake", "compile"], cwd=ROOT / "ruby", check=True,
                               env={**os.environ, "CARGO_SKIP": "1"}, stdout=subprocess.DEVNULL)

    def argv(self, domain, sizes, warmup, duration):
        n, a = self.name, self.args
        tail = [str(domain), ",".join(map(str, sizes)), str(warmup), str(duration)]
        if n == "rust":
            return [str(peers.Rustorb().program()), "ping", "--domain", str(domain), "--transport", "shm",
                    "--sizes", tail[1], "--warmup", tail[2], "--duration", tail[3], "--json"]
        if n == "cpp":
            return [str(BUILD / "cpp" / "ping"), *tail]
        if n == "java":
            return ["java", "--enable-native-access=ALL-UNNAMED", f"-Drustorb.dds.ffi.library={FFI}",
                    "-cp", f"{ROOT / 'java' / 'target' / 'classes'}:{BUILD / 'java'}", "Ping", *tail]
        if n == "dotnet":
            return [a.dotnet, str(BUILD / "dotnet" / "Ping.dll"), *tail]
        if n == "python":
            return [a.python, str(B / "ping.py"), *tail]
        return [a.ruby, str(B / "ruby" / "ping.rb"), *tail]


def run_once(b, args, domain, log):
    pong = subprocess.Popen([str(peers.Rustorb().program()), "pong", "--domain", str(domain),
                             "--transport", "shm"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(0.3)
    limit = (args.warmup + args.duration + 2) * len(args.sizes) + 60
    env = {**os.environ, "RUSTORB_DDS_FFI_LIBRARY": str(FFI)}
    try:
        p = subprocess.run(b.argv(domain, args.sizes, args.warmup, args.duration), capture_output=True,
                           text=True, timeout=limit, env=env)
        out, status = p.stdout, p.returncode
    except subprocess.TimeoutExpired as e:
        out, status = e.stdout or "", "timeout"
        if isinstance(out, bytes):
            out = out.decode()
    finally:
        try:
            pong.wait(30)
        except subprocess.TimeoutExpired:
            pong.kill()
            pong.wait()
    log.write(f"$ {b.name} domain {domain}\n{out}\n[exit {status}]\n")
    rows = peers.json_lines(out)
    for r in rows:
        r["binding"] = b.name
    return rows, status


def report(rows):
    by = {}
    for r in rows:
        by.setdefault((r["binding"], r["size"]), []).append(r)
    names = [n for n in NAMES if any(r["binding"] == n for r in rows)]
    sizes = sorted({r["size"] for r in rows})

    def med(rs, key):
        v = [x[key] for x in rs if x.get(key) is not None and x.get("count")]
        return statistics.median(v) if v else None

    for title, key in (("round trip, median", "rtt_p50_us"), ("round trip, 99th percentile", "rtt_p99_us")):
        print(f"\n### Through the Rust pong, {title} (µs; in brackets, more than Rust)\n")
        print("| size | " + " | ".join(names) + " |")
        print("|---:|" + "---:|" * len(names))
        for s in sizes:
            base = med(by.get(("rust", s), []), key)
            cells = []
            for n in names:
                v = med(by.get((n, s), []), key)
                if v is None:
                    cells.append("–")
                elif n == "rust" or base is None:
                    cells.append(f"{v:,.0f}")
                else:
                    cells.append(f"{v:,.0f} ({v - base:+,.0f})")
            label = f"{s >> 10} KiB" if s >= 1024 else f"{s} B"
            print(f"| {label} | " + " | ".join(cells) + " |")


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--bindings", default=",".join(NAMES))
    ap.add_argument("--sizes", default="32,4096,65536", type=lambda s: [int(x) for x in s.split(",")])
    ap.add_argument("--warmup", type=float, default=1.0)
    ap.add_argument("--duration", type=float, default=5.0)
    ap.add_argument("--repeat", type=int, default=3)
    ap.add_argument("--domain", type=int, default=30)
    ap.add_argument("--python", default=sys.executable)
    ap.add_argument("--mvn", default="mvn")
    ap.add_argument("--dotnet", default="dotnet")
    ap.add_argument("--ruby", default="ruby")
    ap.add_argument("--out", type=Path)
    ap.add_argument("--report", type=Path, help="print the table of an earlier run")
    ap.add_argument("--strict", action="store_true", help="exit with 1 if a run failed (for CI)")
    args = ap.parse_args()
    if args.report:
        rows = [json.loads(l) for l in (args.report / "bindings.jsonl").read_text().splitlines() if l]
        report(rows)
        return 0
    cargo("-p", "rustorb-dds-perf")
    todo = []
    for n in args.bindings.split(","):
        b = Binding(n, args)
        why = b.missing()
        if why:
            print(f"[bindings] {n}: skipped ({why})", file=sys.stderr)
            continue
        b.build()
        todo.append(b)
    stamp = datetime.datetime.now().strftime("%Y-%m-%d-%H%M")
    out = args.out or BENCH / "results" / f"{stamp}-{socket.gethostname()}-bindings"
    out.mkdir(parents=True, exist_ok=True)
    rows, failed = [], []
    k = 0
    with open(out / "bindings.jsonl", "a") as f, open(out / "log.txt", "a") as log:
        for rep in range(1, args.repeat + 1):
            for b in todo:
                domain = args.domain + k % 20
                k += 1
                got, status = run_once(b, args, domain, log)
                for r in got:
                    r["repeat"] = rep
                    f.write(json.dumps(r) + "\n")
                f.flush()
                rows += got
                ok = status == 0 and len(got) == len(args.sizes)
                if not ok:
                    failed.append(f"{b.name} #{rep}")
                print(f"[bindings] {b.name} #{rep}: " + ("ok" if ok else f"FAILED ({status})"), file=sys.stderr)
    report(rows)
    print(f"\n[bindings] results in {out}", file=sys.stderr)
    return 1 if args.strict and failed else 0


if __name__ == "__main__":
    sys.exit(main())
