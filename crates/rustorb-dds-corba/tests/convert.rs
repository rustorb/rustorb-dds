//! TypeCodes and anys of rustorb as DDS-XTypes types and DynamicData:
//! the TypeObjects are those of the code rustorb-idl writes for the same
//! IDL (whose TypeObjects are those of Cyclone DDS's idlc, see
//! rustorb-xtypes/tests/cyclonedds.rs), values are encoded the same, and
//! both convert back to what they were.

use rustorb_cdr::{ByteOrder, Encoding};
use rustorb_core::typecode::{TCKind, TypeCode};
use rustorb_core::{Any, LongDouble, OutStream, Value};
use rustorb_dds_corba::{Error, Options, any, dynamic_data, dynamic_type, type_code};
use rustorb_typesupport::TopicType;
use rustorb_xtypes::dynamic::{DynamicType, MEMBER_ID_INVALID};
use rustorb_xtypes::types::{DescribeType, Extensibility, TypeObjects};

#[allow(dead_code, clippy::all)]
mod generated {
    pub mod all {
        include!("../../rustorb-xtypes/tests/generated/generated.rs");
    }
    pub mod hello_world {
        include!("../../rustorb-xtypes/tests/generated/hello_world.rs");
    }
    pub mod keyed {
        include!("../../rustorb-xtypes/tests/generated/keyed.rs");
    }
}

use generated::all::r#gen as g;

fn simple(kind: TCKind) -> TypeCode {
    TypeCode::simple(kind).unwrap()
}

fn struct_tc(id: &str, name: &str, members: &[(&str, TypeCode)]) -> TypeCode {
    TypeCode::struct_tc(id, name, members).unwrap()
}

// ---------- the TypeCodes rustorb has for the fixture IDL ---------------

fn greeting_tc() -> TypeCode {
    struct_tc(
        "IDL:HelloWorld/Greeting:1.0",
        "Greeting",
        &[
            ("index", simple(TCKind::ULong)),
            ("message", TypeCode::string(0)),
        ],
    )
}

fn shape_tc() -> TypeCode {
    struct_tc(
        "IDL:Keyed/Shape:1.0",
        "Shape",
        &[
            ("id", simple(TCKind::ULong)),
            ("index", simple(TCKind::ULong)),
            ("message", TypeCode::string(0)),
        ],
    )
}

fn plain_tc() -> TypeCode {
    use TCKind::*;
    struct_tc(
        "IDL:Gen/Plain:1.0",
        "Plain",
        &[
            ("b", simple(Boolean)),
            ("o", simple(Octet)),
            ("s", simple(Short)),
            ("us", simple(UShort)),
            ("l", simple(Long)),
            ("ul", simple(ULong)),
            ("ll", simple(LongLong)),
            ("ull", simple(ULongLong)),
            ("f", simple(Float)),
            ("d", simple(Double)),
            ("c", simple(Char)),
        ],
    )
}

fn inner_tc() -> TypeCode {
    struct_tc(
        "IDL:Gen/Inner:1.0",
        "Inner",
        &[("id", simple(TCKind::Long)), ("big", TypeCode::string(300))],
    )
}

fn u_tc() -> TypeCode {
    TypeCode::union_tc(
        "IDL:Gen/U:1.0",
        "U",
        &simple(TCKind::Long),
        &[
            (Some(1), "x", simple(TCKind::Long)),
            (Some(2), "x", simple(TCKind::Long)),
            (Some(3), "s", TypeCode::string(0)),
            (None, "i", inner_tc()),
        ],
    )
    .unwrap()
}

/// What rustorb-idl and Cyclone DDS make of the fixture IDL: `@final`
/// where the IDL says so (HelloWorld and Keyed are compiled with
/// `-Wbextensibility=final`), `@appendable` by default.
fn fixture_options() -> Options {
    Options::new()
        .extensibility("HelloWorld::Greeting", Extensibility::Final)
        .extensibility("Keyed::Shape", Extensibility::Final)
        .extensibility("Gen::Plain", Extensibility::Final)
        .extensibility("Gen::U", Extensibility::Final)
        .key("Keyed::Shape", "id")
        .key("Gen::Inner", "id")
}

fn check_like_generated<T: DescribeType>(tc: &TypeCode) {
    let ty = dynamic_type(tc, &fixture_options()).unwrap();
    let expected = T::describe_type();
    assert_eq!(ty.as_type(), &expected, "{:?}", tc.id());
    let converted = TypeObjects::new(ty.as_type()).unwrap();
    let generated = TypeObjects::of::<T>().unwrap();
    assert_eq!(converted.minimal_id(), generated.minimal_id());
    assert_eq!(converted.complete_id(), generated.complete_id());
    assert_eq!(converted.information, generated.information);
    // And back: the same TypeCode.
    let back = type_code(&ty).unwrap();
    assert!(back.equal(tc), "{back:?} is not {tc:?}");
}

#[test]
fn type_objects_like_generated_code() {
    check_like_generated::<generated::hello_world::hello_world::Greeting>(&greeting_tc());
    check_like_generated::<generated::keyed::keyed::Shape>(&shape_tc());
    check_like_generated::<g::Plain>(&plain_tc());
    check_like_generated::<g::Inner>(&inner_tc());
    check_like_generated::<g::U>(&u_tc());
}

/// An any converted to DynamicData is encoded as rustorb-idl's code
/// encodes the same value.
fn check_payload<T: TopicType>(value: &T, any_value: Any) {
    let ty = dynamic_type(any_value.typecode(), &fixture_options()).unwrap();
    let data = dynamic_data(&any_value, &ty).unwrap();
    for encoding in [Encoding::Xcdr1, Encoding::Xcdr2] {
        assert_eq!(
            data.to_payload(encoding, ByteOrder::Little).unwrap(),
            value.to_payload(encoding, ByteOrder::Little).unwrap(),
            "{encoding:?}"
        );
    }
    if ty.is_keyed() {
        assert_eq!(data.key_hash().unwrap(), value.key_hash().unwrap());
    }
    let back = any(&data, any_value.typecode()).unwrap();
    assert_eq!(back.value(), any_value.value());
}

#[test]
fn values_encoded_like_generated_code() {
    check_payload(
        &generated::hello_world::hello_world::Greeting {
            index: 7,
            message: "Hello, world!".into(),
        },
        Any::new(
            greeting_tc(),
            Value::List(vec![Value::ULong(7), Value::String("Hello, world!".into())]),
        ),
    );
    check_payload(
        &generated::keyed::keyed::Shape {
            id: 3,
            index: 9,
            message: "square".into(),
        },
        Any::new(
            shape_tc(),
            Value::List(vec![
                Value::ULong(3),
                Value::ULong(9),
                Value::String("square".into()),
            ]),
        ),
    );
    check_payload(
        &g::Plain {
            b: true,
            o: 0xab,
            s: -2,
            us: 3,
            l: -4,
            ul: 5,
            ll: -6,
            ull: 7,
            f: 1.5,
            d: -2.25,
            c: 'z',
        },
        Any::new(
            plain_tc(),
            Value::List(vec![
                Value::Bool(true),
                Value::Octet(0xab),
                Value::Short(-2),
                Value::UShort(3),
                Value::Long(-4),
                Value::ULong(5),
                Value::LongLong(-6),
                Value::ULongLong(7),
                Value::Float(1.5),
                Value::Double(-2.25),
                Value::Char('z'),
            ]),
        ),
    );
    let inner = Value::List(vec![Value::Long(1), Value::String("big".into())]);
    check_payload(
        &g::Inner {
            id: 1,
            big: "big".into(),
        },
        Any::new(inner_tc(), inner.clone()),
    );
    for (value, disc, member) in [
        (g::U::X(2, 10), Value::Long(2), Some((1, Value::Long(10)))),
        (
            g::U::S("s".into()),
            Value::Long(3),
            Some((2, Value::String("s".into()))),
        ),
        (
            g::U::I(
                42,
                g::Inner {
                    id: 1,
                    big: "big".into(),
                },
            ),
            Value::Long(42),
            Some((3, inner.clone())),
        ),
    ] {
        check_payload(
            &value,
            Any::new(
                u_tc(),
                Value::Union {
                    disc: Box::new(disc),
                    member: member.map(|(i, v)| (i, Box::new(v))),
                },
            ),
        );
    }
}

// ---------- every kind a TypeCode and a DynamicType share --------------

fn color_tc(prefix: &str) -> TypeCode {
    TypeCode::enum_tc(
        &format!("IDL:{prefix}M/Color:1.0"),
        "Color",
        &["RED", "GREEN", "BLUE"],
    )
    .unwrap()
}

/// `module M { ... struct Every { ... }; };`, with `#pragma prefix` `prefix`
/// (`"my.org/"`, or `""` for none).
fn every_tc(prefix: &str) -> TypeCode {
    let id = |name: &str| format!("IDL:{prefix}M/{name}:1.0");
    let matrix = TypeCode::alias(
        &id("Matrix"),
        "Matrix",
        &TypeCode::array(&TypeCode::array(&simple(TCKind::Long), 3).unwrap(), 2).unwrap(),
    )
    .unwrap();
    let point = struct_tc(
        &id("Point"),
        "Point",
        &[("x", simple(TCKind::Double)), ("y", simple(TCKind::Double))],
    );
    let points = TypeCode::alias(
        &id("Points"),
        "Points",
        &TypeCode::sequence(&point, 0).unwrap(),
    )
    .unwrap();
    let shape = TypeCode::union_tc(
        &id("Shape"),
        "Shape",
        &color_tc(prefix),
        &[
            (Some(0), "radius", simple(TCKind::Float)),
            (Some(2), "corners", points.clone()),
            (Some(1), "corners", points.clone()),
        ],
    )
    .unwrap();
    let by_char = TypeCode::union_tc(
        &id("ByChar"),
        "ByChar",
        &simple(TCKind::Char),
        &[
            (Some(u32::from('a').into()), "a", simple(TCKind::Short)),
            (None, "other", TypeCode::string(8)),
        ],
    )
    .unwrap();
    struct_tc(
        &id("Every"),
        "Every",
        &[
            ("color", color_tc(prefix)),
            ("m", matrix),
            ("points", points),
            ("shape", shape.clone()),
            ("shapes", TypeCode::array(&shape, 2).unwrap()),
            ("by_char", by_char),
            (
                "bytes",
                TypeCode::sequence(&simple(TCKind::Octet), 16).unwrap(),
            ),
            ("block", TypeCode::array(&simple(TCKind::Octet), 4).unwrap()),
            (
                "letters",
                TypeCode::array(&simple(TCKind::Char), 3).unwrap(),
            ),
            (
                "names",
                TypeCode::sequence(&TypeCode::string(10), 0).unwrap(),
            ),
            (
                "nested",
                TypeCode::sequence(
                    &TypeCode::sequence(&simple(TCKind::ULongLong), 0).unwrap(),
                    0,
                )
                .unwrap(),
            ),
            ("ld", simple(TCKind::LongDouble)),
        ],
    )
}

fn every_value() -> Value {
    let point = |x, y| Value::List(vec![Value::Double(x), Value::Double(y)]);
    let points = Value::List(vec![point(0.0, 1.0), point(2.0, 3.0)]);
    let shape = |disc, member| Value::Union {
        disc: Box::new(Value::Enum(disc)),
        member: Some(member),
    };
    Value::List(vec![
        Value::Enum(2),
        Value::List(vec![
            Value::List((1..=3).map(Value::Long).collect()),
            Value::List((4..=6).map(Value::Long).collect()),
        ]),
        points.clone(),
        // GREEN: the second label of "corners", the third TypeCode member.
        shape(1, (2, Box::new(points))),
        Value::List(vec![
            shape(0, (0, Box::new(Value::Float(0.5)))),
            shape(2, (1, Box::new(Value::List(vec![])))),
        ]),
        Value::Union {
            disc: Box::new(Value::Char('q')),
            member: Some((1, Box::new(Value::String("default".into())))),
        },
        Value::Octets(vec![1, 2, 3].into()),
        Value::Octets(vec![9, 8, 7, 6].into()),
        Value::Chars("abc".into()),
        Value::List(vec![
            Value::String("one".into()),
            Value::String("two".into()),
        ]),
        Value::List(vec![
            Value::List(vec![]),
            Value::List(vec![Value::ULongLong(u64::MAX)]),
        ]),
        Value::LongDouble(LongDouble::from_f64(-0.125)),
    ])
}

/// The value encoded by rustorb's CDR (GIOP 1.2, little endian): the
/// same octets as XCDR1 for final types without wide characters.
fn corba_cdr(any: &Any) -> Vec<u8> {
    let mut enc = rustorb_core::cdr::Encoder::new(
        rustorb_core::cdr::ByteOrder::Little,
        rustorb_core::cdr::Encoding::Giop1_2,
    );
    OutStream::standalone(&mut enc)
        .write_value(any.typecode(), any.value())
        .unwrap();
    enc.into_bytes()
}

#[test]
fn every_kind_round_trips() {
    let tc = every_tc("my.org/");
    let options = Options::new().default_extensibility(Extensibility::Final);
    let ty = dynamic_type(&tc, &options).unwrap();
    assert_eq!(ty.get_name(), "M::Every");

    // TypeCode -> type -> TypeCode: the same type, the prefix left out of
    // the repository IDs.
    let back = type_code(&ty).unwrap();
    assert_eq!(back.id().unwrap(), "IDL:M/Every:1.0");
    assert!(back.equal(&every_tc("")), "{back:?}");
    // ... and again to the same DDS-XTypes type and TypeObject.
    let again = dynamic_type(&back, &options).unwrap();
    assert_eq!(again.as_type(), ty.as_type());
    assert_eq!(
        TypeObjects::new(again.as_type()).unwrap().complete_id(),
        TypeObjects::new(ty.as_type()).unwrap().complete_id()
    );

    // Any -> DynamicData -> Any.
    let value = Any::new(tc.clone(), every_value());
    let data = dynamic_data(&value, &ty).unwrap();
    assert_eq!(
        data.get_value(data.get_member_id_by_name("color"))
            .unwrap()
            .get_int32_value(MEMBER_ID_INVALID)
            .unwrap(),
        2
    );
    let m = data.get_value(data.get_member_id_by_name("m")).unwrap();
    assert_eq!(
        m.get_int32_values(MEMBER_ID_INVALID).unwrap(),
        vec![1, 2, 3, 4, 5, 6]
    );
    let shape = data.get_value(data.get_member_id_by_name("shape")).unwrap();
    assert_eq!(shape.get_discriminator_value().unwrap(), 1);
    let back_value = any(&data, &tc).unwrap();
    assert_eq!(back_value.value(), value.value());

    // The DynamicData encoded as XCDR1 is the CDR rustorb writes.
    let payload = data.to_payload(Encoding::Xcdr1, ByteOrder::Little).unwrap();
    assert_eq!(&payload[4..], &corba_cdr(&value)[..], "XCDR1 and CDR");
    // ... and it reads back to the same value.
    let read = rustorb_xtypes::dynamic::DynamicData::from_payload(&ty, &payload).unwrap();
    assert!(read.equals(&data));
}

#[test]
fn wide_characters() {
    let tc = struct_tc(
        "IDL:W:1.0",
        "W",
        &[("c", simple(TCKind::WChar)), ("s", TypeCode::wstring(5))],
    );
    let ty = dynamic_type(&tc, &Options::new()).unwrap();
    let value = Any::new(
        tc.clone(),
        Value::List(vec![Value::WChar(0x3042), Value::String("ことば".into())]),
    );
    let data = dynamic_data(&value, &ty).unwrap();
    assert_eq!(data.get_char16_value(0).unwrap(), 'あ');
    assert_eq!(any(&data, &tc).unwrap().value(), value.value());
    // Longer than the bound.
    let long = Any::new(
        tc,
        Value::List(vec![Value::WChar(0x41), Value::String("abcdef".into())]),
    );
    assert!(dynamic_data(&long, &ty).is_err());
}

#[test]
fn defaults_and_options() {
    let ty = dynamic_type(&inner_tc(), &Options::new()).unwrap();
    // Appendable by default, no keys.
    let generated = g::Inner::describe_type();
    assert_ne!(ty.as_type(), &generated);
    assert!(!ty.is_keyed());
    // A key that is not there.
    let wrong = Options::new().key("Gen::Inner", "nope");
    assert!(matches!(
        dynamic_type(&inner_tc(), &wrong),
        Err(Error::Mismatch(_))
    ));
}

#[test]
fn unsupported_kinds() {
    let options = Options::new();
    for tc in [
        simple(TCKind::Any),
        simple(TCKind::TypeCode),
        TypeCode::objref("IDL:I:1.0", "I").unwrap(),
        TypeCode::fixed(5, 2).unwrap(),
        struct_tc("IDL:S:1.0", "S", &[("a", simple(TCKind::Any))]),
    ] {
        assert!(
            matches!(dynamic_type(&tc, &options), Err(Error::Unsupported(_))),
            "{tc:?}"
        );
    }
    // A recursive struct.
    let node = struct_tc(
        "IDL:Node:1.0",
        "Node",
        &[(
            "children",
            TypeCode::sequence(&TypeCode::recursive("IDL:Node:1.0"), 0).unwrap(),
        )],
    );
    assert!(matches!(
        dynamic_type(&node, &options),
        Err(Error::Unsupported(_))
    ));
    // The other way: optional members, bitmasks, int8.
    let mutable = DynamicType::new(g::Mut::describe_type());
    assert!(matches!(type_code(&mutable), Err(Error::Unsupported(_))));
    let all = DynamicType::new(g::All::describe_type());
    assert!(matches!(type_code(&all), Err(Error::Unsupported(_))));
}

#[test]
fn mismatched_values() {
    let ty = dynamic_type(&greeting_tc(), &Options::new()).unwrap();
    let wrong = Any::new(
        greeting_tc(),
        Value::List(vec![Value::String("x".into()), Value::ULong(1)]),
    );
    assert!(dynamic_data(&wrong, &ty).is_err());
    let short = Any::new(greeting_tc(), Value::List(vec![Value::ULong(1)]));
    assert!(dynamic_data(&short, &ty).is_err());
}
