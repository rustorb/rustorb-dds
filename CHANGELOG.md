# Changelog

The releases of rustorb-dds. The Python package (`rustorb-dds` on PyPI)
is tagged `python-v<version>`; the Rust crates are not published yet. See
[docs/release.md](docs/release.md).

## [Unreleased]

### Added

- `max_samples` for takes as well as reads (DDS 1.4, 2.2.2.5.3.8, #67):
  `DataReader::max_samples(n)` in Rust gives the first `n` samples to
  arrive and leaves the others as they were; `max_samples` of
  `rdds_reader_select` and `rdds_reader_select_w_condition` in the C ABI
  (a new argument), `Selector::max_samples` with `take` in C++,
  `maxSamples` with `take` in Java and `max_samples` in Python. A read
  with a limit no longer marks the samples beyond it read.
- Listeners of participants, publishers, subscribers and topics (DDS 1.4,
  2.2.4.3, #66): a status that an entity's own listener does not take
  goes to its publisher's or subscriber's listener, then to the
  participant's; new data goes first to `on_data_on_readers`, and
  `Subscriber::notify_datareaders` hands it to the readers' listeners.
  INCONSISTENT_TOPIC goes to the topic's listener, else the
  participant's. In Rust, the C ABI, C++, Java and Python.
- DDS Security 1.2 (feature `security`, off by default; docs/security.md,
  #59): the builtin plugins written from the specification with RustCrypto
  in the new crate `rustorb-dds-security`: authentication (DDS:Auth:PKI-DH,
  EC P-256 and RSA certificates, DH and ECDH), access control
  (DDS:Access:Permissions, signed governance and permissions documents)
  and cryptography (DDS:Crypto:AES-GCM-GMAC), with the secure SEDP and WLP
  endpoints. `ParticipantConfig::security` takes a `SecurityConfig`. It
  works with Cyclone DDS 0.10 and Fast DDS 2.11 with their security on
  (the `-secure` cases of `interop/run.sh`).
- A shared memory transport between participants of rustorb-dds on one
  host (feature `shm`, on by default; Linux, macOS and Windows): a ring
  buffer in a mapped file of `/dev/shm` (else the temporary directory)
  per participant and a loopback UDP port to wake it, announced in SPDP with locators of a vendor kind; UDP with other
  implementations and other hosts. `ParticipantConfig::shared_memory`
  turns it off (docs/shm.md, #58).
- Requests and replies over DDS (DDS-RPC 1.0, the request/reply style):
  `rpc::Requester` and `rpc::Replier`, with the related sample identity
  sent in-line (`PID_RELATED_SAMPLE_IDENTITY`, also in
  `SampleInfo::related_sample_identity` and `DataWriter::write_w_params`),
  the writer and reader of each side announced as a pair in SEDP
  (`PID_RELATED_ENTITY_GUID`, `PID_SERVICE_INSTANCE_NAME`), and the
  enhanced service discovery rules (7.6.2.2). They work with ROS 2
  services (rmw_fastrtps_cpp) both ways: the example `ros2_service` and
  the new cases of `interop/ros2.sh`. In the C ABI (`rdds_requester`,
  `rdds_replier`), C++ (`dds::rpc`), Java (`Requester`, `Replier`) and
  Python too (docs/rpc.md, #57).
- Content filtered topics (DDS 1.4, 2.2.2.3.3): the filter expressions
  of DDS 1.4, Annex B (`ContentFilter` in `rustorb-xtypes`, on
  DynamicData), `DomainParticipant::create_contentfilteredtopic`,
  `ContentFilteredTopic::set_expression_parameters`, and readers of them
  through `TopicDescription`. The readers drop the samples that do not
  pass and announce their filter in SEDP (`PID_CONTENT_FILTER_PROPERTY`,
  DDSI-RTPS 2.5, 8.7.3.1). The type must be registered. In the C ABI
  (`rdds_cftopic`), C++ (`dds::topic::ContentFilteredTopic`,
  `dds::topic::Filter`), Java (`ContentFilteredTopic`) and Python too.
  `interop/run.sh` runs them with Cyclone DDS and Fast DDS (#56).
- Interoperation with OpenRTM-aist 2.0's data ports over its Fast DDS
  transport: the example `openrtm` in Rust, C++ and Python behaves as an
  OutPort or InPort of `RTC::TimedLong`, and `interop/openrtm.sh` (CI job
  `openrtm`, OpenRTM-aist 2.0.2 on Ubuntu 22.04) runs it with OpenRTM's
  ConsoleIn and ConsoleOut in both directions (docs/openrtm.md, #48).
- The crate `rustorb-dds-corba`: CORBA TypeCodes as DDS-XTypes types
  (and so TypeObjects) and back, and anys as DynamicData and back, on
  `rustorb-core` 0.3. Extensibility and keys, which TypeCodes do not
  have, come from options; member IDs are sequential. The TypeObjects are
  those rustorb-idl describes for the same IDL (docs/corba.md, #47).
- The Java binding (`java/`, Java 22 or newer): the DDS Java 5 PSM
  (`org.omg.dds`, a subset) on the C ABI through the FFM API, with no
  JNI: DomainParticipant, Topic, Publisher, Subscriber, DataWriter,
  DataReader with their QoS, statuses and listeners, samples, conditions
  and WaitSets, and the exceptions of the PSM; a Maven project with JUnit
  tests and the examples `HelloWorldExample` and `KeyedExample`.
  rustorb-idl writes the types and a `<Name>Codec` per type
  (`-bjava -Wbdds_only`, or `-Wbdds` beside the CORBA code).
  `interop/run.sh` runs them with Cyclone DDS and Fast DDS (#46).
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

### Fixed

- Tests use domain ids of 100 or less, so that their ports stay below the
  ephemeral port range and do not fail with `AddrInUse` (#64).

## [python-0.1.0] - 2026-11-05

### Added

- The Python package `rustorb-dds` (`import rustorb_dds`), with its native
  module in the `rustorb-dds-python` crate (PyO3, maturin): types declared
  as Python classes or as DynamicTypes, DomainParticipant, Topic,
  Publisher, Subscriber, DataWriter, DataReader, QoS, WaitSet, conditions
  and listeners (#35). rustorb-idl writes the type declarations from IDL
  (`-bpython -Wbdds_only`).
