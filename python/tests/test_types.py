"""Type declarations, and their samples converted through DynamicData."""

import enum

import pytest

import rustorb_dds as dds


@dds.struct("HelloWorld::Greeting")
class Greeting:
    index: dds.uint32
    message: str


@dds.enum("Color")
class Color(enum.IntEnum):
    RED = 0
    GREEN = 1
    BLUE = 2


@dds.bitmask("Flags", bit_bound=8)
class Flags(enum.IntFlag):
    A = 1
    B = 2
    C = 8


@dds.union(dds.int32, "U")
class U:
    num: dds.case(dds.int32, 1, 2)
    text: dds.case(str, 3)
    other: dds.default_case(dds.float64)


@dds.struct("Inner", extensibility="appendable")
class Inner:
    x: dds.int16
    y: dds.array(dds.float32, 2, 3)


@dds.struct("Everything", extensibility="mutable")
class Everything:
    id: dds.key(dds.uint64)
    flag: bool
    c: dds.char8
    w: dds.wstring(10)
    color: Color
    flags: Flags
    u: U
    inner: Inner
    inners: dds.sequence(Inner, 4)
    blob: bytes
    octets: dds.array(dds.uint8, 4)
    table: dds.map(str, dds.int32)
    note: dds.optional(str)
    big: dds.member(dds.int64, id=100)


def test_greeting_type():
    t = dds.type_of(Greeting)
    assert t.name == "HelloWorld::Greeting"
    assert not t.keyed
    assert Greeting() == Greeting(0, "")
    assert dds.type_of(Greeting) is t


def test_everything_type():
    t = dds.type_of(Everything)
    assert t.keyed
    assert t.describe()[0] == "struct"
    assert dds.DynamicType(t.describe()) == t


def test_zero_values():
    e = Everything()
    assert e.color is Color.RED
    assert e.flags == Flags(0)
    assert e.u == U(1, 0)
    assert e.inner.y == [[0.0] * 3, [0.0] * 3]
    assert e.octets == bytes(4)
    assert e.note is None


def round_trip(cls, value, domain):
    p = dds.DomainParticipant(domain, localhost=True)
    topic = p.create_topic("RoundTrip", cls)
    reader = p.create_subscriber().create_datareader(topic, dds.Qos(reliability="reliable"))
    writer = p.create_publisher().create_datawriter(topic)
    assert writer.wait_for_subscriptions(1, 5.0)
    writer.write(value)
    assert reader.wait_for_data(5.0)
    (s,) = reader.take()
    return s


def test_round_trip_everything():
    e = Everything(
        id=7, flag=True, c="z", w="wide", color=Color.BLUE,
        flags=Flags.A | Flags.C, u=U(3, "three"),
        inner=Inner(5, [[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]),
        inners=[Inner(1), Inner(2)], blob=b"\x00\x01", octets=b"abcd",
        table={"a": 1, "b": -2}, note="hi", big=-(1 << 40),
    )
    s = round_trip(Everything, e, 170)
    assert s.data == e
    assert isinstance(s.data.color, Color)
    assert s.info.valid_data and s.info.instance_state == "alive"


def test_round_trip_default_case():
    e = Everything(id=1, u=U(42, 2.5), note=None)
    s = round_trip(Everything, e, 171)
    assert s.data.u == U(42, 2.5)
    assert s.data.note is None


def test_bad_values():
    p = dds.DomainParticipant(172, localhost=True)
    w = p.create_publisher().create_datawriter(p.create_topic("Bad", Greeting))
    with pytest.raises((ValueError, TypeError, OverflowError)):
        w.write(Greeting(-1, "x"))
    with pytest.raises((ValueError, TypeError)):
        w.write(Greeting(1, 2))


def test_dynamic_type_samples_are_plain():
    t = dds.DynamicType(("struct", "Dyn", "final", False, None, [
        ("a", 0, "int32", True, False, False, False),
        ("b", 1, ("sequence", ("string", 0), 0), False, False, False, False),
    ]))
    s = round_trip(t, {"a": 3, "b": ["x", "y"]}, 173)
    assert s.data == {"a": 3, "b": ["x", "y"]}
