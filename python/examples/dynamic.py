"""DynamicData from Python: a writer or a reader that does not know the
type of its topic beforehand, and gets it from the other side's endpoint
(with TypeLookup when it announces only its hash).

The same as the Rust example ``dynamic``, and the same command line, used
by ``interop/run.sh``::

    dynamic.py pub <domain> <count> <appendable|mutable>
    dynamic.py sub <domain> <count> <timeout_s> <appendable|mutable>

The topic is ``evolve_<kind>``, RELIABLE, KEEP_ALL, XCDR2. Samples have
number 1, 2, 3..., text "evolve <number>", and extra 100 + number when
the type has it.
"""

import sys
import time

import rustorb_dds as dds


def main(argv):
    usage = ("usage: dynamic.py pub <domain> <count> <appendable|mutable>\n"
             "       dynamic.py sub <domain> <count> <timeout_s> <appendable|mutable>")
    if len(argv) < 5 or argv[1] not in ("pub", "sub"):
        sys.exit(usage)
    sub = argv[1] == "sub"
    kind = argv[5] if sub and len(argv) > 5 else argv[4]
    if kind not in ("appendable", "mutable"):
        sys.exit(usage)
    topic_name = "evolve_" + kind
    domain, count = int(argv[2]), int(argv[3])
    wait = int(argv[4]) if sub else 60
    deadline = time.monotonic() + wait
    participant = dds.DomainParticipant(domain)
    print("rustorb-dds (Python): waiting for the type of %s..." % topic_name, flush=True)
    t = participant.discover_topic_type(topic_name, wait)
    if t is None:
        sys.exit("rustorb-dds (Python): the type of the topic did not come")
    members = [m[0] for m in t.describe()[5]]
    print("rustorb-dds (Python): got type %s %s" % (t.name, members), flush=True)
    extra = "extra" in members
    topic = participant.create_topic(topic_name, t)
    qos = dds.Qos(reliability="reliable", history="keep_all", data_representation=["xcdr2"])
    if not sub:
        writer = participant.create_publisher().create_datawriter(topic, qos)
        while not writer.wait_for_subscriptions(1, 1.0):
            pass
        time.sleep(0.5)
        for n in range(1, count + 1):
            sample = {"number": n, "text": "evolve %d" % n}
            if extra:
                sample["extra"] = 100 + n
            writer.write(sample)
            print("rustorb-dds (Python): sent %d" % n, flush=True)
            time.sleep(0.02)
        writer.wait_for_acknowledgments(30.0)
        print("rustorb-dds (Python): all acknowledged", flush=True)
    else:
        reader = participant.create_subscriber().create_datareader(topic, qos)
        got = 0
        while got < count:
            if time.monotonic() > deadline:
                sys.exit("rustorb-dds (Python): got %d of %d samples" % (got, count))
            reader.wait_for_data(0.2)
            for s in reader.take():
                if s.data is None:
                    continue
                got += 1
                want = "evolve %d" % got
                if s.data["number"] != got or s.data["text"] != want:
                    sys.exit("got %r, want %d %r" % (s.data, got, want))
                print("rustorb-dds (Python): received %r" % (s.data,), flush=True)


if __name__ == "__main__":
    main(sys.argv)
