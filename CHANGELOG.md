# Changelog

The releases of rustorb-dds. The Python package (`rustorb-dds` on PyPI)
is tagged `python-v<version>`; the Rust crates are not published yet. See
[docs/release.md](docs/release.md).

## [Unreleased]

## [python-0.1.0] - 2026-11-05

### Added

- The Python package `rustorb-dds` (`import rustorb_dds`), with its native
  module in the `rustorb-dds-python` crate (PyO3, maturin): types declared
  as Python classes or as DynamicTypes, DomainParticipant, Topic,
  Publisher, Subscriber, DataWriter, DataReader, QoS, WaitSet, conditions
  and listeners (#35). rustorb-idl writes the type declarations from IDL
  (`-bpython -Wbdds_only`).
