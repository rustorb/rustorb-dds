"""Types of DDS topics, declared in Python.

A struct is a class whose annotations are its members, in order::

    import rustorb_dds as dds

    @dds.struct("HelloWorld::Greeting")
    class Greeting:
        index: dds.uint32
        message: str

    @dds.struct("Keyed::Shape", extensibility="appendable")
    class Shape:
        id: dds.key(dds.uint32)
        index: dds.uint32
        message: dds.string(100)
        note: dds.optional(str)

The class becomes a dataclass whose members default to the zero of their
type. A member's type is one of:

- ``bool``, ``int`` (``int32``), ``float`` (``float64``), ``str`` (an
  unbounded string), ``bytes`` (``sequence(uint8)``);
- ``boolean``, ``byte``, ``int8``, ``uint8``, ``int16``, ``uint16``,
  ``int32``, ``uint32``, ``int64``, ``uint64``, ``float32``,
  ``float64``, ``float128``, ``char8``, ``char16``;
- ``string(bound)``, ``wstring(bound)``, ``sequence(t, bound)``,
  ``array(t, *dims)``, ``map(k, v, bound)``;
- a class made with :func:`struct`, :func:`union`, :func:`enum` or
  :func:`bitmask`;
- :func:`key`, :func:`optional` or :func:`member` of one of those, for
  the member's annotations.

Member IDs are 0, 1, 2... in order (``@autoid(SEQUENTIAL)``), or the
hashes of the names with ``autoid="hash"``, unless :func:`member` gives
one. :func:`typedef` is an IDL typedef.

rustorb-idl writes these declarations from IDL (``-bpython
-Wbdds_only``).

In samples, enums are :class:`enum.IntEnum` members (an unknown value
stays an int), bitmasks :class:`enum.IntFlag`, unions instances of their
class with ``discriminator`` and ``value``, sequences and arrays lists
(bytes for octets), maps dicts.
"""

import dataclasses
import enum as _enum
import typing

from . import _native

__all__ = [
    "boolean", "byte", "int8", "uint8", "int16", "uint16", "int32",
    "uint32", "int64", "uint64", "float32", "float64", "float128",
    "char8", "char16", "string", "wstring", "sequence", "array", "map",
    "key", "optional", "member", "case", "default_case", "struct", "union",
    "enum", "bitmask", "typedef", "type_of", "DynamicType",
]

DynamicType = _native.DynamicType


class Spec:
    """How a type is described to the native module, and its samples
    converted from the plain values the native module gives."""

    def desc(self):
        raise NotImplementedError

    def zero(self):
        raise NotImplementedError

    def decode(self, v):
        return v

    def encode(self, v):
        return v


class Primitive(Spec):
    def __init__(self, name, zero):
        self.name = name
        self._zero = zero

    def desc(self):
        return self.name

    def zero(self):
        return self._zero

    def __repr__(self):
        return "rustorb_dds." + self.name


boolean = Primitive("boolean", False)
byte = Primitive("byte", 0)
int8 = Primitive("int8", 0)
uint8 = Primitive("uint8", 0)
int16 = Primitive("int16", 0)
uint16 = Primitive("uint16", 0)
int32 = Primitive("int32", 0)
uint32 = Primitive("uint32", 0)
int64 = Primitive("int64", 0)
uint64 = Primitive("uint64", 0)
float32 = Primitive("float32", 0.0)
float64 = Primitive("float64", 0.0)
float128 = Primitive("float128", 0.0)
char8 = Primitive("char8", "\0")
char16 = Primitive("char16", "\0")


class _String(Spec):
    def __init__(self, wide, bound):
        self.wide = wide
        self.bound = bound

    def desc(self):
        return ("wstring" if self.wide else "string", self.bound)

    def zero(self):
        return ""


def string(bound=0):
    """``string<bound>`` (0: no bound)."""
    return _String(False, bound)


def wstring(bound=0):
    """``wstring<bound>`` (0: no bound)."""
    return _String(True, bound)


def _octets(spec):
    return isinstance(spec, Primitive) and spec.name in ("byte", "uint8")


class _Sequence(Spec):
    def __init__(self, element, bound):
        self.element = element if isinstance(element, Spec) else spec_of(element)
        self.bound = bound

    def desc(self):
        return ("sequence", self.element.desc(), self.bound)

    def zero(self):
        return b"" if _octets(self.element) else []

    def decode(self, v):
        if isinstance(v, bytes):
            return v
        return [self.element.decode(x) for x in v]

    def encode(self, v):
        if isinstance(v, (bytes, bytearray)):
            return bytes(v)
        return [self.element.encode(x) for x in v]


def sequence(element, bound=0):
    """``sequence<element, bound>`` (0: no bound)."""
    return _Sequence(element, bound)


class _Array(Spec):
    def __init__(self, element, dims):
        if not dims:
            raise ValueError("an array has at least one dimension")
        self.element = spec_of(element)
        self.dims = list(dims)

    def desc(self):
        return ("array", self.element.desc(), self.dims)

    def zero(self):
        if _octets(self.element) and len(self.dims) == 1:
            return bytes(self.dims[0])

        def make(dims):
            if len(dims) == 1:
                return [self.element.zero() for _ in range(dims[0])]
            return [make(dims[1:]) for _ in range(dims[0])]

        return make(self.dims)

    def _map(self, f, v, depth):
        if isinstance(v, (bytes, bytearray)):
            return bytes(v)
        if depth == 0:
            return [f(x) for x in v]
        return [self._map(f, x, depth - 1) for x in v]

    def decode(self, v):
        return self._map(self.element.decode, v, len(self.dims) - 1)

    def encode(self, v):
        return self._map(self.element.encode, v, len(self.dims) - 1)


def array(element, *dims):
    """An array ``element[d1][d2]...``."""
    return _Array(element, dims)


class _Map(Spec):
    def __init__(self, key, element, bound):
        self.key = spec_of(key)
        self.element = spec_of(element)
        self.bound = bound

    def desc(self):
        return ("map", self.key.desc(), self.element.desc(), self.bound)

    def zero(self):
        return {}

    def decode(self, v):
        return {self.key.decode(k): self.element.decode(e) for k, e in v.items()}

    def encode(self, v):
        return {self.key.encode(k): self.element.encode(e) for k, e in v.items()}


def map(key, element, bound=0):  # noqa: A001 - the IDL name
    """``map<key, element, bound>`` (0: no bound)."""
    return _Map(key, element, bound)


class Member:
    """The annotations of a struct or union member: see :func:`member`."""

    def __init__(self, type, id=None, key=False, optional=False,
                 must_understand=False, external=False, labels=None,
                 default=False, hash_id=None, name=None):
        self.type = type
        self.id = id
        self.hash_id = hash_id
        self.name = name
        self.key = key
        self.optional = optional
        self.must_understand = must_understand
        self.external = external
        self.labels = labels
        self.default = default


def member(type, id=None, key=False, optional=False, must_understand=False,
           external=False, hash_id=None, name=None):
    """A struct member of ``type`` with its annotations: ``@id``,
    ``@key``, ``@optional``, ``@must_understand``, ``@external``, and
    ``@hashid(hash_id)`` (``""`` for ``@hashid`` of the member's name).
    ``name`` is its name in the type when not that of the attribute
    (``from`` for the attribute ``_from``)."""
    return Member(type, id=id, key=key, optional=optional,
                  must_understand=must_understand, external=external,
                  hash_id=hash_id, name=name)


def key(type):
    """A ``@key`` member of ``type``."""
    return Member(type, key=True)


def optional(type):
    """An ``@optional`` member of ``type``: None when absent."""
    return Member(type, optional=True)


def case(type, *labels, id=None, hash_id=None, name=None):
    """A union member of ``type`` selected by the discriminator values
    ``labels``."""
    return Member(type, id=id, labels=list(labels), hash_id=hash_id, name=name)


def default_case(type, *labels, id=None, hash_id=None, name=None):
    """The ``default`` union member of ``type`` (and of ``labels``)."""
    return Member(type, id=id, labels=list(labels), default=True, hash_id=hash_id,
                  name=name)


_BUILTINS = {
    bool: boolean,
    int: int32,
    float: float64,
    str: _String(False, 0),
    bytes: _Sequence(uint8, 0),
}


def spec_of(t):
    """The :class:`Spec` of a type given as a member annotation."""
    if isinstance(t, Spec):
        return t
    if isinstance(t, Member):
        return spec_of(t.type)
    if isinstance(t, type) and t in _BUILTINS:
        return _BUILTINS[t]
    s = getattr(t, "__dds_spec__", None)
    if s is not None:
        return s
    if isinstance(t, _native.DynamicType):
        return _Dynamic(t)
    raise TypeError("not a DDS type: %r" % (t,))


def type_of(t):
    """The :class:`DynamicType` of a type: a class made with
    :func:`struct` or :func:`union`, or a DynamicType."""
    if isinstance(t, _native.DynamicType):
        return t
    s = spec_of(t)
    dt = getattr(s, "_dynamic_type", None)
    if dt is None:
        dt = _native.DynamicType(s.desc())
        s._dynamic_type = dt
    return dt


class _Alias(Spec):
    def __init__(self, name, t):
        self.name = name
        self.t = spec_of(t)

    def desc(self):
        return ("alias", self.name, self.t.desc())

    def zero(self):
        return self.t.zero()

    def decode(self, v):
        return self.t.decode(v)

    def encode(self, v):
        return self.t.encode(v)

    def __repr__(self):
        return "rustorb_dds.typedef(%r, %r)" % (self.name, self.t)


def typedef(name, t):
    """The alias (IDL ``typedef``) ``name`` of the type ``t``: the same
    values, and a type of its own in the TypeObjects, so that the type
    matches one declared with the typedef in IDL."""
    return _Alias(name, t)


class _Dynamic(Spec):
    """A type known only as a DynamicType: samples are plain values."""

    def __init__(self, t):
        self.t = t

    def desc(self):
        return self.t

    def zero(self):
        return None


def _extensibility(e):
    if e not in ("final", "appendable", "mutable"):
        raise ValueError("extensibility is final, appendable or mutable")
    return e


def hashed_id(name):
    """The member ID of a name (DDS-XTypes 7.3.1.2.1.1): the first 4
    octets of the MD5 of its UTF-8, as a little endian integer, with the
    top 4 bits cleared."""
    import hashlib

    d = hashlib.md5(name.encode("utf-8")).digest()
    return int.from_bytes(d[:4], "little") & 0x0FFFFFFF


def _autoid(a):
    if a not in ("sequential", "hash"):
        raise ValueError("autoid is sequential or hash")
    return a


def _member_ids(members, autoid):
    """(name, id, Member) of the members: their ``@id``, ``@hashid``, else
    by ``autoid``."""
    out = []
    prev = None
    for name, m in members:
        name = m.name or name
        if m.id is not None:
            mid = m.id
        elif m.hash_id is not None:
            mid = hashed_id(m.hash_id or name)
        elif autoid == "hash":
            mid = hashed_id(name)
        else:
            mid = 0 if prev is None else prev + 1
        out.append((mid, m))
        prev = mid
    return out


def _members(cls):
    """The (name, Member) of the annotations of ``cls``, in order."""
    out = []
    for name, t in cls.__dict__.get("__annotations__", {}).items():
        if isinstance(t, str):
            t = typing.get_type_hints(cls)[name]
        out.append((name, t if isinstance(t, Member) else Member(t)))
    return out


class _Struct(Spec):
    def __init__(self, cls, name, extensibility, nested, autoid):
        self.cls = cls
        self.name = name
        self.extensibility = _extensibility(extensibility)
        self.nested = nested
        self.autoid = _autoid(autoid)
        self.base = None
        for b in cls.__mro__[1:]:
            if isinstance(getattr(b, "__dds_spec__", None), _Struct):
                self.base = b.__dds_spec__
                break
        ms = _members(cls)
        self.members = [(n, mid, m, spec_of(m.type))
                        for (n, _), (mid, m) in zip(ms, _member_ids(ms, self.autoid))]

    def all_members(self):
        base = self.base.all_members() if self.base else []
        return base + self.members

    def desc(self):
        return (
            "struct", self.name, self.extensibility, self.nested,
            self.base.desc() if self.base else None,
            [(m.name or n, i, s.desc(), m.key, m.optional,
              m.must_understand and not m.key, m.external, m.hash_id)
             for n, i, m, s in self.members],
            self.autoid == "hash",
        )

    def zero(self):
        return self.cls()

    def decode(self, v):
        kw = {}
        for n, _, m, s in self.all_members():
            x = v.get(m.name or n)
            kw[n] = None if x is None and m.optional else s.decode(x)
        return self.cls(**kw)

    def encode(self, v):
        """The dict the native module takes, from an instance of the class
        or a dict (by attribute or member name)."""
        out = {}
        for n, _, m, s in self.all_members():
            if isinstance(v, dict):
                x = v.get(n, v.get(m.name)) if m.name else v.get(n)
            else:
                x = getattr(v, n, None)
            out[m.name or n] = None if x is None else s.encode(x)
        return out


def struct(name=None, extensibility="final", nested=False, autoid="sequential"):
    """Makes the decorated class a struct type named ``name`` (its
    qualified name by default), ``@final`` by default, with member IDs by
    ``autoid`` (``"sequential"`` or ``"hash"``) where not given. A base
    class that is a struct is its base type."""

    def wrap(cls):
        spec = _Struct(cls, name or cls.__qualname__.replace(".", "::"),
                       extensibility, nested, autoid)
        for n, _, m, s in spec.members:
            if n not in cls.__dict__:
                if m.optional:
                    setattr(cls, n, None)
                else:
                    setattr(cls, n, dataclasses.field(default_factory=s.zero))
        cls = dataclasses.dataclass(cls)
        cls.__dds_spec__ = spec
        spec.cls = cls
        return cls

    return wrap


def _label(x):
    """A union label as an integer: a char is its code."""
    return ord(x) if isinstance(x, str) else int(x)


class _Union(Spec):
    def __init__(self, cls, name, discriminator, extensibility, nested, autoid):
        self.cls = cls
        self.name = name
        self.discriminator = spec_of(discriminator)
        self.extensibility = _extensibility(extensibility)
        self.nested = nested
        self.autoid = _autoid(autoid)
        self.members = []
        ms = _members(cls)
        for (name_, _), (mid, m) in zip(ms, _member_ids(ms, self.autoid)):
            if m.labels is None:
                raise TypeError("union member %s needs case() or default_case()" % name_)
            self.members.append((name_, mid, m, spec_of(m.type)))

    def desc(self):
        return (
            "union", self.name, self.extensibility, self.nested,
            self.discriminator.desc(),
            [(m.name or n, i, s.desc(), [_label(x) for x in m.labels], m.default)
             for n, i, m, s in self.members],
            self.autoid == "hash",
        )

    def _selected(self, d):
        d = _label(d)
        for n, _, m, s in self.members:
            if d in [_label(x) for x in m.labels]:
                return s
        for n, _, m, s in self.members:
            if m.default:
                return s
        return None

    def zero(self):
        n, _, m, s = self.members[0]
        d = m.labels[0] if m.labels else 0
        return self.cls(self.discriminator.decode(d), s.zero())

    def _disc(self, d):
        """The discriminator as the native module gives it (an integer)
        as a value of its type."""
        t = self.discriminator
        while isinstance(t, _Alias):
            t = t.t
        if isinstance(t, Primitive) and t.name in ("char8", "char16"):
            return chr(d)
        if isinstance(t, Primitive) and t.name == "boolean":
            return bool(d)
        return t.decode(d)

    def decode(self, v):
        d, x = v
        s = self._selected(d)
        return self.cls(self._disc(d),
                        None if s is None or x is None else s.decode(x))

    def encode(self, v):
        if isinstance(v, tuple):
            d, x = v
        else:
            d, x = v.discriminator, v.value
        s = self._selected(d)
        return (_label(d), None if s is None or x is None else s.encode(x))


def union(discriminator, name=None, extensibility="final", nested=False,
          autoid="sequential"):
    """Makes the decorated class a union type with a discriminator of
    type ``discriminator``; its annotations are its members, each a
    :func:`case` or :func:`default_case`. Its values have
    ``discriminator`` and ``value``."""

    def wrap(cls):
        spec = _Union(cls, name or cls.__qualname__.replace(".", "::"),
                      discriminator, extensibility, nested, autoid)
        names = [n for n, _, _, _ in spec.members]

        def __init__(self, discriminator, value=None):
            self.discriminator = discriminator
            self.value = value

        def __eq__(self, other):
            return (type(self) is type(other)
                    and self.discriminator == other.discriminator
                    and self.value == other.value)

        def __repr__(self):
            return "%s(%r, %r)" % (cls.__qualname__, self.discriminator, self.value)

        cls.__init__ = __init__
        cls.__eq__ = __eq__
        cls.__hash__ = None
        cls.__repr__ = __repr__
        cls.__dds_spec__ = spec
        cls._members = names
        return cls

    return wrap


class _Enum(Spec):
    def __init__(self, cls, name, bit_bound, extensibility, default_literal, names):
        self.cls = cls
        self.names = names or {}
        self.name = name
        self.bit_bound = bit_bound
        self.extensibility = _extensibility(extensibility)
        self.explicit = cls[default_literal] if default_literal else None
        self.default = self.explicit or list(cls)[0]

    def desc(self):
        return ("enum", self.name, self.bit_bound, self.extensibility,
                [(self.names.get(n, n), int(m.value), m is self.explicit)
                 for n, m in self.cls.__members__.items()])

    def zero(self):
        return self.default

    def decode(self, v):
        try:
            return self.cls(v)
        except ValueError:
            return v

    def encode(self, v):
        return int(v)


def enum(name=None, bit_bound=32, extensibility="final", default_literal=None,
         names=None):
    """Makes the decorated :class:`enum.IntEnum` an enum type; its default
    is its first literal, or the one named ``default_literal``
    (``@default_literal``). ``names`` maps the Python names of literals
    to their names in the type where they differ."""

    def wrap(cls):
        if not issubclass(cls, _enum.IntEnum):
            raise TypeError("an enum type is an enum.IntEnum")
        cls.__dds_spec__ = _Enum(cls, name or cls.__qualname__.replace(".", "::"),
                                 bit_bound, extensibility, default_literal, names)
        return cls

    return wrap


class _Bitmask(Spec):
    def __init__(self, cls, name, bit_bound, extensibility, names):
        self.cls = cls
        self.names = names or {}
        self.name = name
        self.bit_bound = bit_bound
        self.extensibility = _extensibility(extensibility)

    def desc(self):
        flags = []
        # In the order they are declared.
        for n, m in self.cls.__members__.items():
            v = int(m.value)
            if v <= 0 or v & (v - 1):
                raise ValueError("a bitmask flag is one bit: %s" % n)
            flags.append((self.names.get(n, n), v.bit_length() - 1))
        return ("bitmask", self.name, self.bit_bound, self.extensibility, flags)

    def zero(self):
        return self.cls(0)

    def decode(self, v):
        return self.cls(v)

    def encode(self, v):
        return int(v)


def bitmask(name=None, bit_bound=32, extensibility="final", names=None):
    """Makes the decorated :class:`enum.IntFlag` a bitmask type: each
    member is one flag, at the position of its bit."""

    def wrap(cls):
        if not issubclass(cls, _enum.IntFlag):
            raise TypeError("a bitmask type is an enum.IntFlag")
        cls.__dds_spec__ = _Bitmask(cls, name or cls.__qualname__.replace(".", "::"),
                                    bit_bound, extensibility, names)
        return cls

    return wrap
