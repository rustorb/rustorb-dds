# TypeObject fixtures from Cyclone DDS

What Cyclone DDS's IDL compiler (`idlc`, Ubuntu 24.04 package
`cyclonedds-tools`, version in `idlc-version.txt`) writes for the IDL in this
directory: for each topic type `T`,

- `T.typeinfo`: its TypeInformation, XCDR2 little endian;
- `T.typemap`: the minimal and the complete TypeIdentifier/TypeObject pairs
  of `T` and of the types it uses, then the complete-to-minimal
  TypeIdentifier pairs.

Both are in hex. `update.sh` regenerates them. Cyclone DDS is used as a
black box: only its output is kept, and its sources are not read.

`tests/cyclonedds.rs` checks that rustorb-xtypes reads every fixture and
writes it back octet for octet, that each TypeObject hashes to its
TypeIdentifier, and that describing the same types gives the same
TypeObjects and TypeInformation, both by hand with `rustorb_xtypes::types`
and with the descriptions rustorb-idl writes (`tests/generated/`).

`Generated.idl` is `Fixtures.idl` without struct inheritance, which
rustorb-idl does not support, and without `int8` and `uint8` (see below).
`tests/generated/` is what rustorb-idl writes for it and for HelloWorld.idl
and Keyed.idl:

```sh
for f in Generated HelloWorld Keyed; do
  rustorb-idl -brust -Wbdds_only -Wbxtypes -Wbextensibility=final \
    -C crates/rustorb-xtypes/tests/generated \
    crates/rustorb-xtypes/tests/fixtures/cyclonedds/$f.idl
done
```

What these fixtures show about Cyclone DDS 0.10.4:

- `int8` and `uint8` get the TypeIdentifiers of `char` and `octet`
  (`TK_CHAR8`, `TK_BYTE`), not `TK_INT8` and `TK_UINT8` of DDS-XTypes 1.3.
- Members and elements have `TRY_CONSTRUCT1` set (`@try_construct(DISCARD)`,
  the default); key members also have `IS_MUST_UNDERSTAND`.
- The union discriminator has `IS_MUST_UNDERSTAND`, which Annex B does not
  list among the discriminator's flags.
- Enums and bitmasks have the `IS_FINAL` type flag, and no literal has
  `IS_DEFAULT`.
- `@nested` and `@autoid(HASH)` stay in the minimal TypeObject's flags.
- Struct members are in declaration order in both TypeObjects, also in
  mutable types whose member IDs are not in order.
- `@hashid` is recorded in `ann_builtin.hash_id` (an empty string when it
  has no argument); `@autoid(HASH)` alone records nothing.
- `dependent_typeids` lists every type the topic type uses, in the order
  they are first used.
