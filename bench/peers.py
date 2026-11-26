"""The DDS implementations bench/run.py measures.

Every peer but `cyclonedds-ddsperf` is a program with the command line
and the JSON output of rustorb-dds-perf (crates/rustorb-dds-perf), for
the sample type of bench/idl/Perf.idl, so that all are measured the
same way and can talk to each other:

    <program> ping|pong|pub|sub --domain N --sizes A,B,.. | --size N
              --reliability reliable|best-effort --transport udp|shm|...
              --warmup S --duration S --json

`cyclonedds-ddsperf` is Cyclone DDS's own tool, ddsperf, with its own
type and output, for a second view of Cyclone DDS.
"""

import json
import math
import os
import re
import shutil
import subprocess
from dataclasses import dataclass, field
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BENCH = ROOT / "bench"
BUILD = Path(os.environ.get("BENCH_BUILD", ROOT / "target" / "bench"))

NAMES = ["rustorb", "rustorb-listener", "cyclonedds", "fastdds", "cyclonedds-ddsperf"]


@dataclass
class Cmd:
    argv: list
    env: dict = field(default_factory=dict)


def tool_args(mode, domain, transport, reliability, warmup, duration, sizes):
    a = [
        mode,
        "--domain", str(domain),
        "--reliability", reliability,
        "--transport", transport,
        "--warmup", str(warmup),
        "--duration", str(duration),
        "--json",
    ]
    if mode in ("ping", "pong"):
        a += ["--sizes", ",".join(map(str, sizes))]
    else:
        a += ["--size", str(sizes[0])]
    return a


def peak_rss_kib(pid):
    """The largest resident set of a running process so far (VmHWM), in
    KiB, or None once it is gone.

    `wait4`'s ru_maxrss cannot be used: a child forked from Python starts
    with Python's resident set as its high-water mark, and keeps it across
    exec, so a small program shows the size of the runner instead of its
    own. The runners read this while the program runs, every 50 ms.
    """
    try:
        with open(f"/proc/{pid}/status") as f:
            for line in f:
                if line.startswith("VmHWM:"):
                    return int(line.split()[1])
    except OSError:
        pass
    return None


def json_lines(out):
    rs = []
    for line in out.splitlines():
        line = line.strip()
        if line.startswith("{"):
            try:
                rs.append(json.loads(line))
            except json.JSONDecodeError:
                pass
    return rs


class Peer:
    name = "?"
    transports = ("udp",)
    # The exit statuses of a run that went well.
    ok_status = (0,)

    def missing(self):
        """Why it cannot run here, or None."""
        return None

    def build(self):
        pass

    def version(self):
        return None

    def program(self):
        raise NotImplementedError

    def env(self, transport):
        return {}

    def cmd(self, mode, **c):
        return Cmd([str(self.program())] + tool_args(mode, **c), self.env(c["transport"]))

    def ping(self, **c):
        return self.cmd("ping", **c)

    def pong(self, **c):
        return self.cmd("pong", **c)

    def pub(self, **c):
        return self.cmd("pub", **c)

    def sub(self, **c):
        return self.cmd("sub", **c)

    def parse(self, kind, out, sizes):
        return json_lines(out)


class Rustorb(Peer):
    name = "rustorb"
    transports = ("udp", "shm", "loan")

    def missing(self):
        return None if shutil.which("cargo") else "no cargo"

    def build(self):
        subprocess.run(
            ["cargo", "build", "--release", "--locked", "-p", "rustorb-dds-perf"],
            cwd=ROOT, check=True,
        )

    def program(self):
        return ROOT / "target" / "release" / "rustorb-dds-perf"

    def version(self):
        m = re.search(r'^version = "([^"]+)"', (ROOT / "Cargo.toml").read_text(), re.M)
        return m and m.group(1)


class RustorbListener(Rustorb):
    """rustorb-dds-perf with --listener: ping and pong receive in the
    reader's listener and answer from it, as ddsperf does. Latency only."""

    name = "rustorb-listener"
    latency_only = True
    # rustorb-dds-perf's interop cases are those of `rustorb`.
    interop = False

    def ping(self, **c):
        cmd = super().ping(**c)
        cmd.argv.append("--listener")
        return cmd

    def pong(self, **c):
        cmd = super().pong(**c)
        cmd.argv.append("--listener")
        return cmd


def dpkg_version(package):
    try:
        r = subprocess.run(
            ["dpkg-query", "-W", "-f=${Version}", package],
            capture_output=True, text=True,
        )
        return r.stdout.strip() or None
    except OSError:
        return None


def cmake_build(source, build):
    build.mkdir(parents=True, exist_ok=True)
    subprocess.run(
        ["cmake", "-S", str(source), "-B", str(build), "-DCMAKE_BUILD_TYPE=Release"],
        check=True, stdout=subprocess.DEVNULL,
    )
    subprocess.run(["cmake", "--build", str(build), "-j"], check=True, stdout=subprocess.DEVNULL)


class CycloneDds(Peer):
    """bench/cyclonedds/perf.c on Cyclone DDS's C API."""

    name = "cyclonedds"
    transports = ("udp", "shm")

    def missing(self):
        if not shutil.which("idlc"):
            return "no idlc (apt-get install cyclonedds-dev cyclonedds-tools)"
        return None

    def build(self):
        cmake_build(BENCH / "cyclonedds", BUILD / "cyclonedds")

    def program(self):
        return BUILD / "cyclonedds" / "perf"

    def version(self):
        return dpkg_version("cyclonedds-dev")

    def env(self, transport):
        return cyclone_env(transport)


def cyclone_env(transport):
    """The configuration: shared memory through iceoryx (iox-roudi must
    run; bench/run.py starts it) or not."""
    shm = "true" if transport == "shm" else "false"
    return {
        "CYCLONEDDS_URI": (
            "<CycloneDDS><Domain><SharedMemory>"
            f"<Enable>{shm}</Enable><LogLevel>error</LogLevel>"
            "</SharedMemory></Domain></CycloneDDS>"
        )
    }


class FastDds(Peer):
    """bench/fastdds/perf.cpp on Fast DDS's C++ API."""

    name = "fastdds"
    transports = ("udp", "shm")

    def missing(self):
        if not shutil.which("fastddsgen"):
            return "no fastddsgen (apt-get install libfastrtps-dev fastddsgen)"
        return None

    def build(self):
        gen = BUILD / "fastdds" / "gen"
        gen.mkdir(parents=True, exist_ok=True)
        subprocess.run(
            ["fastddsgen", "-replace", "-d", str(gen), str(BENCH / "idl" / "Perf.idl")],
            check=True, stdout=subprocess.DEVNULL,
        )
        cmake_build(BENCH / "fastdds", BUILD / "fastdds")

    def program(self):
        return BUILD / "fastdds" / "perf"

    def version(self):
        return dpkg_version("libfastrtps-dev")


class CycloneDdsperf(Peer):
    """Cyclone DDS's ddsperf, with its own topic KS (keyed, a sequence
    of octets) and output."""

    name = "cyclonedds-ddsperf"
    transports = ("udp", "shm")

    def missing(self):
        return None if shutil.which("ddsperf") else "no ddsperf (apt-get install cyclonedds-tools)"

    def version(self):
        return dpkg_version("cyclonedds-tools")

    def base(self, c):
        a = ["ddsperf", "-i", str(c["domain"]), "-TKS"]
        if c["reliability"] == "best-effort":
            a.append("-u")
        return a

    # ddsperf runs one size per process: bench/run.py gives ping one size
    # when the peer says so.
    one_size_latency = True
    # Its type is not RustorbPerf::Sample.
    interop = False

    # With shared memory (iceoryx) ddsperf ends by SIGTERM after -D.
    ok_status = (0, -15)

    @staticmethod
    def seconds(c):
        """-D: ddsperf prints once a second from its second second, and
        the first of those lines is left out as warm-up."""
        return max(3, math.ceil(c["warmup"]) + math.ceil(c["duration"]) + 1)

    def ping(self, **c):
        d = self.seconds(c)
        return Cmd(self.base(c) + ["-D", str(d), "ping", "size", str(c["sizes"][0])],
                   cyclone_env(c["transport"]))

    def pong(self, **c):
        d = self.seconds(c) + 2
        return Cmd(self.base(c) + ["-D", str(d), "pong"], cyclone_env(c["transport"]))

    def pub(self, **c):
        d = self.seconds(c) + 2
        return Cmd(self.base(c) + ["-D", str(d), "pub", "size", str(c["sizes"][0])],
                   cyclone_env(c["transport"]))

    def sub(self, **c):
        d = self.seconds(c)
        return Cmd(self.base(c) + ["-D", str(d), "sub"], cyclone_env(c["transport"]))

    def parse(self, kind, out, sizes):
        return parse_ddsperf(kind, out, sizes)


def parse_ddsperf(kind, out, sizes):
    """Reads ddsperf's once-a-second lines (of Cyclone DDS 0.10.4).
    Latency lines look like `... size 1024 mean 18.9us min 10.5us 50%
    16.8us 90% 21.6us 99% 35.7us max 1340.8us cnt 26205`, throughput
    lines `... size 1024 total 397140 lost 0 delta 397140 lost 0 rate
    397.10 kS/s 3253.06 Mb/s ...`. The first second is the warm-up
    and left out; the latency of the others is the median of their
    per-second values (ddsperf does not give the whole distribution)."""
    unit = {"ns": 1e-3, "us": 1.0, "ms": 1e3, "s": 1e6}

    def us(v, u):
        return float(v) * unit[u]

    lat, thr = [], []
    t = r"([\d.]+)(ns|us|ms|s)"
    for line in out.splitlines():
        m = re.search(
            rf"size (\d+) mean {t} min {t} 50% {t} 90% {t} 99% {t} max {t} cnt (\d+)", line
        )
        if m:
            g = m.groups()
            lat.append({
                "rtt_mean_us": us(g[1], g[2]), "rtt_min_us": us(g[3], g[4]),
                "rtt_p50_us": us(g[5], g[6]), "rtt_p90_us": us(g[7], g[8]),
                "rtt_p99_us": us(g[9], g[10]), "rtt_max_us": us(g[11], g[12]),
                "count": int(g[13]),
            })
            continue
        m = re.search(
            r"size (\d+) total (\d+) lost (\d+) delta (\d+) lost (\d+) rate ([\d.]+) kS/s ([\d.]+) Mb/s",
            line,
        )
        if m:
            thr.append({
                "lost": int(m.group(5)),
                "samples_per_s": float(m.group(6)) * 1e3, "mbit_per_s": float(m.group(7)),
            })
    if kind == "latency":
        lat = lat[1:]
        if not lat:
            return []

        def med(k):
            v = sorted(x[k] for x in lat)
            return v[len(v) // 2]

        r = {k: med(k) for k in lat[0] if k != "count"}
        r["count"] = sum(x["count"] for x in lat)
        r["size"] = sizes[0]
        r["note"] = "ddsperf: median of per-second values; no p99.9"
        return [r]
    thr = thr[1:]
    if not thr:
        return []
    n = len(thr)
    return [{
        "size": sizes[0],
        "samples_per_s": sum(x["samples_per_s"] for x in thr) / n,
        "mbit_per_s": sum(x["mbit_per_s"] for x in thr) / n,
        "lost": sum(x["lost"] for x in thr),
        "samples_per_s_min": min(x["samples_per_s"] for x in thr),
        "samples_per_s_max": max(x["samples_per_s"] for x in thr),
        "seconds": float(n),
        "note": "ddsperf: mean of per-second values; size includes its 12-octet header",
    }]


def make(name):
    return {
        "rustorb": Rustorb,
        "rustorb-listener": RustorbListener,
        "cyclonedds": CycloneDds,
        "fastdds": FastDds,
        "cyclonedds-ddsperf": CycloneDdsperf,
    }[name]()
