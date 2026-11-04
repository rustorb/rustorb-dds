"""rustorb-dds: DDS (the OMG Data Distribution Service) with its core in
Rust.

A topic's type is a class declared with :func:`struct` (see
:mod:`rustorb_dds.types`), or a :class:`DynamicType`, such as one
:meth:`DomainParticipant.discover_topic_type` learns from the network::

    import rustorb_dds as dds

    @dds.struct("HelloWorld::Greeting")
    class Greeting:
        index: dds.uint32
        message: str

    participant = dds.DomainParticipant(0)
    topic = participant.create_topic("HelloWorldTopic", Greeting)
    writer = participant.create_publisher().create_datawriter(topic)
    writer.write(Greeting(1, "hello"))

    reader = participant.create_subscriber().create_datareader(topic)
    if reader.wait_for_data(5.0):
        for s in reader.take():
            print(s.data)
"""

import dataclasses
import enum as _enum
import threading
import types as _types
import typing

from . import _native
from .types import *  # noqa: F401,F403
from .types import __all__ as _types_all
from .types import spec_of, type_of

__version__ = "0.1.0"

__all__ = _types_all + [
    "DomainParticipant", "Topic", "Publisher", "Subscriber", "DataWriter",
    "DataReader", "Qos", "Sample", "SampleInfo", "StatusMask",
    "DataWriterListener", "DataReaderListener", "Condition",
    "GuardCondition", "StatusCondition", "ReadCondition", "WaitSet",
]


class StatusMask(_enum.IntFlag):
    """The communication statuses (DDS 2.2.4.1)."""

    NONE = 0
    INCONSISTENT_TOPIC = 1 << 0
    OFFERED_DEADLINE_MISSED = 1 << 1
    REQUESTED_DEADLINE_MISSED = 1 << 2
    OFFERED_INCOMPATIBLE_QOS = 1 << 5
    REQUESTED_INCOMPATIBLE_QOS = 1 << 6
    SAMPLE_LOST = 1 << 7
    SAMPLE_REJECTED = 1 << 8
    DATA_ON_READERS = 1 << 9
    DATA_AVAILABLE = 1 << 10
    LIVELINESS_LOST = 1 << 11
    LIVELINESS_CHANGED = 1 << 12
    PUBLICATION_MATCHED = 1 << 13
    SUBSCRIPTION_MATCHED = 1 << 14
    ALL = 0x7FE7


@dataclasses.dataclass
class Qos:
    """The QoS of a writer or reader; None leaves a policy at its
    default. Times are in seconds.

    - ``reliability``: ``"reliable"`` (a writer's default) or
      ``"best_effort"`` (a reader's default); ``max_blocking_time``.
    - ``history``: the depth of KEEP_LAST (1 by default), or
      ``"keep_all"``.
    - ``durability``: ``"volatile"`` (default), ``"transient_local"``.
    - ``deadline``: the period.
    - ``liveliness``: ``"automatic"`` (default),
      ``"manual_by_participant"``, ``"manual_by_topic"``;
      ``lease_duration`` (None: infinite).
    - ``data_representation``: ``["xcdr"]``, ``["xcdr2"]``...
    - ``user_data``: bytes.
    - ``max_samples``, ``max_instances``, ``max_samples_per_instance``.
    - ``autodispose_unregistered_instances`` (writers; True by default).
    """

    reliability: typing.Optional[str] = None
    max_blocking_time: typing.Optional[float] = None
    history: typing.Union[int, str, None] = None
    durability: typing.Optional[str] = None
    deadline: typing.Optional[float] = None
    liveliness: typing.Optional[str] = None
    lease_duration: typing.Optional[float] = None
    data_representation: typing.Optional[typing.List[str]] = None
    user_data: typing.Optional[bytes] = None
    max_samples: typing.Optional[int] = None
    max_instances: typing.Optional[int] = None
    max_samples_per_instance: typing.Optional[int] = None
    autodispose_unregistered_instances: typing.Optional[bool] = None

    def _dict(self):
        return {k: v for k, v in dataclasses.asdict(self).items() if v is not None}


def _qos(qos):
    if qos is None:
        return None
    if isinstance(qos, Qos):
        return qos._dict()
    if isinstance(qos, dict):
        return dict(qos)
    raise TypeError("a QoS is a rustorb_dds.Qos or a dict")


def _status(d):
    return _types.SimpleNamespace(**d)


@dataclasses.dataclass(frozen=True)
class SampleInfo:
    """The information about a sample (DDS 2.2.2.5.5)."""

    sample_state: str
    view_state: str
    instance_state: str
    valid_data: bool
    source_timestamp: typing.Optional[float]
    instance_handle: str
    publication_handle: str
    sequence_number: int
    disposed_generation_count: int
    no_writers_generation_count: int
    sample_rank: int
    generation_rank: int
    absolute_generation_rank: int


class Sample(typing.NamedTuple):
    """A sample: ``data`` is None when the sample only tells a change of
    the instance state (``info.valid_data`` is False)."""

    data: typing.Any
    info: SampleInfo


class DomainParticipant:
    """A participant in the domain ``domain_id``. ``localhost`` keeps
    its traffic on the loopback interface."""

    def __init__(self, domain_id=0, localhost=False):
        self._native = _native.DomainParticipant(domain_id, localhost)

    @property
    def domain_id(self):
        return self._native.domain_id

    @property
    def guid(self):
        return self._native.guid

    def create_topic(self, name, type):  # noqa: A002 - the DDS name
        """A topic named ``name`` of ``type``: a class made with
        :func:`struct` or :func:`union`, or a :class:`DynamicType`."""
        spec = spec_of(type)
        return Topic(self._native.create_topic(name, type_of(type)), spec)

    def discover_topic_type(self, topic_name, timeout=5.0):
        """The type remote endpoints of ``topic_name`` announce, fetched
        with TypeLookup when needed, or None after ``timeout`` seconds."""
        return self._native.discover_topic_type(topic_name, timeout)

    def create_publisher(self, partition=None):
        """A publisher in the partitions ``partition`` (a list of names,
        with ``*`` and ``?`` wildcards)."""
        qos = {"partition": list(partition)} if partition else None
        return Publisher(self._native.create_publisher(qos), self)

    def create_subscriber(self, partition=None):
        """A subscriber in the partitions ``partition``."""
        qos = {"partition": list(partition)} if partition else None
        return Subscriber(self._native.create_subscriber(qos), self)

    def assert_liveliness(self):
        self._native.assert_liveliness()

    def discovered_participants(self):
        """The GUIDs of the remote participants discovered."""
        return self._native.discovered_participants()


class Topic:
    """A topic: its name and its type."""

    def __init__(self, native, spec):
        self._native = native
        self._spec = spec

    @property
    def name(self):
        return self._native.name

    @property
    def type_name(self):
        return self._native.type_name

    @property
    def type(self):  # noqa: A003 - the DDS name
        """The :class:`DynamicType` of the topic."""
        return self._native.type

    def inconsistent_topic_status(self):
        return _status(self._native.inconsistent_topic_status())


class Publisher:
    def __init__(self, native, participant):
        self._native = native
        self.participant = participant

    def create_datawriter(self, topic, qos=None, listener=None,
                          mask=StatusMask.ALL):
        """A writer of ``topic`` with ``qos`` (a :class:`Qos` or dict)."""
        e = DataWriter(None, topic)
        native = self._native.create_datawriter(
            topic._native, _qos(qos),
            None if listener is None else _callback(e, listener), int(mask))
        _created(e, native)
        return e


class Subscriber:
    def __init__(self, native, participant):
        self._native = native
        self.participant = participant

    def create_datareader(self, topic, qos=None, listener=None,
                          mask=StatusMask.ALL):
        """A reader of ``topic`` with ``qos`` (a :class:`Qos` or dict)."""
        e = DataReader(None, topic)
        native = self._native.create_datareader(
            topic._native, _qos(qos),
            None if listener is None else _callback(e, listener), int(mask))
        _created(e, native)
        return e


class DataWriterListener:
    """Override the methods for the statuses to be told of; each is called
    on a thread of the participant with the writer and the status."""

    def on_publication_matched(self, writer, status):
        pass

    def on_offered_incompatible_qos(self, writer, status):
        pass

    def on_liveliness_lost(self, writer, status):
        pass

    def on_offered_deadline_missed(self, writer, status):
        pass


class DataReaderListener:
    """Override the methods for the statuses to be told of; each is called
    on a thread of the participant with the reader (and the status)."""

    def on_data_available(self, reader):
        pass

    def on_subscription_matched(self, reader, status):
        pass

    def on_requested_incompatible_qos(self, reader, status):
        pass

    def on_liveliness_changed(self, reader, status):
        pass

    def on_requested_deadline_missed(self, reader, status):
        pass

    def on_sample_lost(self, reader, status):
        pass

    def on_sample_rejected(self, reader, status):
        pass


def _callback(entity, listener):
    def call(event, status):
        f = getattr(listener, "on_" + event, None)
        if f is None:
            return
        with entity._lock:
            if entity._native is None:
                # Called while the entity is created: told once it is.
                entity._early.append((f, status))
                return
        if status is None:
            f(entity)
        else:
            f(entity, _status(status))

    return call


def _created(entity, native):
    """Sets the native entity of ``entity``, and calls its listener for
    what happened while it was created."""
    with entity._lock:
        entity._native = native
        early, entity._early = entity._early, []
    for f, status in early:
        if status is None:
            f(entity)
        else:
            f(entity, _status(status))


class DataWriter:
    """A writer. A sample is an instance of the topic's class (or a dict
    of its members), or, for a :class:`DynamicType`, a dict."""

    def __init__(self, native, topic):
        self._native = native
        self._lock = threading.Lock()
        self._early = []
        self.topic = topic
        self._encode = topic._spec.encode

    @property
    def guid(self):
        return self._native.guid

    def write(self, sample, timestamp=None):
        """Writes ``sample``; ``timestamp`` (seconds since the epoch) is
        its source timestamp, now by default."""
        self._native.write(self._encode(sample), timestamp)

    def register_instance(self, sample):
        """Registers the instance of the key of ``sample``; returns its
        handle."""
        return self._native.register_instance(self._encode(sample))

    def unregister_instance(self, sample, timestamp=None):
        self._native.unregister_instance(self._encode(sample), timestamp)

    def dispose(self, sample, timestamp=None):
        self._native.dispose(self._encode(sample), timestamp)

    def lookup_instance(self, sample):
        """The handle of the instance of the key of ``sample``, or None."""
        return self._native.lookup_instance(self._encode(sample))

    def wait_for_acknowledgments(self, timeout):
        """Waits until the reliable readers acknowledged all the samples
        written (``TimeoutError`` after ``timeout`` seconds)."""
        self._native.wait_for_acknowledgments(timeout)

    def wait_for_subscriptions(self, count=1, timeout=5.0):
        """Waits until ``count`` readers match; False on timeout."""
        return self._native.wait_for_subscriptions(count, timeout)

    def matched_subscriptions(self):
        return self._native.matched_subscriptions()

    def assert_liveliness(self):
        self._native.assert_liveliness()

    def publication_matched_status(self):
        return _status(self._native.publication_matched_status())

    def offered_incompatible_qos_status(self):
        return _status(self._native.offered_incompatible_qos_status())

    def liveliness_lost_status(self):
        return _status(self._native.liveliness_lost_status())

    def offered_deadline_missed_status(self):
        return _status(self._native.offered_deadline_missed_status())

    def status_changes(self):
        return StatusMask(self._native.status_changes())

    @property
    def status_condition(self):
        return StatusCondition(self._native.status_condition())

    def set_listener(self, listener, mask=StatusMask.ALL):
        """Sets a :class:`DataWriterListener` for the statuses in
        ``mask``; None clears it."""
        if listener is None:
            self._native.clear_listener()
        else:
            self._native.set_listener(_callback(self, listener), int(mask))


_ANY = None


class DataReader:
    """A reader. Samples are instances of the topic's class or, for a
    :class:`DynamicType`, plain values (dicts for structs)."""

    def __init__(self, native, topic):
        self._native = native
        self._lock = threading.Lock()
        self._early = []
        self.topic = topic
        self._decode = topic._spec.decode

    @property
    def guid(self):
        return self._native.guid

    def _samples(self, v):
        return [Sample(None if d is None else self._decode(d), SampleInfo(**i))
                for d, i in v]

    def take(self, sample_states=_ANY, view_states=_ANY, instance_states=_ANY,
             instance=None):
        """Takes the samples in the given states (lists of state names:
        ``"read"``/``"not_read"``, ``"new"``/``"not_new"``, ``"alive"``/
        ``"not_alive_disposed"``/``"not_alive_no_writers"``; None for
        any), or those of the ``instance`` handle."""
        return self._samples(self._native.select(
            True, sample_states, view_states, instance_states, instance))

    def read(self, sample_states=_ANY, view_states=_ANY, instance_states=_ANY,
             instance=None):
        """Like :meth:`take`, leaving the samples in the reader."""
        return self._samples(self._native.select(
            False, sample_states, view_states, instance_states, instance))

    def take_w_condition(self, condition):
        return self._samples(self._native.select_w_condition(True, condition._native))

    def read_w_condition(self, condition):
        return self._samples(self._native.select_w_condition(False, condition._native))

    def lookup_instance(self, sample):
        """The handle of the instance of the key of ``sample``, or None."""
        return self._native.lookup_instance(self.topic._spec.encode(sample))

    def get_key_value(self, instance):
        """A sample holding the key of the ``instance`` handle, or None."""
        v = self._native.get_key_value(instance)
        return None if v is None else self._decode(v)

    def wait_for_data(self, timeout=5.0):
        """Waits until there are samples to take; False on timeout."""
        return self._native.wait_for_data(timeout)

    def wait_for_publications(self, count=1, timeout=5.0):
        """Waits until ``count`` writers match; False on timeout."""
        return self._native.wait_for_publications(count, timeout)

    def matched_publications(self):
        return self._native.matched_publications()

    def subscription_matched_status(self):
        return _status(self._native.subscription_matched_status())

    def requested_incompatible_qos_status(self):
        return _status(self._native.requested_incompatible_qos_status())

    def liveliness_changed_status(self):
        return _status(self._native.liveliness_changed_status())

    def requested_deadline_missed_status(self):
        return _status(self._native.requested_deadline_missed_status())

    def sample_lost_status(self):
        return _status(self._native.sample_lost_status())

    def sample_rejected_status(self):
        return _status(self._native.sample_rejected_status())

    def status_changes(self):
        return StatusMask(self._native.status_changes())

    @property
    def status_condition(self):
        return StatusCondition(self._native.status_condition())

    def create_readcondition(self, sample_states=_ANY, view_states=_ANY,
                             instance_states=_ANY):
        """A condition true while the reader holds samples in the given
        states."""
        return ReadCondition(self._native.create_readcondition(
            sample_states, view_states, instance_states))

    def set_listener(self, listener, mask=StatusMask.ALL):
        """Sets a :class:`DataReaderListener` for the statuses in
        ``mask``; None clears it."""
        if listener is None:
            self._native.clear_listener()
        else:
            self._native.set_listener(_callback(self, listener), int(mask))


class Condition:
    """A condition a :class:`WaitSet` waits for."""

    def __init__(self, native):
        self._native = native

    @property
    def trigger_value(self):
        return self._native.trigger_value

    def __eq__(self, other):
        return isinstance(other, Condition) and self._native == other._native

    __hash__ = None


class GuardCondition(Condition):
    """A condition the application triggers."""

    def __init__(self):
        super().__init__(_native.Condition.guard())

    def set_trigger_value(self, value):
        self._native.set_trigger_value(value)


class StatusCondition(Condition):
    """True while a status in its enabled statuses has changed."""

    @property
    def enabled_statuses(self):
        return StatusMask(self._native.enabled_statuses)

    def set_enabled_statuses(self, mask):
        self._native.set_enabled_statuses(int(mask))


class ReadCondition(Condition):
    """True while its reader holds samples in its states."""


class WaitSet:
    """Waits for conditions."""

    def __init__(self):
        self._native = _native.WaitSet()
        self._conditions = []

    def attach_condition(self, condition):
        self._native.attach_condition(condition._native)
        if condition not in self._conditions:
            self._conditions.append(condition)

    def detach_condition(self, condition):
        self._native.detach_condition(condition._native)
        self._conditions.remove(condition)

    @property
    def conditions(self):
        return list(self._conditions)

    def wait(self, timeout):
        """Waits until a condition is true; returns those that are, or
        raises ``TimeoutError`` after ``timeout`` seconds."""
        return [self._conditions[i] for i in self._native.wait(timeout)]
