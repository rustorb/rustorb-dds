"""Talks with OpenRTM-aist 2.0 data ports over its Fast DDS transport,
from Python: the same as the Rust example ``openrtm``, with the same
command line::

    openrtm.py out [domain [count [interval_ms [topic]]]]
    openrtm.py in [domain [count [timeout_s [topic]]]]

``out`` waits for a reader, then writes ``RTC::TimedLong`` samples with
data 1, 2, 3...; ``in`` prints what it gets and exits after ``count``
samples, with an error on timeout. The topic is ``chatter`` by default,
as in OpenRTM-aist.

The types are those ``rustorb-idl -bpython -Wbdds_only
-Wbextensibility=final`` writes for OpenRTM-aist's BasicDataType.idl:
its output ``BasicDataType_dds.py`` when it can be imported (on
``PYTHONPATH``), else the same declarations, written here.
"""

import sys
import time

import rustorb_dds as dds


try:
    from BasicDataType_dds import RTC

    Time, TimedLong = RTC.Time, RTC.TimedLong
except ImportError:

    @dds.struct("RTC::Time")
    class Time:
        sec: dds.uint32
        nsec: dds.uint32

    @dds.struct("RTC::TimedLong")
    class TimedLong:
        tm: Time
        data: dds.int32


def main(argv):
    role = argv[1] if len(argv) > 1 else "in"
    domain = int(argv[2]) if len(argv) > 2 else 0
    count = int(argv[3]) if len(argv) > 3 else None
    topic_name = argv[5] if len(argv) > 5 else "chatter"
    participant = dds.DomainParticipant(domain)
    topic = participant.create_topic(topic_name, TimedLong)
    # OpenRTM-aist's ports by default: best effort, keep last 1.
    qos = dds.Qos(reliability="best_effort", history=1)
    if role == "out":
        interval = (int(argv[4]) if len(argv) > 4 else 1000) / 1000
        writer = participant.create_publisher().create_datawriter(topic, qos)
        while not writer.wait_for_subscriptions(1, 1.0):
            pass
        time.sleep(0.5)
        i = 0
        while count is None or i < count:
            i += 1
            now = time.time_ns()
            writer.write(TimedLong(Time(now // 10**9, now % 10**9), i))
            print("rustorb-dds (Python): write %d" % i, flush=True)
            time.sleep(interval)
    elif role == "in":
        deadline = time.monotonic() + (int(argv[4]) if len(argv) > 4 else 60)
        reader = participant.create_subscriber().create_datareader(topic, qos)
        status = reader.status_condition
        status.set_enabled_statuses(dds.StatusMask.DATA_AVAILABLE)
        waitset = dds.WaitSet()
        waitset.attach_condition(status)
        got = 0
        while count is None or got < count:
            try:
                waitset.wait(max(0.0, deadline - time.monotonic()))
            except TimeoutError:
                sys.exit("rustorb-dds (Python): got %d samples" % got)
            for s in reader.take():
                if s.data is None:
                    continue
                print("rustorb-dds (Python): read %d (tm %d.%09d)"
                      % (s.data.data, s.data.tm.sec, s.data.tm.nsec), flush=True)
                got += 1
    else:
        sys.exit("unknown role %r: use out or in" % role)


if __name__ == "__main__":
    main(sys.argv)
