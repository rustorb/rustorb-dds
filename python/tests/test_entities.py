"""Entities, QoS, instances, conditions and listeners.

The Python tests use domains 20 to 36, which the Rust tests do not use.
Domains above 166 have ports in the range Windows may reserve (49152 and
up), where binding fails with WSAEACCES."""

import threading

import pytest

import rustorb_dds as dds


@dds.struct("Keyed::Shape", extensibility="appendable")
class Shape:
    id: dds.key(dds.uint32)
    index: dds.uint32
    message: dds.string(100)


RELIABLE = dds.Qos(reliability="reliable", history="keep_all")


def pair(domain, reader_qos=RELIABLE, writer_qos=RELIABLE, **kw):
    p = dds.DomainParticipant(domain, localhost=True)
    topic = p.create_topic("Shapes", Shape)
    reader = p.create_subscriber(**kw).create_datareader(topic, reader_qos)
    writer = p.create_publisher(**kw).create_datawriter(topic, writer_qos)
    assert writer.wait_for_subscriptions(1, 5.0)
    assert reader.wait_for_publications(1, 5.0)
    return p, writer, reader


def test_instances_and_dispose():
    _, w, r = pair(30)
    for i in range(3):
        w.write(Shape(i % 2, i, "m%d" % i))
    w.dispose(Shape(0))
    w.wait_for_acknowledgments(5.0)
    h0 = r.lookup_instance(Shape(0))
    assert h0 is not None and h0 == w.lookup_instance(Shape(0))
    assert r.get_key_value(h0).id == 0
    one = r.take(instance=r.lookup_instance(Shape(1)))
    assert [s.data.index for s in one] == [1]
    rest = r.take()
    assert [s.data.index for s in rest if s.info.valid_data] == [0, 2]
    disposed = [s for s in rest if not s.info.valid_data]
    assert len(disposed) == 1 and disposed[0].data is None
    assert disposed[0].info.instance_state == "not_alive_disposed"


def test_read_states():
    _, w, r = pair(31)
    w.write(Shape(1, 1, "a"))
    w.wait_for_acknowledgments(5.0)
    assert r.wait_for_data(5.0)
    assert len(r.read()) == 1
    assert r.read(sample_states=["not_read"]) == []
    assert len(r.take(sample_states=["read"])) == 1
    with pytest.raises(ValueError):
        r.read(sample_states=["bogus"])


def test_waitset_conditions():
    _, w, r = pair(32)
    ws = dds.WaitSet()
    guard = dds.GuardCondition()
    rc = r.create_readcondition(sample_states=["not_read"])
    sc = r.status_condition
    sc.set_enabled_statuses(dds.StatusMask.DATA_AVAILABLE)
    assert sc.enabled_statuses == dds.StatusMask.DATA_AVAILABLE
    for c in (guard, rc, sc):
        ws.attach_condition(c)
    with pytest.raises(TimeoutError):
        ws.wait(0.1)
    w.write(Shape(1, 1, "a"))
    active = ws.wait(5.0)
    assert rc in active
    assert [s.data.index for s in r.take_w_condition(rc)] == [1]
    guard.set_trigger_value(True)
    assert guard in ws.wait(1.0)


class ReaderListener(dds.DataReaderListener):
    def __init__(self):
        self.data = threading.Event()
        self.matched = []

    def on_data_available(self, reader):
        self.data.set()

    def on_subscription_matched(self, reader, status):
        self.matched.append(status.current_count)


class WriterListener(dds.DataWriterListener):
    def __init__(self):
        self.matched = threading.Event()

    def on_publication_matched(self, writer, status):
        if status.current_count:
            self.matched.set()


def test_listeners():
    p = dds.DomainParticipant(33, localhost=True)
    topic = p.create_topic("Shapes", Shape)
    rl, wl = ReaderListener(), WriterListener()
    r = p.create_subscriber().create_datareader(topic, RELIABLE, listener=rl)
    w = p.create_publisher().create_datawriter(topic, RELIABLE, listener=wl)
    assert wl.matched.wait(5.0)
    w.write(Shape(3, 3, "x"))
    assert rl.data.wait(5.0)
    assert 1 in rl.matched
    assert r.subscription_matched_status().current_count == 1
    assert w.publication_matched_status().current_count == 1
    r.set_listener(None)


def test_partitions_and_incompatible_qos():
    p = dds.DomainParticipant(34, localhost=True)
    topic = p.create_topic("Shapes", Shape)
    r = p.create_subscriber(partition=["a*"]).create_datareader(topic, RELIABLE)
    w = p.create_publisher(partition=["abc"]).create_datawriter(topic, RELIABLE)
    assert w.wait_for_subscriptions(1, 5.0)
    other = p.create_publisher(partition=["x"]).create_datawriter(topic, RELIABLE)
    assert not other.wait_for_subscriptions(1, 0.5)
    be = p.create_publisher(partition=["abc"]).create_datawriter(
        topic, dds.Qos(reliability="best_effort"))
    assert not be.wait_for_subscriptions(1, 1.0)
    assert be.offered_incompatible_qos_status().total_count >= 1


def test_transient_local_late_joiner():
    p = dds.DomainParticipant(35, localhost=True)
    topic = p.create_topic("Shapes", Shape)
    tl = dds.Qos(reliability="reliable", durability="transient_local", history=10)
    w = p.create_publisher().create_datawriter(topic, tl)
    for i in range(3):
        w.write(Shape(i, i, ""))
    r = p.create_subscriber().create_datareader(topic, tl)
    deadline = 50
    got = []
    while len(got) < 3 and deadline:
        r.wait_for_data(0.1)
        got += r.take()
        deadline -= 1
    assert sorted(s.data.index for s in got) == [0, 1, 2]


def test_discover_topic_type():
    p = dds.DomainParticipant(36, localhost=True)
    w = p.create_publisher().create_datawriter(p.create_topic("Shapes", Shape))
    q = dds.DomainParticipant(36, localhost=True)
    t = q.discover_topic_type("Shapes", 5.0)
    assert t == dds.type_of(Shape)
    r = q.create_subscriber().create_datareader(q.create_topic("Shapes", t), RELIABLE)
    assert w.wait_for_subscriptions(1, 5.0)
    w.write(Shape(9, 1, "dyn"))
    assert r.wait_for_data(5.0)
    assert r.take()[0].data == {"id": 9, "index": 1, "message": "dyn"}


def test_content_filtered_topic():
    p = dds.DomainParticipant(28, localhost=True)
    topic = p.create_topic("FilteredShapes", Shape)
    cft = p.create_contentfilteredtopic(
        "BigShapes", topic, "index > %0 AND message LIKE 'm%'", [2])
    assert cft.name == "BigShapes"
    assert cft.type_name == "Keyed::Shape"
    assert cft.related_topic is topic
    assert cft.expression_parameters == ["2"]
    reader = p.create_subscriber().create_datareader(cft, RELIABLE)
    writer = p.create_publisher().create_datawriter(topic, RELIABLE)
    assert writer.wait_for_subscriptions(1, 5.0)
    for i in range(6):
        writer.write(Shape(i, i, "x" if i == 4 else "m"))
    writer.wait_for_acknowledgments(5.0)
    assert reader.wait_for_data(5.0)
    got = []
    for _ in range(50):
        got += [s.data.index for s in reader.take() if s.data]
        if len(got) >= 2:
            break
        threading.Event().wait(0.05)
    assert got == [3, 5]
    cft.set_expression_parameters(["4"])
    assert cft.expression_parameters == ["4"]
    with pytest.raises(ValueError):
        cft.set_expression_parameters([])
    with pytest.raises(ValueError):
        p.create_contentfilteredtopic("Bad", topic, "nope = 1")
