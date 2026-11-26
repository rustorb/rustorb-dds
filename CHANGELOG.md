# Changelog

The releases of rustorb-dds. From 0.2.0 on, the Rust crates (on
crates.io) and the Python package (`rustorb-dds` on PyPI), and from 0.3.0
on the NuGet package (`Rustorb.Dds`) and the gem (`rustorb-dds`), have one
version and one tag, `v<version>`; the Python package 0.1.0 alone was tagged
`python-v0.1.0`. See [docs/release.md](docs/release.md).

## [Unreleased]

### Changed

- The datagrams received and not yet handled are at most
  `rustorb_rtps::udp::INCOMING_CAPACITY` (1024): when the participant's
  thread falls behind, the receive threads wait and the kernel drops what
  the sockets cannot hold, instead of the queue growing without end
  (#110). `UdpTransport::injector` and `ShmTransport::open` take a
  `SyncSender`.
- User traffic is handled on the thread that received it (UDP or shared
  memory) rather than handed to the participant's thread, when it comes
  alone and nothing received before it is still waiting; a burst still
  goes to the participant's thread. The readers' waiters and listeners
  are told of new data before what is due is sent (an ACKNACK, say).
  The round trip of a small reliable sample went from about 123 µs to
  85 µs in the container (#111).
  `UdpTransport::open_with` takes a `UserHandler`; `injector` gives a
  `Sink`, which `ShmTransport::open` takes; `Incoming::handled` says how
  many user datagrams were handled.
- A writer of the participant asks its reliable readers to acknowledge
  every 16 samples, and 1 ms after the last one, rather than with each
  sample: the HEARTBEAT sent with the others has the final flag set. A blocked write and
  `wait_for_acknowledgments` ask at once. Reliable throughput of 256-octet
  samples went from about 156 to 241 Mbit/s in the container (#111,
  #113). `WriterConfig` has `acknowledgment_batch` (1 by default, as
  before) and `acknowledgment_delay`; `ReliableWriter` has
  `request_acknowledgment`.
- `rustorb-dds-perf --listener` (ping, pong) answers in the reader's
  listener, as Cyclone DDS's `ddsperf` does, and `bench/run.py` has it as
  the latency-only peer `rustorb-listener`. The round trip of 32 octets
  went from 85 µs (WaitSet) to 51 µs in the container (#111).
- Large samples (#112): the HEARTBEAT sent after a change goes in the same
  message as long as the transport carries it, instead of a second
  datagram for a change of about 16 KiB; on Linux, a UDP datagram is
  received straight into a buffer of its size rather than copied out of
  the receive buffer; a received value moves into the reader without a
  copy, and a reader of a keyless topic no longer keeps a copy of the last
  value (so `get_key_value` gives `None` on a keyless topic, as it did for
  loaned samples). The reliable round trip of 16 KiB went from about 160
  to 105 µs, and of 256 KiB from about 600 to 550 µs, in the container.
- Discovery is faster (#106). The participant's first SPDP announcement,
  and its answer to a new participant, go out at once rather than at the
  thread's next wake-up (up to 50 ms); the SEDP writers send a HEARTBEAT
  every 100 ms (was 1 s) while something is not acknowledged; and a
  reliable reader's first ACKNACK to a newly matched writer asks for a
  HEARTBEAT (final flag clear), so what the writer sent before the reader
  knew it is asked for at once. In the container, 10 participants started
  together all matched in about 30 ms instead of about 1 s.
  `SedpConfig::default()` has the 100 ms period; `Spdp::next_deadline`
  is now when an announcement is due.
- A read or take with `max_samples` costs in proportion to `max_samples`,
  not to the samples in the reader (#110).
- The sub of `rustorb-dds-perf` takes 256 samples at a time (as the
  Cyclone DDS program does), and the best-effort sub of all three
  programs keeps at most 1000 samples not yet taken (KEEP_ALL with
  `max_samples`): one slower than its pub loses samples instead of
  keeping them all (#110).

### Added

- Performance measurements (#100): `rustorb-dds-perf` (round-trip latency
  and throughput in the manner of ddsperf, over UDP, shared memory or
  loaned samples), the same program for Cyclone DDS and Fast DDS in
  `bench/`, `bench/run.py` and `bench/report.py`, and the first results
  in docs/benchmark.md (#101 to #105, #109).
- Discovery measurements (#106): the `disc` mode of `rustorb-dds-perf`
  and of the Cyclone DDS and Fast DDS programs, and `bench/discovery.py`,
  which starts N participants at once (alone or mixed with another
  implementation) and gives the time until all matched, CPU time, memory
  and the UDP traffic; the results are in docs/benchmark.md.

### Fixed

- A listener that holds an entity of its own participant (a reader's
  listener that writes back with a writer, say) is dropped with no lock
  held: deleting the reader or writer, or replacing the listener, no
  longer hangs (#117).
- A writer faster than its reader no longer fails with "the shared
  memory buffer is full" once the reader's ring is full: the message is
  lost on the way like a datagram, and reliable readers get it again
  (#101).

### Changed

- The NuGet package `Rustorb.Dds` depends on rustorb's `Rustorb.Types`
  (0.7.1), the `Omg.Types` (`ISequence<T>`, `Sequence<T>`) that the code
  of `rustorb-idl -bcsharp -Wbdds_only` uses for sequences, without CORBA.
  The tests no longer reference `Rustorb` (#98).

## [0.3.0] - 2026-11-24

The first release of the .NET binding (NuGet `Rustorb.Dds`) and the Ruby
binding (gem `rustorb-dds`), and of prebuilt `librustorb_dds_ffi` archives
(D7, #90).

### Added

- Pre-shared key (PSK) protection of RTPS messages (#84, DDS Security
  1.2): `ParticipantConfig::psk` (`PskConfig`) or the properties
  `dds.sec.crypto.rtps_psk_secret_passphrase`, `..._extra`,
  `dds.sec.crypto.rtps_psk_symmetric_cipher_algorithm` and
  `dds.sec.access.rtps_psk_protection_kind`. Alone, they are the PSK
  plugins (DDS:Auth:PSK, DDS:Access:PSK, DDS:Crypto:PSK): every RTPS
  message, discovery included, is signed or encrypted with keys derived
  from the passphrase (10.5.2.1.3). With the builtin plugins, the
  governance's `rtps_psk_protection_kind` protects the messages the
  participant key does not, the bootstrapping ones first. The SRTPS
  messages with the AdditionalAuthenticatedData flag are decoded too.
  See [docs/security.md](docs/security.md).

- The .NET binding (#93): `dotnet/src/Rustorb.Dds` (the NuGet package
  `Rustorb.Dds`, .NET 8), the structure of the DDS Java 5 PSM in C#
  (`Omg.Dds.Domain`, `.Pub`, `.Sub`, `.Topic`, `.Core`, `.Core.Policy`,
  `.Core.Status`) on the C ABI through P/Invoke (`LibraryImport`,
  generated from `dds.h` by `dotnet/tools/gen-pinvoke.py`): participants,
  topics and content filtered topics, writers and readers, keyed
  instances, QoS as immutable records, listeners, conditions and wait
  sets, DDS Security through the Property QoS. Types come from
  `rustorb-idl -bcsharp -Wbdds_only` (rustorb/rustorb-idl#18). xUnit tests,
  the examples `HelloWorld` and `Keyed`, the .NET cases of
  `interop/run.sh`, and the CI job `dotnet` (Linux, macOS, Windows).
  See [docs/dotnet.md](docs/dotnet.md).
- The Ruby binding (#92): the gem `rustorb-dds` (`ruby/`, Ruby 3.0 or
  newer), the structure of the DDS C++ PSM in Ruby (`DDS::Domain`,
  `DDS::Pub`, `DDS::Sub`, `DDS::Topic`, `DDS::Core`, `DDS::Core::Policy`,
  `DDS::Core::Status`) as a C extension that links `librustorb_dds_ffi`
  statically, in the shape of rustorb's `rustorb` gem: participants,
  topics and content filtered topics, writers and readers, keyed
  instances, QoS, listeners, conditions and wait sets, DDS Security
  through the Property QoS. Blocking calls release the GVL; listeners run
  on Ruby threads. Types come from `rustorb-idl -bruby -Wbdds_only`
  (rustorb/rustorb-idl#17). minitest tests, the examples `hello_world.rb`
  and `keyed.rb`, the Ruby cases of `interop/run.sh`, and the CI job
  `ruby` (Linux and macOS). See [docs/ruby.md](docs/ruby.md).
- Prebuilt C ABI (#91): the release attaches
  `rustorb-dds-native-<version>-<platform>`, `librustorb_dds_ffi` shared
  and static with `rustorb/dds.h`, for Linux x86_64 and aarch64 (glibc
  2.28), macOS arm64 and x86_64 and Windows x64, for the NuGet package
  and the gem (docs/c-abi.md).
- Release of the .NET and Ruby bindings (#94): the release workflow builds
  the NuGet package `Rustorb.Dds` (with `librustorb_dds_ffi` of each
  platform in `runtimes/<rid>/native`) and the gem `rustorb-dds` (with
  `librustorb_dds_ffi.a` of each platform in `vendor/`), runs a program
  with each, attaches them to the GitHub Release and publishes them to
  nuget.org and RubyGems (environments `nuget` and `rubygems`, see
  docs/release.md). `tools/set-version.py` sets their versions too.

## [0.2.0] - 2026-11-16

The first release of the Rust crates on crates.io: `rustorb-dds`,
`rustorb-rtps`, `rustorb-xtypes`, `rustorb-dds-security`,
`rustorb-dds-corba` and `rustorb-dds-ffi` (#75).

### Added

- Loaned samples (zero-copy) through shared memory (#74), a rustorb API
  in the style of Cyclone DDS's loans, for types of a fixed size that
  implement the new unsafe trait `PlainType` (`#[repr(C)]` plain data
  whose layout is its serialized form; the primitives and arrays of them
  do). `DataWriter::loan_sample` lends a sample in the writer's pool in
  shared memory (`WriteLoan`), `DataWriter::write_loaned` publishes it,
  and `DataReader::take_loaned`, `read_loaned` and `take_loaned_max`
  return `ReadLoan`s, which read the sample in place. A slot is used
  again once the writer's history no longer has it and every reader has
  dropped it. The sample goes by reference (a 20-octet payload naming its
  slot) only to readers of this participant or reached by shared memory,
  from a VOLATILE writer without DDS Security; otherwise it is copied (and,
  without a pool, serialized). The writer checks the layout on the first
  loaned sample. For a 1 MiB sample, write and take between two
  participants take about 0.07 ms against 12 ms. In rustorb-rtps:
  `shm::Pool`, `PoolSlot`, `PoolLoan`, `Pools` and
  `ShmTransport::create_pool`, `pools` and `reaches`.

- DDS Security: OCSP (RFC 6960; DDS Security 1.2, 10.3.2.5 and 10.3.3,
  #73). `SecurityConfig::ocsp` (`OcspOptions`), or the properties
  `rustorb.dds.sec.auth.ocsp` (a responder URL, or `aia`),
  `.ocsp_timeout` and `.ocsp_refresh`. The participant must be good to
  start, staples its response to its handshake messages (`ocsp_status`),
  and checks its peers: by their stapled response, or else by asking the
  responder. The statuses are asked again every refresh period, and a peer
  found revoked is revoked as `revoke_participant` does. Stapled responses
  are taken even with OCSP off. In rustorb-dds-security: the `ocsp`
  module, `IdentityOptions::ocsp`, `Identity::verify_peer_with`, and a new
  `ocsp` parameter of `Identity::from_uris_with`.

- DDS Security: identity revocation and key revision (DDS Security 1.2,
  9.3.2.12.2, 9.5.1.6 and 9.8.9.5, #73).
  `DomainParticipant::revoke_participant` ends the communication with an
  authenticated participant and refuses its certificate from then on;
  with `<enable_key_revision>` in the governance it also revises the keys.
  `DomainParticipant::revise_keys` revises the key material of the
  participant and its endpoints (not the volatile ones), sends it to the
  matched participants and uses it once they acknowledge it or after 2 s.
  In rustorb-dds-security, `KeyMaterial::revision` (the first three octets
  of the transformation kind), `KeyMaterial::revised`,
  `Encoder::set_key_material` and `Decoder::add_revision` (the last 8
  revisions are kept); `Encoder::key_material` and `Decoder::key_material`
  now return the key material by value. `DomainRule::enable_key_revision`
  is read from the governance.

- DDS Security: data tagging, DDS:Tagging:DDS_Discovery (DDS Security
  1.2, 9.7, #73). `DataWriterQos::data_tags` and `DataReaderQos::data_tags`
  are announced as `PID_DATA_TAGS` (`EndpointData::data_tags` in
  rustorb-rtps). The `<data_tags>` of the permissions are checked when a
  local endpoint is created and when a remote one is matched
  (`Grant::allows_tagged` in rustorb-dds-security).

- DDS Security: the builtin logging plugin, DDS:Logging:DDS_LogTopic (DDS
  Security 1.2, 9.6 and 10.6, #73). `SecurityConfig::logging`
  (`security_log::LogOptions`), or the properties
  `dds.sec.log.builtin.DDS_LogTopic.logging_level`, `.log_file` and
  `.distribute`. It logs authentication and access control events to a
  file and/or publishes them on `DDS:Security:LogTopicV2`
  (`BuiltinLoggingTypeV2`). That topic gets the builtin rule of 10.6: anyone
  may write, reading needs a grant. See [docs/security.md](docs/security.md).

- DDS Security: encrypted private keys and CRLs (#73). `SecurityConfig`
  has two new fields, `password` and `crl`.
  - `dds.sec.auth.password` (`SecurityConfig::password`) decrypts the
    private key: PKCS #8 `ENCRYPTED PRIVATE KEY`, or PEM with `DEK-Info`
    (AES-CBC). The password can also come from `?password=` in the
    `file:` URI of the key.
  - `rustorb.dds.sec.auth.crl` (`SecurityConfig::crl`) gives CRLs of the
    identity CA. A revoked certificate fails authentication, this
    participant's or a peer's.

  See [docs/security.md](docs/security.md).

- The rest of DDS-RPC 1.0 (#72):
  - The function-call style: rustorb-idl (`-brust -Wbdds_only -Wbxtypes`)
    writes a trait, `<I>Client` and `<I>Server<S>` for each `@DDSService`
    interface, over the Enhanced Service Mapping, with `RpcError` and
    `GENERATED_CODE_VERSION_1` of `rustorb_dds::rpc`.
  - The Basic Service Mapping: `RequestHeader`, `ReplyHeader` and
    `RemoteExceptionCode` (`rustorb_dds::rpc::idl`, from `DdsRpc.idl`),
    the traits `BasicRequest` and `BasicReply`, and `Requester::basic` and
    `Replier::basic`, which correlate by the headers and honour the
    instance name. The requester reads the replies through a content
    filtered topic on the header.
  - Topic aliases (`PID_TOPIC_ALIASES`): announced from
    `RpcConfig::request_topic_aliases` and `reply_topic_aliases`, read, and
    used in matching, so that a client of a base interface finds a service
    of a derived one.
  - A replier sends a reply only to the reply reader of the requester that
    asked. The others get a GAP (`WriteParams::directed_to`,
    `ReliableWriter::direct` in rustorb-rtps).

  See [docs/rpc.md](docs/rpc.md).

- TRANSIENT and PERSISTENT durability, with DURABILITY_SERVICE (DDS 1.4,
  2.2.3.4 and 2.2.3.5, #71): a deleted TRANSIENT or PERSISTENT writer
  stays with its participant, which keeps what its DURABILITY_SERVICE
  allows and sends it to late readers; a PERSISTENT writer also keeps its
  changes in a file, which a PERSISTENT writer of the same topic and type
  created later takes up, also after a restart. `DurabilityService` and
  `ParticipantConfig::persistence_directory` (or the property
  `rustorb.dds.persistence.directory`, or `RUSTORB_DDS_PERSISTENCE_DIR`);
  `service_*` in `rdds_writer_qos`, `Qos` in Python,
  `dds::core::policy::DurabilityService` in C++ and `DurabilityService`
  in Java. See [docs/durability.md](docs/durability.md).

- Recursive types (DDS-XTypes 1.3, 7.3.4.8 and 7.3.4.9, #70): a type
  that uses itself through a sequence, a map or an optional member, made
  with `Type::forward` and `Type::define` in Rust, `rdds_type_forward`
  and `rdds_type_define` in the C ABI, and the name of the type as a
  string in Python. Their TypeObjects are identified by their strongly
  connected component (SCC identifiers), which TypeLookup serves and
  resolves as a whole, so a reader learns such a type from the network.
- Bitset types (#70): `BitsetType` with its TypeObjects, DynamicData of
  its fields and its serialization as the smallest unsigned integer that
  holds its bits; `rdds_bitset_builder_*` in the C ABI (one integer in
  the sample streams) and `@dds.bitset` with `dds.bitfield` in Python.
- Annotation types (#70): `AnnotationType` with its TypeObjects both ways
  and its DynamicType (members with their defaults); its DynamicData is
  not serialized.

- DDS Security from the PROPERTY of the participant QoS (DDS Security
  1.2, 7.2.5, #69): the six `dds.sec.*` properties in
  `ParticipantConfig::set_properties` in Rust, `property` of
  `rdds_participant_qos` in the C ABI (built with `security` by
  default), `dds::core::policy::Property` in C++, `Property` in Java and
  `properties` of `DomainParticipant` in Python. The `hello_world`
  examples of C++ and Python read `RUSTORB_DDS_SECURITY` as the Rust one
  does, with new `-secure` interop cases.
- The secure SPDP endpoints (`DCPSParticipantsSecure`, DDS Security 1.2,
  7.5.1.6, #69): with discovery protection, authenticated participants
  exchange their data and the deletion of a participant there, and
  changes and disposes of them on plain SPDP are ignored.
- Interoperability tests with OpenDDS 3.31 (`interop/run.sh opendds`, in
  Docker, and the CI job `opendds`, #68): best effort, reliable,
  transient-local, large samples, keyed instances, XCDR2, type evolution
  and TypeLookup, from Rust, Python, C++ and Java.
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

### Changed

- The minimum supported Rust version is 1.89, for `File::try_lock` (#71).
- rustorb-cdr and rustorb-typesupport 0.6.0 (from 0.2.0), and
  rustorb-core 0.6.0 for rustorb-dds-corba (#65). rustorb-cdr writes the
  XCDR2 encapsulation identifiers of DDS-XTypes Table 60 (`CDR2_LE` is
  `0x0007`) itself since 0.5.0, so rustorb-dds no longer rewrites them;
  it still reads those of Table 39.
- One version for every package, set with `tools/set-version.py`, and
  one tag `v<version>` that publishes the crates and the Python package
  (`.github/workflows/release.yml`, #75).

### Fixed

- Tests use domain ids of 100 or less, so that their ports stay below the
  ephemeral port range and do not fail with `AddrInUse` (#64).
- Readers take samples whose encapsulation counts padding octets that
  the payload does not have (OpenDDS 3.31 leaves them out of a DATA that
  ends a message); they were dropped (#68).
- A dispose or an unregister carries the key payload as well as the key
  hash when an OpenDDS reader is matched, as OpenDDS 3.31 needs it (#68).

## [python-0.1.0] - 2026-11-05

### Added

- The Python package `rustorb-dds` (`import rustorb_dds`), with its native
  module in the `rustorb-dds-python` crate (PyO3, maturin): types declared
  as Python classes or as DynamicTypes, DomainParticipant, Topic,
  Publisher, Subscriber, DataWriter, DataReader, QoS, WaitSet, conditions
  and listeners (#35). rustorb-idl writes the type declarations from IDL
  (`-bpython -Wbdds_only`).
