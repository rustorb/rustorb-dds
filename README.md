# rustorb-dds

A DDS (Data Distribution Service) implementation in Rust, from the
[rustorb](https://github.com/rustorb/rustorb) project ([日本語](README.ja.md)).

**Status: just started. Nothing works yet.** The plan is
[rustorb/rustorb#111](https://github.com/rustorb/rustorb/issues/111).

## Crates

| Crate | Role | License |
|---|---|---|
| `rustorb-dds` | The DCPS API (DDS 1.4) | MIT OR Apache-2.0 |
| `rustorb-rtps` | The wire protocol (DDSI-RTPS 2.5): messages, discovery, writers and readers | MIT OR Apache-2.0 |
| `rustorb-xtypes` | The type system (DDS-XTypes 1.3): TypeObject, assignability, DynamicData | MIT OR Apache-2.0 |

The XCDR1/XCDR2 encodings are in `rustorb-cdr` and the IDL generator is
`rustorb-idl`, both in the rustorb repository and shared with the ORB.
The DDS crates depend on `rustorb-cdr` only; the generator is
GPL-3.0-or-later, and the code it generates is not covered by that license.

## Stages

1. **D0** (in rustorb): XCDR1/XCDR2 in `rustorb-cdr`, DDS type support from `rustorb-idl`
2. **D1**: RTPS messages, SPDP/SEDP, best-effort writers and readers; hello world both ways with Cyclone DDS and Fast DDS
3. **D2**: reliable delivery, DATA_FRAG, keyed instances, History/Durability/Liveliness/Deadline, WaitSet; ROS 2 talker/listener
4. **D3**: TypeObject, assignability, TypeLookup, DynamicData, Python binding
5. **D4**: C++/Java, OpenRTM-aist 2.0, TypeCode ⇄ TypeObject
6. **Later**: DDS Security, shared memory, content-filtered topics, DDS-RPC

## Clean room

This implementation is written from the OMG specifications (DDS 1.4,
DDSI-RTPS 2.5, DDS-XTypes 1.3, DDS Security, IDL4) and black-box
interoperability tests with other implementations only. Do not read or copy
the source code of other DDS implementations; see
[CONTRIBUTING.md](CONTRIBUTING.md).

## License

MIT OR Apache-2.0, at your option ([LICENSE-MIT](LICENSE-MIT),
[LICENSE-APACHE](LICENSE-APACHE)).
