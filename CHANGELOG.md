# Changelog

The releases of rustorb-dds. The Python package (`rustorb-dds` on PyPI)
is tagged `python-v<version>`; the Rust crates are not published yet. See
[docs/release.md](docs/release.md).

## [Unreleased]

### Added

- The C++ binding (`cpp/`, header only, C++17): the ISO/IEC C++ PSM for
  DDS (`<dds/dds.hpp>`) on the C ABI: DomainParticipant, Topic,
  Publisher, Subscriber, DataWriter, DataReader with their QoS, statuses
  and listeners, samples, conditions and WaitSets, and the exceptions of
  the PSM; a CMake project with tests, the examples `hello_world` and
  `keyed`, and a CMake package (`find_package(rustorb_dds)`). rustorb-idl
  writes the types (`-bc++11 -Wbdds_only`, or `-Wbdds` beside the CORBA
  code). `interop/run.sh` runs them with Cyclone DDS and Fast DDS (#45).
- The C ABI `rustorb-dds-ffi` (`include/rustorb/dds.h`, `rdds_` prefix),
  which the C++ and Java bindings are built on: types built member by
  member as DDS-XTypes `DynamicTypeBuilder` does, samples written and read
  as streams of values in the order of their type (the library encodes
  them), the DCPS entities with their QoS, statuses and listeners,
  conditions and WaitSets (#44).

## [python-0.1.0] - 2026-11-05

### Added

- The Python package `rustorb-dds` (`import rustorb_dds`), with its native
  module in the `rustorb-dds-python` crate (PyO3, maturin): types declared
  as Python classes or as DynamicTypes, DomainParticipant, Topic,
  Publisher, Subscriber, DataWriter, DataReader, QoS, WaitSet, conditions
  and listeners (#35). rustorb-idl writes the type declarations from IDL
  (`-bpython -Wbdds_only`).
