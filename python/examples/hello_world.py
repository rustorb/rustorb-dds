"""Hello world over DDS, from Python.

In one terminal: ``python hello_world.py sub``
In another:      ``python hello_world.py pub``

The same as the Rust example ``hello_world`` (topic ``hello_world`` of
``HelloWorld::Greeting``), and the same command line, used by
``interop/run.sh``::

    hello_world.py pub <domain> <count> <interval_ms> [best_effort|reliable|transient_local]
    hello_world.py sub <domain> <count> <timeout_s> [best_effort|reliable|transient_local]

The subscriber exits once it has ``count`` samples (all of them, in
order, when reliable), with an error on timeout.

``RUSTORB_DDS_SECURITY=<file>`` turns on DDS Security, as in the Rust
example: the file has the ``dds.sec.*`` properties of the participant
QoS, one ``name=value`` a line.
"""

import os
import sys
import time

import rustorb_dds as dds


@dds.struct("HelloWorld::Greeting")
class Greeting:
    index: dds.uint32
    message: str


MESSAGE = "Hello, world!"


def security_properties():
    """The properties in the file named by RUSTORB_DDS_SECURITY, if any."""
    path = os.environ.get("RUSTORB_DDS_SECURITY")
    if not path:
        return None
    props = {}
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#"):
                name, _, value = line.partition("=")
                props[name.strip()] = value.strip()
    return props


def main(argv):
    role = argv[1] if len(argv) > 1 else "pub"
    domain = int(argv[2]) if len(argv) > 2 else 0
    count = int(argv[3]) if len(argv) > 3 else 10
    kind = argv[5] if len(argv) > 5 else "best_effort"
    if kind not in ("best_effort", "reliable", "transient_local"):
        sys.exit("unknown reliability %r" % kind)
    reliable = kind != "best_effort"
    durability = "transient_local" if kind == "transient_local" else "volatile"
    participant = dds.DomainParticipant(domain, properties=security_properties())
    topic = participant.create_topic("hello_world", Greeting)
    if role == "pub":
        interval = (int(argv[4]) if len(argv) > 4 else 1000) / 1000
        qos = (dds.Qos(reliability="reliable", history="keep_all", durability=durability)
               if reliable else dds.Qos(reliability="best_effort"))
        writer = participant.create_publisher().create_datawriter(topic, qos)
        if durability == "volatile":
            print("rustorb-dds (Python): waiting for a subscriber...", flush=True)
            while not writer.wait_for_subscriptions(1, 1.0):
                pass
            time.sleep(0.5)
        for index in range(1, count + 1):
            writer.write(Greeting(index, MESSAGE))
            print("rustorb-dds (Python): sent %d" % index, flush=True)
            time.sleep(interval)
        if durability == "transient_local":
            while not writer.wait_for_subscriptions(1, 1.0):
                pass
        if reliable:
            writer.wait_for_acknowledgments(30.0)
            print("rustorb-dds (Python): all acknowledged", flush=True)
    elif role == "sub":
        deadline = time.monotonic() + (int(argv[4]) if len(argv) > 4 else 3600)
        qos = dds.Qos(reliability="reliable" if reliable else "best_effort",
                      history="keep_all", durability=durability)
        if durability == "transient_local":
            time.sleep(2)
        reader = participant.create_subscriber().create_datareader(topic, qos)
        status = reader.status_condition
        status.set_enabled_statuses(dds.StatusMask.DATA_AVAILABLE)
        waitset = dds.WaitSet()
        waitset.attach_condition(status)
        print("rustorb-dds (Python): waiting for greetings...", flush=True)
        got = 0
        while got < count:
            try:
                waitset.wait(max(0.0, deadline - time.monotonic()))
            except TimeoutError:
                sys.exit("rustorb-dds (Python): got %d of %d samples" % (got, count))
            for s in reader.take():
                if s.data is None:
                    continue
                if s.data.message != MESSAGE:
                    sys.exit("bad message %r for %d" % (s.data.message, s.data.index))
                print("rustorb-dds (Python): received %d" % s.data.index, flush=True)
                got += 1
                if reliable and s.data.index != got:
                    sys.exit("expected %d, got %d" % (got, s.data.index))
    else:
        sys.exit("unknown role %r: use pub or sub" % role)


if __name__ == "__main__":
    main(sys.argv)
