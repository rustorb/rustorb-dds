# rustorb-dds

A DDS (Data Distribution Service) implementation in Rust, from the
[rustorb](https://github.com/rustorb/rustorb) project ([日本語](README.ja.md)).

**Status: early.** Participants discover each other (SPDP/SEDP) and
exchange samples with reliable or best-effort writers and readers over
UDPv4, with keyed instances, large samples (DATA_FRAG), TRANSIENT_LOCAL
durability, partitions, LIVELINESS and DEADLINE, and WaitSets, conditions
and listeners for the communication statuses, and content filtered topics. Hello world works both ways with Cyclone DDS
and Fast DDS, reliable transient-local and liveliness too, and `std_msgs/msg/String` both ways
with ROS 2's demo talker and listener ([interop](interop/)). The plan is
[rustorb/rustorb#111](https://github.com/rustorb/rustorb/issues/111).

Try the hello world in two terminals:

```sh
cargo run --example hello_world -- sub
cargo run --example hello_world -- pub
```

or from Python (`pip install ./python`, see [python/](python/README.md)):

```sh
python python/examples/hello_world.py sub
python python/examples/hello_world.py pub
```

## Crates

| Crate | Role | License |
|---|---|---|
| `rustorb-dds` | The DCPS API (DDS 1.4) | MIT OR Apache-2.0 |
| `rustorb-rtps` | The wire protocol (DDSI-RTPS 2.5): messages, discovery, writers and readers | MIT OR Apache-2.0 |
| `rustorb-xtypes` | The type system (DDS-XTypes 1.3): TypeObject, assignability, DynamicData | MIT OR Apache-2.0 |
| `rustorb-dds-python` | The native module of the Python package `rustorb_dds` ([python/](python/)), with PyO3; not published to crates.io | MIT OR Apache-2.0 |
| `rustorb-dds-ffi` | The C ABI ([`include/rustorb/dds.h`](crates/rustorb-dds-ffi/include/rustorb/dds.h)) the C++ and Java bindings are built on ([docs/c-abi.md](docs/c-abi.md)); not published to crates.io | MIT OR Apache-2.0 |
| `rustorb-dds-corba` | CORBA TypeCodes and anys (`rustorb-core`) as DDS-XTypes types and DynamicData, and back ([docs/corba.md](docs/corba.md)); the only crate here that depends on the ORB; not published to crates.io | MIT OR Apache-2.0 |

The C++ binding ([cpp/](cpp/), [docs/cpp.md](docs/cpp.md)) is header only: the ISO/IEC C++ PSM for DDS (`<dds/dds.hpp>`) on that C ABI, with types from `rustorb-idl -bc++11 -Wbdds_only` (MIT OR Apache-2.0).

The Java binding ([java/](java/), [docs/java.md](docs/java.md)) is the DDS Java 5 PSM (`org.omg.dds`) on that C ABI through the FFM API (Java 22 or newer), with types from `rustorb-idl -bjava -Wbdds_only` (MIT OR Apache-2.0).

OpenRTM-aist 2.0's data ports, connected with its Fast DDS transport, talk with rustorb-dds writers and readers of the same type and QoS ([docs/openrtm.md](docs/openrtm.md)).

The XCDR1/XCDR2 encodings are in `rustorb-cdr` and the DDS type support
traits in `rustorb-typesupport`, both in the rustorb repository and shared
with the ORB. The IDL generator is
[rustorb-idl](https://github.com/rustorb/rustorb-idl). The DDS crates
depend on `rustorb-cdr` and `rustorb-typesupport` only (`rustorb-dds-corba`
alone adds `rustorb-core`, the ORB); the generator is
GPL-3.0-or-later, and the code it generates is not covered by that license.

## Stages

1. **D0** (in rustorb): XCDR1/XCDR2 in `rustorb-cdr`, DDS type support from `rustorb-idl`
2. **D1**: RTPS messages, SPDP/SEDP, best-effort writers and readers; hello world both ways with Cyclone DDS and Fast DDS
3. **D2**: reliable delivery, DATA_FRAG, keyed instances, History/Durability/Liveliness/Deadline, WaitSet; ROS 2 talker/listener
4. **D3**: TypeObject, assignability, TypeLookup, DynamicData, Python binding
5. **D4**: C++/Java, OpenRTM-aist 2.0, TypeCode ⇄ TypeObject
6. **D5** (in progress, #55): content-filtered topics, DDS-RPC, shared memory, DDS Security

## Clean room

This implementation is written from the OMG specifications (DDS 1.4,
DDSI-RTPS 2.5, DDS-XTypes 1.3, DDS Security, IDL4) and black-box
interoperability tests with other implementations only. Do not read or copy
the source code of other DDS implementations; see
[CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT OR Apache-2.0, at your option ([LICENSE-MIT](LICENSE-MIT),
[LICENSE-APACHE](LICENSE-APACHE)).
