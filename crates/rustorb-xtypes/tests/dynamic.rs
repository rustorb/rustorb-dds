//! DynamicData against the code rustorb-idl generates: a value written by
//! one is read by the other, in XCDR1 and XCDR2 and both byte orders, to
//! the same octets, with the same key hash.

use rustorb_cdr::{ByteOrder, Encoding};
use rustorb_typesupport::TopicType;

use rustorb_xtypes::dynamic::*;
use rustorb_xtypes::registry::TypeRegistry;
use rustorb_xtypes::types::*;

/// See tests/cyclonedds.rs.
#[allow(missing_docs, dead_code, clippy::all)]
mod generated {
    pub mod all {
        include!("generated/generated.rs");
    }
    pub mod hello_world {
        include!("generated/hello_world.rs");
    }
    pub mod keyed {
        include!("generated/keyed.rs");
    }
}

use generated::all::r#gen as g;

const FORMATS: [(Encoding, ByteOrder); 4] = [
    (Encoding::Xcdr1, ByteOrder::Little),
    (Encoding::Xcdr1, ByteOrder::Big),
    (Encoding::Xcdr2, ByteOrder::Little),
    (Encoding::Xcdr2, ByteOrder::Big),
];

/// The type of `T`, as described by the generated code and as made back
/// from its COMPLETE TypeObjects.
fn types_of<T: DescribeType>() -> [DynamicType; 2] {
    let ty = T::describe_type();
    let objects = TypeObjects::new(&ty).unwrap();
    let mut registry = TypeRegistry::new();
    registry.add_type(&objects);
    let back =
        DynamicTypeBuilderFactory::create_type_w_type_identifier(objects.complete_id(), &registry)
            .unwrap();
    [DynamicType::from(ty), back]
}

/// Checks that `v` reads and writes back the same with DynamicData, and
/// returns what was read (from XCDR2 little endian).
fn round_trip<T: TopicType + DescribeType + PartialEq + std::fmt::Debug>(v: &T) -> DynamicData {
    let mut last = None;
    for ty in types_of::<T>() {
        for (encoding, order) in FORMATS {
            let payload = v.to_payload(encoding, order).unwrap();
            let data = DynamicData::from_payload(&ty, &payload)
                .unwrap_or_else(|e| panic!("{} {encoding:?} {order:?}: {e}", T::TYPE_NAME));
            assert_eq!(
                data.to_payload(encoding, order).unwrap(),
                payload,
                "{} {encoding:?} {order:?}",
                T::TYPE_NAME
            );
            assert_eq!(&T::from_payload(&payload).unwrap(), v);
            assert_eq!(data.key_holder().unwrap(), v.key_holder().unwrap());
            assert_eq!(data.key_hash().unwrap(), v.key_hash().unwrap());
            last = Some(data);
        }
    }
    last.unwrap()
}

fn inner(id: i32) -> g::Inner {
    g::Inner {
        id,
        big: format!("inner {id}"),
    }
}

fn all() -> g::All {
    let mut big_arr = [0; 300];
    for (i, x) in big_arr.iter_mut().enumerate() {
        *x = i as i32 * 3 - 100;
    }
    g::All {
        color: g::Color::Blue,
        perm: g::Perm(g::Perm::READ.bits() | g::Perm::EXEC.bits()),
        ls: vec![1, -2, 3],
        name: "name".into(),
        m: [[1, 2, 3], [4, 5, 6]],
        inners: vec![inner(1), inner(2)],
        ss: vec![vec![], vec![7], vec![8, 9]],
        arr: [inner(10), inner(11), inner(12), inner(13)],
        big_arr,
        strs: vec!["a".into(), "".into(), "ccc".into()],
        u: g::U::I(9, inner(20)),
        ue: g::Ue::N("blue".into()),
        r#mut: g::Mut {
            a: 5,
            b: Some("bee".into()),
            k: -7,
            inner: inner(30),
        },
        h: g::Hashed {
            alpha: 1,
            beta: "beta".into(),
            gamma: 3,
        },
        p: g::Plain {
            b: true,
            o: 0xfe,
            s: -300,
            us: 60000,
            l: -70000,
            ul: 4_000_000_000,
            ll: -(1 << 40),
            ull: 1 << 63,
            f: 1.5,
            d: -2.25,
            c: 'é',
        },
        ext: Box::new(inner(40)),
        opt: Some(77),
    }
}

#[test]
fn reads_and_writes_what_generated_code_does() {
    round_trip(&g::Plain::default());
    round_trip(&inner(1));
    round_trip(&g::Mut::default());
    round_trip(&g::Mut {
        a: 1,
        b: Some("b".into()),
        k: 2,
        inner: inner(3),
    });
    for u in [
        g::U::X(1, 5),
        g::U::X(2, 6),
        g::U::S("s".into()),
        g::U::I(-4, inner(4)),
    ] {
        round_trip(&u);
    }
    for ue in [
        g::Ue::R(0.5),
        g::Ue::N("n".into()),
        g::Ue::Default(g::Color::Green),
    ] {
        round_trip(&ue);
    }
    round_trip(&g::Hashed {
        alpha: 1,
        beta: "b".into(),
        gamma: 2,
    });
    let mut a = all();
    round_trip(&a);
    a.r#mut.b = None;
    a.opt = None;
    a.u = g::U::S("".into());
    a.ue = g::Ue::Default(g::Color::Green);
    round_trip(&a);
    round_trip(&generated::keyed::keyed::Shape {
        id: 1,
        index: 2,
        message: "hello".into(),
    });
    round_trip(&generated::hello_world::hello_world::Greeting {
        index: 1,
        message: "hello".into(),
    });
    let top = g::Top { id: 42, all: all() };
    let data = round_trip(&top);

    // And the values are where they should be.
    let id = |d: &DynamicData, name: &str| d.get_member_id_by_name(name);
    assert_eq!(data.get_int32_value(id(&data, "id")).unwrap(), 42);
    let all = data.get_value(id(&data, "all")).unwrap();
    assert_eq!(all.get_int32_value(id(all, "color")).unwrap(), 7);
    let perm = all.get_value(id(all, "perm")).unwrap();
    assert!(perm.get_boolean_value(5).unwrap());
    assert!(!perm.get_boolean_value(1).unwrap());
    let m = all.get_value(id(all, "m")).unwrap();
    assert_eq!(m.get_item_count(), 6);
    assert_eq!(m.get_int32_value(4).unwrap(), 5);
    let mutable = all.get_value(id(all, "mut")).unwrap();
    assert_eq!(mutable.get_string_value(id(mutable, "b")).unwrap(), "bee");
    let u = all.get_value(id(all, "u")).unwrap();
    assert_eq!(u.get_discriminator_value().unwrap(), 9);
    let p = all.get_value(id(all, "p")).unwrap();
    assert_eq!(p.get_char8_value(id(p, "c")).unwrap(), 'é');
    assert_eq!(p.get_uint64_value(id(p, "ull")).unwrap(), 1 << 63);
    assert_eq!(all.get_int32_value(id(all, "opt")).unwrap(), 77);
}

/// Values made with the setters are read by the generated code.
#[test]
fn values_made_dynamically() {
    let [ty, _] = types_of::<g::Mut>();
    let mut data = DynamicDataFactory::create_data(&ty);
    let id = |name: &str| data.get_member_id_by_name(name);
    let (a, b, k, inner_id) = (id("a"), id("b"), id("k"), id("inner"));
    data.set_int32_value(a, 10).unwrap();
    data.set_string_value(b, "dynamic").unwrap();
    data.set_int16_value(k, 11).unwrap();
    let inner_data = data.loan_value(inner_id).unwrap();
    let big = inner_data.get_member_id_by_name("big");
    inner_data.set_string_value(big, "inside").unwrap();
    for (encoding, order) in FORMATS {
        let payload = data.to_payload(encoding, order).unwrap();
        let v = g::Mut::from_payload(&payload).unwrap();
        assert_eq!(
            v,
            g::Mut {
                a: 10,
                b: Some("dynamic".into()),
                k: 11,
                inner: g::Inner {
                    id: 0,
                    big: "inside".into(),
                },
            }
        );
    }
    // A key member of a mutable type that is not there.
    let payload = g::Mut::default()
        .to_payload(Encoding::Xcdr2, ByteOrder::Little)
        .unwrap();
    let mut without_k = payload.clone();
    // DHEADER, then members a (id 5, 8 octets), k (id 9)...: drop k by
    // renaming it to an unknown ID without the must-understand flag.
    let at = 4 + 4 + 8;
    without_k[at..at + 4].copy_from_slice(&(0x2000_0000u32 | 99).to_le_bytes());
    assert_eq!(
        DynamicData::from_payload(&ty, &without_k),
        Err(rustorb_xtypes::Error::Invalid("a key member is missing"))
    );
}

/// Version 1 and version 2 of an appendable and a mutable type: each
/// reads what the other writes (7.2.4.4.8: added members take their
/// default value, unknown ones are skipped).
#[test]
fn evolution() {
    for extensibility in [Extensibility::Appendable, Extensibility::Mutable] {
        let make = |extra: bool| {
            let mut members = vec![
                StructMember::new("number", 0, Type::Primitive(Primitive::Int32)),
                StructMember::new(
                    "text",
                    1,
                    Type::String {
                        wide: false,
                        bound: 0,
                    },
                ),
            ];
            if extra {
                members.push(StructMember::new(
                    "extra",
                    2,
                    Type::Primitive(Primitive::Int32),
                ));
            }
            DynamicType::from(Type::from(StructType {
                name: "evolve::T".into(),
                extensibility,
                nested: false,
                autoid_hash: false,
                base: None,
                members,
            }))
        };
        let (v1, v2) = (make(false), make(true));
        let mut new = DynamicDataFactory::create_data(&v2);
        new.set_int32_value(0, 3).unwrap();
        new.set_string_value(1, "three").unwrap();
        new.set_int32_value(2, 103).unwrap();
        let payload = new.to_payload(Encoding::Xcdr2, ByteOrder::Little).unwrap();
        let old = DynamicData::from_payload(&v1, &payload).unwrap();
        assert_eq!(old.get_int32_value(0).unwrap(), 3);
        assert_eq!(old.get_string_value(1).unwrap(), "three");
        let payload = old.to_payload(Encoding::Xcdr2, ByteOrder::Big).unwrap();
        let back = DynamicData::from_payload(&v2, &payload).unwrap();
        assert_eq!(back.get_string_value(1).unwrap(), "three");
        assert_eq!(back.get_int32_value(2).unwrap(), 0);
    }
}

/// A map, a mutable union, a derived struct and the other things the
/// generated code does not have, through both encodings.
#[test]
fn types_only_dynamic_data_has() {
    let color = Type::from(EnumType {
        name: "M::Small".into(),
        bit_bound: 8,
        extensibility: Extensibility::Final,
        literals: vec![
            EnumLiteral {
                name: "A".into(),
                value: 1,
                default: false,
            },
            EnumLiteral {
                name: "B".into(),
                value: 2,
                default: false,
            },
        ],
    });
    let union = Type::from(UnionType {
        name: "M::MU".into(),
        extensibility: Extensibility::Mutable,
        nested: false,
        autoid_hash: false,
        discriminator: color.clone(),
        discriminator_key: false,
        discriminator_try_construct: TryConstruct::Discard,
        members: vec![
            UnionMember::new("a", 1, Type::Primitive(Primitive::Float64), vec![1]),
            UnionMember::new(
                "b",
                2,
                Type::String {
                    wide: true,
                    bound: 0,
                },
                vec![2],
            ),
        ],
    });
    let map = Type::Map(std::sync::Arc::new(MapType {
        key: Element::new(Type::String {
            wide: false,
            bound: 8,
        }),
        element: Element::new(Type::Primitive(Primitive::Int64)),
        bound: 0,
    }));
    let base = Type::from(StructType {
        name: "M::Base".into(),
        extensibility: Extensibility::Appendable,
        nested: false,
        autoid_hash: false,
        base: None,
        members: vec![StructMember {
            key: true,
            ..StructMember::new("id", 0, Type::Primitive(Primitive::Uint16))
        }],
    });
    let derived = DynamicType::from(Type::from(StructType {
        name: "M::Derived".into(),
        extensibility: Extensibility::Appendable,
        nested: false,
        autoid_hash: false,
        base: Some(base),
        members: vec![
            StructMember::new("u", 1, union),
            StructMember::new("map", 2, map),
            StructMember::new("small", 3, color),
            StructMember {
                optional: true,
                ..StructMember::new("w", 4, Type::Primitive(Primitive::Char16))
            },
            StructMember::new("ld", 5, Type::Primitive(Primitive::Float128)),
        ],
    }));
    let mut data = DynamicDataFactory::create_data(&derived);
    data.set_uint16_value(0, 513).unwrap();
    data.loan_value(1)
        .unwrap()
        .set_wstring_value(2, "wide ü")
        .unwrap();
    let key = |s: &str| {
        let mut k = DynamicDataFactory::create_data(&DynamicType::from(Type::String {
            wide: false,
            bound: 8,
        }));
        k.set_string_value(MEMBER_ID_INVALID, s).unwrap();
        k
    };
    let element = |x: i64| {
        let mut e =
            DynamicDataFactory::create_data(&DynamicType::from(Type::Primitive(Primitive::Int64)));
        e.set_int64_value(MEMBER_ID_INVALID, x).unwrap();
        e
    };
    let m = data.loan_value(2).unwrap();
    m.map_insert(key("one"), element(1)).unwrap();
    m.map_insert(key("two"), element(-2)).unwrap();
    // An 8-bit enum is set as an int8 (7.5.2.11.9).
    assert!(data.set_int32_value(3, 2).is_err());
    data.set_int8_value(3, 2).unwrap();
    data.set_char16_value(4, 'ω').unwrap();
    data.set_float64_value(5, 0.25).unwrap();
    for (encoding, order) in FORMATS {
        let payload = data.to_payload(encoding, order).unwrap();
        let back = DynamicData::from_payload(&derived, &payload).unwrap();
        assert_eq!(back, data, "{encoding:?} {order:?}");
    }
    // The key holder is the base's key member: 513 big endian.
    let mut want = [0u8; 16];
    want[..2].copy_from_slice(&[2, 1]);
    assert_eq!(data.key_hash().unwrap(), want);
}
