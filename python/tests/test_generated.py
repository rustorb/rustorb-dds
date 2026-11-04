"""Types that rustorb-idl declares for rustorb_dds (``-bpython
-Wbdds_only``): tests/generated/ is its output for the IDL files of the
Cyclone DDS fixtures (crates/rustorb-xtypes/tests/fixtures/cyclonedds, see
the README there). Their TypeObjects must hash as idlc's do, so that they
match the same types in other implementations."""

import os
import sys

import pytest

import rustorb_dds as dds

HERE = os.path.dirname(os.path.abspath(__file__))
FIXTURES = os.path.join(HERE, "..", "..", "crates", "rustorb-xtypes", "tests",
                        "fixtures", "cyclonedds")
sys.path.insert(0, os.path.join(HERE, "generated"))

from Generated_dds import Gen  # noqa: E402
from HelloWorld_dds import HelloWorld  # noqa: E402
from Keyed_dds import Keyed  # noqa: E402


def idlc_hashes(name):
    """The minimal and complete hashes of the type of a fixture, from
    idlc's TypeInformation."""
    with open(os.path.join(FIXTURES, name + ".typeinfo")) as f:
        data = bytes.fromhex("".join(f.read().split()))
    # TypeInformation is mutable: the minimal member (0x1001) comes first,
    # then the complete one (0x1002), each starting with the type's
    # TypeIdentifier: EK_MINIMAL (0xf1) or EK_COMPLETE (0xf2) and 14
    # octets of hash.
    i = data.index(b"\xf1")
    j = data.index(b"\xf2", data.index(b"\x02\x10\x00\x40"))
    return data[i + 1:i + 15].hex(), data[j + 1:j + 15].hex()


@pytest.mark.parametrize("fixture,cls", [
    ("Gen_Plain", Gen.Plain),
    ("Gen_Inner", Gen.Inner),
    ("Gen_Mut", Gen.Mut),
    ("Gen_U", Gen.U),
    ("Gen_UE", Gen.UE),
    ("Gen_Hashed", Gen.Hashed),
    ("Gen_All", Gen.All),
    ("Gen_Top", Gen.Top),
    ("HelloWorld_Greeting", HelloWorld.Greeting),
    ("Keyed_Shape", Keyed.Shape),
])
def test_hashes_like_idlc(fixture, cls):
    assert dds.type_of(cls).type_hashes() == idlc_hashes(fixture)


def test_round_trip_of_everything():
    inner = Gen.Inner(id=4, big="x" * 300)
    all_ = Gen.All(
        color=Gen.Color.BLUE,
        perm=Gen.Perm.READ | Gen.Perm.EXEC,
        ls=[1, 2, 3],
        name="name",
        m=[[1, 2, 3], [4, 5, 6]],
        inners=[inner, Gen.Inner(id=5)],
        ss=[[1], [], [2, 3]],
        arr=[inner, Gen.Inner(), Gen.Inner(), Gen.Inner(id=9)],
        big_arr=list(range(300)),
        strs=["a", "b"],
        u=Gen.U(7, inner),
        ue=Gen.UE(Gen.Color.BLUE, "blue"),
        mut=Gen.Mut(a=1, b=None, k=2, inner=inner),
        h=Gen.Hashed(alpha=1, beta="b", gamma=3),
        p=Gen.Plain(b=True, o=255, s=-2, us=3, l=-4, ul=5, ll=-6, ull=7, f=0.5,
                    d=0.25, c="c"),
        ext=inner,
        opt=11,
    )
    top = Gen.Top(id=1, all=all_)
    p = dds.DomainParticipant(175, localhost=True)
    t = p.create_topic("Top", Gen.Top)
    r = p.create_subscriber().create_datareader(t, dds.Qos(reliability="reliable"))
    w = p.create_publisher().create_datawriter(t)
    assert w.wait_for_subscriptions(1, 5.0)
    w.write(top)
    assert r.wait_for_data(5.0)
    (s,) = r.take()
    assert s.data == top
    assert r.lookup_instance(Gen.Top(id=1)) == s.info.instance_handle
