"""Keyed instances over DDS, from Python: write, dispose and unregister.

The same as the Rust example ``keyed`` (topic ``keyed`` of
``Keyed::Shape``, keyed by ``id``), and the same command line, used by
``interop/run.sh``::

    keyed.py pub <domain> <count> <interval_ms>
    keyed.py sub <domain> <count> <timeout_s>

The publisher writes ``count`` samples to the instances id 1, 2, 3 in
turn, then disposes id 2 and unregisters id 3. The subscriber exits once
it has ``count`` samples, each instance's in order, and has seen id 2
NOT_ALIVE_DISPOSED and id 3 NOT_ALIVE_NO_WRITERS.
"""

import sys
import time

import rustorb_dds as dds


@dds.struct("Keyed::Shape")
class Shape:
    id: dds.key(dds.uint32)
    index: dds.uint32
    message: str


MESSAGE = "Hello, instance!"


def main(argv):
    role = argv[1] if len(argv) > 1 else "pub"
    domain = int(argv[2]) if len(argv) > 2 else 0
    count = int(argv[3]) if len(argv) > 3 else 30
    participant = dds.DomainParticipant(domain)
    topic = participant.create_topic("keyed", Shape)
    if role == "pub":
        interval = (int(argv[4]) if len(argv) > 4 else 100) / 1000
        writer = participant.create_publisher().create_datawriter(topic, dds.Qos(
            reliability="reliable", history="keep_all",
            autodispose_unregistered_instances=False))
        print("rustorb-dds (Python): waiting for a subscriber...", flush=True)
        while not writer.wait_for_subscriptions(1, 1.0):
            pass
        time.sleep(0.5)
        for index in range(1, count + 1):
            id_ = (index - 1) % 3 + 1
            writer.write(Shape(id_, index, MESSAGE))
            print("rustorb-dds (Python): sent id %d index %d" % (id_, index), flush=True)
            time.sleep(interval)
        writer.dispose(Shape(2))
        writer.unregister_instance(Shape(3))
        print("rustorb-dds (Python): disposed 2, unregistered 3", flush=True)
        writer.wait_for_acknowledgments(30.0)
        print("rustorb-dds (Python): all acknowledged", flush=True)
    elif role == "sub":
        deadline = time.monotonic() + (int(argv[4]) if len(argv) > 4 else 3600)
        reader = participant.create_subscriber().create_datareader(
            topic, dds.Qos(reliability="reliable", history="keep_all"))
        print("rustorb-dds (Python): waiting for shapes...", flush=True)
        got = 0
        last = {}
        disposed = no_writers = False
        while got < count or not disposed or not no_writers:
            if time.monotonic() >= deadline:
                sys.exit("rustorb-dds (Python): got %d of %d samples, disposed %s, "
                         "no writers %s" % (got, count, disposed, no_writers))
            if not reader.wait_for_data(0.2):
                continue
            for s in reader.take():
                d = s.data
                if d is not None:
                    if (d.message != MESSAGE or d.index <= last.get(d.id, 0)
                            or (d.index - 1) % 3 + 1 != d.id):
                        sys.exit("bad sample %r" % (d,))
                    last[d.id] = d.index
                    got += 1
                    print("rustorb-dds (Python): received id %d index %d"
                          % (d.id, d.index), flush=True)
                else:
                    k = reader.get_key_value(s.info.instance_handle)
                    id_ = k.id if k is not None else 0
                    state = s.info.instance_state
                    print("rustorb-dds (Python): id %d is %s" % (id_, state), flush=True)
                    disposed |= id_ == 2 and state == "not_alive_disposed"
                    no_writers |= id_ == 3 and state == "not_alive_no_writers"
    else:
        sys.exit("unknown role %r: use pub or sub" % role)


if __name__ == "__main__":
    main(sys.argv)
