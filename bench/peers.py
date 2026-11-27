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

NAMES = ["rustorb", "rustorb-listener", "cyclonedds", "fastdds", "rustdds", "opendds", "cyclonedds-ddsperf"]


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

    # Whether `secure` can turn DDS Security on.
    security = False

    def secure(self, cmd, sec):
        """`cmd` with DDS Security on, configured by the files of `sec`
        (made by `security_files`)."""
        raise NotImplementedError


class Rustorb(Peer):
    name = "rustorb"
    transports = ("udp", "shm", "loan")

    def missing(self):
        return None if shutil.which("cargo") else "no cargo"

    security = True
    # Built with the feature `security` (bench/run.py --security sets it,
    # for every case of the run, so that they all use one program).
    with_security = False

    def build(self):
        features = ["--features", "security"] if Rustorb.with_security else []
        subprocess.run(
            ["cargo", "build", "--release", "--locked", "-p", "rustorb-dds-perf"] + features,
            cwd=ROOT, check=True,
        )

    def secure(self, cmd, sec):
        return Cmd(cmd.argv + ["--security", str(sec / "peer.properties")], cmd.env)

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

    security = True

    def secure(self, cmd, sec):
        env = dict(cmd.env)
        env["CYCLONEDDS_URI"] = env.get("CYCLONEDDS_URI", "") + f",file://{sec / 'cyclonedds.xml'}"
        return Cmd(cmd.argv, env)


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

    security = True

    def secure(self, cmd, sec):
        return Cmd(cmd.argv, {**cmd.env, "FASTRTPS_DEFAULT_PROFILES_FILE": str(sec / "fastdds.xml")})

    def version(self):
        return dpkg_version("libfastrtps-dev")


class RustDds(Peer):
    """bench/rustdds, a program of its own on RustDDS's API (its own
    Cargo.lock, not in the workspace)."""

    name = "rustdds"

    def missing(self):
        return None if shutil.which("cargo") else "no cargo"

    def build(self):
        subprocess.run(["cargo", "build", "--release", "--locked"], cwd=BENCH / "rustdds", check=True)

    def program(self):
        return BENCH / "rustdds" / "target" / "release" / "rustdds-perf"

    def version(self):
        lock = (BENCH / "rustdds" / "Cargo.lock").read_text()
        m = re.search(r'name = "rustdds"\nversion = "([^"]+)"', lock)
        return m and m.group(1)


OPENDDS_IMAGE = os.environ.get("OPENDDS_IMAGE", "ghcr.io/opendds/opendds:DDS-3.31")
# The libraries of the system, left out of those copied from the image.
SYSTEM_LIBS = re.compile(r"/(libc|libm|libdl|libpthread|librt|ld-linux[^/]*)\.so")


class OpenDds(Peer):
    """bench/opendds/perf.cpp on OpenDDS's C++ API, with RTPS discovery
    and the RTPS_UDP transport (bench/opendds/rtps.ini).

    Built in OpenDDS's Docker image (OPENDDS_IMAGE, which has to be
    pulled), as interop/run.sh builds its OpenDDS programs, and run here,
    outside Docker, with the libraries it links copied from the image:
    so that it is pinned to its CPUs and its CPU time and memory are its
    own, as for the others. With OPENDDS_IMAGE empty, the OpenDDS of
    DDS_ROOT here."""

    name = "opendds"

    def missing(self):
        if not OPENDDS_IMAGE:
            return None if os.environ.get("DDS_ROOT") else "no DDS_ROOT (OPENDDS_IMAGE is empty)"
        if not shutil.which("docker"):
            return "no docker"
        r = subprocess.run(["docker", "image", "inspect", OPENDDS_IMAGE], capture_output=True)
        if r.returncode != 0:
            return f"no OpenDDS image (docker pull {OPENDDS_IMAGE})"
        return None

    def build(self):
        out = BUILD / "opendds"
        out.mkdir(parents=True, exist_ok=True)
        script = (
            'cmake -S "$1" -B "$2" -DCMAKE_BUILD_TYPE=Release -DCMAKE_PREFIX_PATH="${DDS_ROOT:-/usr/local}" '
            '>/dev/null && cmake --build "$2" -j "$(nproc)" >/dev/null && mkdir -p "$2/lib" && '
            'ldd "$2/perf" | awk \'/=> \\// {print $3}\' > "$2/libs.txt"'
        )
        run = []
        if OPENDDS_IMAGE:
            df = f"FROM {OPENDDS_IMAGE}\nRUN (command -v cmake && command -v g++) >/dev/null || " \
                 "(apt-get update -qq && apt-get install -y -qq cmake g++ >/dev/null)\n"
            subprocess.run(["docker", "build", "-q", "-t", "rustorb-bench-opendds", "-"],
                           input=df, text=True, check=True, stdout=subprocess.DEVNULL)
            run = ["docker", "run", "--rm", "--user", f"{os.getuid()}:{os.getgid()}",
                   "-v", f"{ROOT}:{ROOT}", "-v", f"{out}:{out}", "rustorb-bench-opendds"]
        subprocess.run(run + ["sh", "-c", script, "sh", str(BENCH / "opendds"), str(out)], check=True)
        if OPENDDS_IMAGE:
            # The libraries perf links, but those of the system, from the image.
            libs = [l for l in (out / "libs.txt").read_text().split() if not SYSTEM_LIBS.search(l)]
            copy = 'for l in "$@"; do cp -L "$l" "$0/"; done'
            subprocess.run(run + ["sh", "-c", copy, str(out / "lib")] + libs, check=True)
            # OpenDDS loads its transport (rtps_udp) by the library's plain
            # name, libOpenDDS_Rtps_Udp.so, as ACE_DLL does.
            for f in (out / "lib").glob("*.so.*"):
                plain = f.with_name(f.name.split(".so.")[0] + ".so")
                if not plain.exists():
                    plain.symlink_to(f.name)

    def program(self):
        return BUILD / "opendds" / "perf"

    def env(self, transport):
        return {"LD_LIBRARY_PATH": str(BUILD / "opendds" / "lib")} if OPENDDS_IMAGE else {}

    def cmd(self, mode, **c):
        cmd = super().cmd(mode, **c)
        cmd.argv += ["-DCPSConfigFile", str(BENCH / "opendds" / "rtps.ini")]
        return cmd

    def version(self):
        return OPENDDS_IMAGE or os.environ.get("DDS_ROOT")


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


# The DDS Security settings of bench/run.py --security and
# bench/discovery.py --security: the protection of discovery, liveliness,
# metadata and data in the governance (the RTPS messages are not
# protected). Every participant has the same identity and permissions.
SECURITY = {"auth": "NONE", "sign": "SIGN", "encrypt": "ENCRYPT"}


def security_files(kind, base):
    """Makes (once) the certificates, governance, permissions and the
    configuration of each implementation for `kind` in base/<kind>, with
    interop/security/gen.sh, and returns that directory."""
    d = Path(base) / kind
    if not (d / "peer.properties").exists():
        subprocess.run(["sh", str(ROOT / "interop" / "security" / "gen.sh"), str(d), "NONE",
                        SECURITY[kind]], check=True)
    return d


def make(name):
    return {
        "rustorb": Rustorb,
        "rustorb-listener": RustorbListener,
        "cyclonedds": CycloneDds,
        "fastdds": FastDds,
        "rustdds": RustDds,
        "opendds": OpenDds,
        "cyclonedds-ddsperf": CycloneDdsperf,
    }[name]()
