# Contributing

## Clean room

rustorb-dds is a clean-room implementation.

- Work from the OMG specifications: DDS 1.4, DDSI-RTPS 2.5, DDS-XTypes 1.3,
  DDS Security 1.1 and IDL 4.2. Cite the clause when code follows a rule
  that is not obvious.
- Other DDS implementations (Cyclone DDS, Fast DDS, OpenDDS, RTI Connext,
  RustDDS and others) may be run as black boxes for interoperability tests:
  their packets may be captured and compared, and their documented
  behaviour relied on. Their source code must not be read or copied, and
  neither may code derived from it.
- When the specification is unclear and an implementation's behaviour
  settles it, write down what was observed (the peer, its version, the
  capture) next to the code or in the test.

## Checks

Before pushing, run:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```
