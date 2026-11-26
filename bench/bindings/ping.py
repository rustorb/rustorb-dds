"""ping of bench/bindings.py in Python (rustorb-dds#107): the round trip
through `rustorb-dds-perf pong`, as `rustorb-dds-perf ping` measures it.

    python3 ping.py <domain> <sizes> <warmup_s> <duration_s>

Writes RustorbPerf::Sample on RustorbPerf_ping (reliable, KEEP_LAST 1),
waits on a WaitSet for the sample of the same seq on RustorbPerf_pong,
and prints one line of JSON per size, as the Rust program does.
"""

import json
import math
import sys
import time

import rustorb_dds as dds


@dds.struct("RustorbPerf::Sample")
class Sample:
    seq: dds.uint64
    stamp: dds.uint64
    payload: dds.sequence(dds.byte)


def summary(ns):
    ns.sort()
    n = len(ns)

    def at(p):
        return ns[min(max(math.ceil(p * n), 1), n) - 1] / 1000 if n else 0.0

    return {"count": n, "rtt_min_us": at(0), "rtt_p50_us": at(0.5), "rtt_p90_us": at(0.9),
            "rtt_p99_us": at(0.99), "rtt_max_us": ns[-1] / 1000 if n else 0.0}


def main(argv):
    domain = int(argv[1])
    sizes = [int(s) for s in argv[2].split(",")]
    warmup, duration = float(argv[3]), float(argv[4])
    p = dds.DomainParticipant(domain)
    qos = dds.Qos(reliability="reliable", history=1)
    writer = p.create_publisher().create_datawriter(p.create_topic("RustorbPerf_ping", Sample), qos)
    reader = p.create_subscriber().create_datareader(p.create_topic("RustorbPerf_pong", Sample), qos)
    writer.wait_for_subscriptions(1, 30.0)
    reader.wait_for_publications(1, 30.0)
    status = reader.status_condition
    status.set_enabled_statuses(dds.StatusMask.DATA_AVAILABLE)
    waitset = dds.WaitSet()
    waitset.attach_condition(status)
    start = time.perf_counter_ns()
    seq = 0
    for size in sizes:
        payload = bytes(size)
        reader.take()
        warm_end = time.monotonic() + warmup
        end = warm_end + duration
        rtts, lost = [], 0
        while time.monotonic() < end:
            measuring = time.monotonic() >= warm_end
            seq += 1
            sent = time.perf_counter_ns()
            writer.write(Sample(seq, sent - start, payload))
            deadline = time.monotonic() + 1.0
            while True:
                try:
                    waitset.wait(max(0.0, deadline - time.monotonic()))
                except TimeoutError:
                    lost += measuring
                    break
                if any(s.data is not None and s.data.seq == seq for s in reader.take()):
                    if measuring:
                        rtts.append(time.perf_counter_ns() - sent)
                    break
        r = {"tool": "bench/bindings/ping.py", "impl": "rustorb-dds-python", "mode": "ping",
             "size": size, "reliability": "reliable", "lost": lost, "seconds": duration}
        r.update(summary(rtts))
        print(json.dumps(r), flush=True)


if __name__ == "__main__":
    main(sys.argv)
