# rustorb-dds (Python)

The Python binding of [rustorb-dds](https://github.com/rustorb/rustorb-dds), a DDS (OMG Data Distribution Service) implementation with its core in Rust. It interoperates over RTPS 2.5 with other DDS implementations, such as Cyclone DDS, Fast DDS and ROS 2.

```python
import rustorb_dds as dds

@dds.struct("HelloWorld::Greeting")
class Greeting:
    index: dds.uint32
    message: str

participant = dds.DomainParticipant(0)
topic = participant.create_topic("HelloWorldTopic", Greeting)

reader = participant.create_subscriber().create_datareader(topic, dds.Qos(reliability="reliable"))
writer = participant.create_publisher().create_datawriter(topic)
writer.wait_for_subscriptions(1, timeout=5.0)
writer.write(Greeting(1, "hello"))

if reader.wait_for_data(5.0):
    for s in reader.take():
        print(s.data, s.info.instance_state)
```

## Types

A topic's type is either:

- a class declared with `@dds.struct` / `@dds.union` (and `@dds.enum` on an `enum.IntEnum`, `@dds.bitmask` on an `enum.IntFlag` for its members), whose annotations are its members:

  ```python
  @dds.struct("Keyed::Shape", extensibility="appendable")
  class Shape:
      id: dds.key(dds.uint32)
      color: dds.string(32)
      points: dds.sequence(dds.int32, 10)
      note: dds.optional(str)
  ```

  Member types are `bool`, `int` (int32), `float` (float64), `str`, `bytes`, the primitives `dds.int8` ... `dds.float128`, `dds.char8`, `dds.string(bound)`, `dds.wstring(bound)`, `dds.sequence(t, bound)`, `dds.array(t, *dims)`, `dds.map(k, v, bound)`, or another declared type. `dds.key`, `dds.optional` and `dds.member(t, id=, key=, optional=, must_understand=, external=, hash_id=)` give annotations; `dds.typedef(name, t)` is an IDL typedef. Member IDs are sequential from 0 unless given (or hashed with `autoid="hash"`).

  [rustorb-idl](https://github.com/rustorb/rustorb-idl) writes these declarations from IDL: `rustorb-idl -bpython -Wbdds_only Foo.idl` writes `Foo_dds.py` (`from Foo_dds import MyModule`). Their TypeObjects are those other implementations make from the same IDL (checked against Cyclone DDS's idlc in `tests/test_generated.py`).

- or a `dds.DynamicType` (XTypes DynamicData), whose samples are plain values: dicts for structs, `(discriminator, value)` tuples for unions, lists, bytes, ints. `participant.discover_topic_type(name)` learns the type of a topic from the remote endpoints (with TypeLookup when they announce only its hash), so a program can read a topic without knowing its type beforehand.

The type of a declared class is described to the network as an XTypes TypeObject, so it matches the same type declared in IDL in other implementations.

## Entities

`DomainParticipant(domain_id, localhost=False)`, `create_topic`, `create_contentfilteredtopic(name, topic, expression, parameters)` (readers of it get the samples that pass the filter; `set_expression_parameters` changes the parameters), `create_publisher(partition=[...])`, `create_subscriber`, `create_datawriter(topic, qos, listener, mask)`, `create_datareader(...)`.

`DataWriter`: `write`, `register_instance`, `unregister_instance`, `dispose`, `lookup_instance`, `wait_for_acknowledgments`, `wait_for_subscriptions`, statuses, `status_condition`, `set_listener`.

`DataReader`: `take` / `read` (by state, or of an instance), `take_w_condition` / `read_w_condition`, `lookup_instance`, `get_key_value`, `wait_for_data`, `wait_for_publications`, statuses, `status_condition`, `create_readcondition`, `set_listener`.

`Qos` (reliability, history, durability, deadline, liveliness, data_representation, user_data, resource limits), `WaitSet`, `GuardCondition`, `StatusCondition`, `ReadCondition`, `StatusMask`, `DataWriterListener`, `DataReaderListener`. Listeners are called on the participant's threads.

## Building

The native module is the `rustorb-dds-python` crate, built with [maturin](https://www.maturin.rs/):

```sh
cd python
maturin develop     # into the current virtualenv
pytest tests
```

## License

MIT OR Apache-2.0.
