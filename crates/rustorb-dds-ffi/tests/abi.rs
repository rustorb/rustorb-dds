//! The C ABI, called from Rust as C would call it: types built member by
//! member have the hashes of the same types described by generated code;
//! samples copied value by value through the streams serialize to the
//! octets generated code writes; and samples go from a writer to a
//! reader, with a listener and a WaitSet.

#![allow(missing_docs, clippy::missing_safety_doc)]

use std::ffi::{CStr, CString, c_void};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicUsize, Ordering};

use rustorb_cdr::{ByteOrder, Encoding};
use rustorb_dds_ffi::condition::*;
use rustorb_dds_ffi::entity::*;
use rustorb_dds_ffi::stream::*;
use rustorb_dds_ffi::types::*;
use rustorb_dds_ffi::*;
use rustorb_typesupport::TopicType;
use rustorb_xtypes::types::{DescribeType, Primitive, Type};

/// rustorb-xtypes's tests/generated: `rustorb-idl -Wbdds -Wbxtypes` of
/// tests/fixtures/cyclonedds/Fixtures.idl, in module `Gen`.
#[allow(missing_docs, dead_code, clippy::all)]
mod generated {
    include!("../../rustorb-xtypes/tests/generated/generated.rs");
}

use generated::r#gen as g;

fn ok(s: Status) {
    if s != OK {
        let msg = unsafe { CStr::from_ptr(rdds_last_error_message()) };
        panic!("status {s}: {}", msg.to_string_lossy());
    }
}

fn c(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// A type handle and its release.
struct T(*mut RddsType);

impl Drop for T {
    fn drop(&mut self) {
        unsafe { rdds_type_release(self.0) }
    }
}

fn prim(kind: u8) -> T {
    let mut t = null_mut();
    ok(unsafe { rdds_type_primitive(kind, &mut t) });
    T(t)
}

fn string(bound: u32) -> T {
    let mut t = null_mut();
    ok(unsafe { rdds_type_string(false, bound, &mut t) });
    T(t)
}

fn hashes(t: &T) -> ([u8; 14], [u8; 14]) {
    let (mut m, mut c) = ([0; 14], [0; 14]);
    ok(unsafe { rdds_type_hashes(t.0, m.as_mut_ptr(), c.as_mut_ptr()) });
    (m, c)
}

fn hashes_of(ty: &Type) -> ([u8; 14], [u8; 14]) {
    let t = T(Box::into_raw(Box::new(rustorb_dds::DynamicType::from(
        ty.clone(),
    ))));
    hashes(&t)
}

/// `@appendable struct Inner { @key long id; string<300> big; };`
fn inner_type() -> T {
    let long = prim(0x04);
    let big = string(300);
    let mut b = null_mut();
    unsafe {
        ok(rdds_struct_builder_new(
            c("Gen::Inner").as_ptr(),
            1,
            0,
            null(),
            &mut b,
        ));
        ok(rdds_struct_builder_add_member(
            b,
            c("id").as_ptr(),
            0,
            long.0,
            MEMBER_KEY,
            null(),
        ));
        ok(rdds_struct_builder_add_member(
            b,
            c("big").as_ptr(),
            1,
            big.0,
            0,
            null(),
        ));
        let mut t = null_mut();
        ok(rdds_struct_builder_build(b, &mut t));
        rdds_struct_builder_release(b);
        T(t)
    }
}

#[test]
fn types_built_member_by_member_have_the_generated_hashes() {
    let inner = inner_type();
    assert_eq!(hashes(&inner), hashes_of(&g::Inner::describe_type()));

    // @mutable struct Mut { @id(5) long a; @id(2) @optional string b;
    //   @id(9) @key short k; Inner inner; };
    let long = prim(0x04);
    let short = prim(0x03);
    let s = string(0);
    unsafe {
        let mut b = null_mut();
        ok(rdds_struct_builder_new(
            c("Gen::Mut").as_ptr(),
            2,
            0,
            null(),
            &mut b,
        ));
        ok(rdds_struct_builder_add_member(
            b,
            c("a").as_ptr(),
            5,
            long.0,
            0,
            null(),
        ));
        ok(rdds_struct_builder_add_member(
            b,
            c("b").as_ptr(),
            2,
            s.0,
            MEMBER_OPTIONAL,
            null(),
        ));
        ok(rdds_struct_builder_add_member(
            b,
            c("k").as_ptr(),
            9,
            short.0,
            MEMBER_KEY,
            null(),
        ));
        ok(rdds_struct_builder_add_member(
            b,
            c("inner").as_ptr(),
            10,
            inner.0,
            0,
            null(),
        ));
        let mut t = null_mut();
        ok(rdds_struct_builder_build(b, &mut t));
        rdds_struct_builder_release(b);
        let t = T(t);
        assert_eq!(hashes(&t), hashes_of(&g::Mut::describe_type()));
    }

    // enum Color { RED, GREEN, @value(7) BLUE };
    unsafe {
        let mut b = null_mut();
        ok(rdds_enum_builder_new(
            c("Gen::Color").as_ptr(),
            32,
            0,
            &mut b,
        ));
        for (n, v) in [("RED", 0), ("GREEN", 1), ("BLUE", 7)] {
            ok(rdds_enum_builder_add_literal(b, c(n).as_ptr(), v, false));
        }
        let mut color = null_mut();
        ok(rdds_enum_builder_build(b, &mut color));
        rdds_enum_builder_release(b);
        let color = T(color);
        let Type::Struct(all) = g::All::describe_type() else {
            panic!("All is a struct")
        };
        let expected = all.members.iter().find(|m| m.name == "color").unwrap();
        assert_eq!((*color.0).as_type(), &expected.ty);
    }
}

/// Copies the values of a sample of `ty` from `src` to `dst` as
/// generated code would, value by value in the order of the type.
unsafe fn copy(ty: &Type, src: *mut In, dst: *mut Out) {
    unsafe {
        match ty {
            Type::Alias(a) => copy(&a.related, src, dst),
            Type::Primitive(p) => match p {
                Primitive::Boolean => rdds_out_boolean(dst, rdds_in_boolean(src)),
                Primitive::Byte => rdds_out_octet(dst, rdds_in_octet(src)),
                Primitive::Int8 => rdds_out_int8(dst, rdds_in_int8(src)),
                Primitive::Uint8 => rdds_out_uint8(dst, rdds_in_uint8(src)),
                Primitive::Int16 => rdds_out_int16(dst, rdds_in_int16(src)),
                Primitive::Uint16 => rdds_out_uint16(dst, rdds_in_uint16(src)),
                Primitive::Int32 => rdds_out_int32(dst, rdds_in_int32(src)),
                Primitive::Uint32 => rdds_out_uint32(dst, rdds_in_uint32(src)),
                Primitive::Int64 => rdds_out_int64(dst, rdds_in_int64(src)),
                Primitive::Uint64 => rdds_out_uint64(dst, rdds_in_uint64(src)),
                Primitive::Float32 => rdds_out_float32(dst, rdds_in_float32(src)),
                Primitive::Float64 => rdds_out_float64(dst, rdds_in_float64(src)),
                Primitive::Float128 => {
                    let (mut h, mut l) = (0, 0);
                    rdds_in_float128(src, &mut h, &mut l);
                    rdds_out_float128(dst, h, l);
                }
                Primitive::Char8 => rdds_out_char8(dst, rdds_in_char8(src)),
                Primitive::Char16 => rdds_out_char16(dst, rdds_in_char16(src)),
            },
            Type::String { wide: false, .. } => {
                let (mut p, mut n) = (null(), 0);
                rdds_in_string(src, &mut p, &mut n);
                rdds_out_string(dst, p, n);
            }
            Type::String { wide: true, .. } => {
                let (mut p, mut n) = (null(), 0);
                rdds_in_wstring(src, &mut p, &mut n);
                rdds_out_wstring(dst, p, n);
            }
            Type::Enum(_) => rdds_out_enum(dst, rdds_in_enum(src)),
            Type::Bitmask(_) => rdds_out_bitmask(dst, rdds_in_bitmask(src)),
            Type::Sequence(s) => {
                let n = rdds_in_length(src);
                rdds_out_length(dst, n);
                elements(&s.element.ty, n as usize, src, dst);
            }
            Type::Array(a) => {
                let n = a.dimensions.iter().product::<u32>() as usize;
                elements(&a.element.ty, n, src, dst);
            }
            Type::Map(m) => {
                let n = rdds_in_length(src);
                rdds_out_length(dst, n);
                for _ in 0..n {
                    copy(&m.key.ty, src, dst);
                    copy(&m.element.ty, src, dst);
                }
            }
            Type::Struct(s) => {
                if let Some(base) = &s.base {
                    copy(base, src, dst);
                }
                for m in &s.members {
                    if m.optional {
                        let present = rdds_in_present(src);
                        rdds_out_present(dst, present);
                        if !present {
                            continue;
                        }
                    }
                    copy(&m.ty, src, dst);
                }
            }
            Type::Union(u) => {
                let d = rdds_in_discriminator(src);
                rdds_out_discriminator(dst, d);
                let d = d as i32;
                let chosen = u
                    .members
                    .iter()
                    .find(|m| m.labels.contains(&d))
                    .or_else(|| u.members.iter().find(|m| m.default));
                if let Some(m) = chosen {
                    copy(&m.ty, src, dst);
                }
            }
        }
    }
}

unsafe fn elements(ty: &Type, n: usize, src: *mut In, dst: *mut Out) {
    let mut t = ty;
    while let Type::Alias(a) = t {
        t = &a.related;
    }
    macro_rules! bulk {
        ($t:ty, $get:ident, $put:ident) => {{
            let mut v: Vec<$t> = vec![Default::default(); n];
            unsafe {
                $get(src, v.as_mut_ptr(), n);
                $put(dst, v.as_ptr(), n);
            }
        }};
    }
    match t {
        Type::Primitive(Primitive::Int32) => bulk!(i32, rdds_in_int32_array, rdds_out_int32_array),
        Type::Primitive(Primitive::Byte) => bulk!(u8, rdds_in_octet_array, rdds_out_octet_array),
        Type::Primitive(Primitive::Float64) => {
            bulk!(f64, rdds_in_float64_array, rdds_out_float64_array)
        }
        Type::Primitive(p) if *p != Primitive::Float128 => {
            panic!("no bulk copy for {p:?} in this test")
        }
        _ => {
            for _ in 0..n {
                unsafe { copy(ty, src, dst) };
            }
        }
    }
}

fn all() -> g::All {
    let inner = |id: i32| g::Inner {
        id,
        big: format!("inner {id}"),
    };
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
fn values_copied_through_the_streams_serialize_as_generated_code_does() {
    let ty = g::All::describe_type();
    let t = T(Box::into_raw(Box::new(rustorb_dds::DynamicType::from(
        ty.clone(),
    ))));
    let v = all();
    for (encoding, xcdr2) in [(Encoding::Xcdr1, false), (Encoding::Xcdr2, true)] {
        let payload = v.to_payload(encoding, ByteOrder::Little).unwrap();
        unsafe {
            let mut src = null_mut();
            ok(rdds_type_deserialize(
                t.0,
                payload.as_ptr(),
                payload.len(),
                &mut src,
            ));
            let mut dst = null_mut();
            ok(rdds_out_new(&mut dst));
            copy(&ty, src, dst);
            ok(rdds_in_status(src));
            assert!(rdds_in_at_end(src));
            let (mut p, mut n) = (null_mut(), 0);
            ok(rdds_type_serialize(t.0, dst, xcdr2, &mut p, &mut n));
            assert_eq!(std::slice::from_raw_parts(p, n), payload.as_slice());
            rdds_bytes_free(p, n);
            rdds_in_release(src);
            rdds_out_release(dst);
        }
    }
}

#[test]
fn values_that_do_not_fit_the_type_fail() {
    let inner = inner_type();
    unsafe {
        let mut out = null_mut();
        ok(rdds_out_new(&mut out));
        rdds_out_int32(out, 1);
        let (mut p, mut n) = (null_mut(), 0);
        // The string is missing.
        assert_eq!(
            rdds_type_serialize(inner.0, out, false, &mut p, &mut n),
            BAD_PARAMETER
        );
        // A string of 301 characters exceeds string<300>.
        let long = "x".repeat(301);
        rdds_out_string(out, long.as_ptr().cast(), long.len());
        assert_eq!(
            rdds_type_serialize(inner.0, out, false, &mut p, &mut n),
            BAD_PARAMETER
        );
        rdds_out_clear(out);
        rdds_out_int32(out, 1);
        rdds_out_string(out, c"ok".as_ptr(), 2);
        rdds_out_int32(out, 2);
        // One value too many.
        assert_eq!(
            rdds_type_serialize(inner.0, out, false, &mut p, &mut n),
            BAD_PARAMETER
        );
        rdds_out_release(out);

        // Reading the wrong kind sticks.
        let payload = g::Inner {
            id: 3,
            big: "b".into(),
        }
        .to_payload(Encoding::Xcdr1, ByteOrder::Little)
        .unwrap();
        let mut src = null_mut();
        ok(rdds_type_deserialize(
            inner.0,
            payload.as_ptr(),
            payload.len(),
            &mut src,
        ));
        assert_eq!(rdds_in_int64(src), 0);
        assert_eq!(rdds_in_int32(src), 0);
        assert_eq!(rdds_in_status(src), BAD_PARAMETER);
        rdds_in_release(src);
    }
}

const WAIT: RddsDuration = RddsDuration {
    sec: 10,
    nanosec: 0,
};

static DATA_AVAILABLE: AtomicUsize = AtomicUsize::new(0);
static RELEASED: AtomicUsize = AtomicUsize::new(0);

unsafe extern "C" fn on_data_available(_: *mut c_void) {
    DATA_AVAILABLE.fetch_add(1, Ordering::SeqCst);
}

unsafe extern "C" fn on_release(_: *mut c_void) {
    RELEASED.fetch_add(1, Ordering::SeqCst);
}

fn write_inner(w: *mut RddsWriter, id: i32, big: &str) {
    unsafe {
        let mut out = null_mut();
        ok(rdds_out_new(&mut out));
        rdds_out_int32(out, id);
        rdds_out_string(out, big.as_ptr().cast(), big.len());
        ok(rdds_writer_write(w, out, null()));
        rdds_out_release(out);
    }
}

#[test]
fn samples_go_from_a_writer_to_a_reader() {
    let inner = inner_type();
    unsafe {
        let mut p = null_mut();
        ok(rdds_participant_new(
            40,
            null(),
            PARTICIPANT_LOCALHOST,
            &mut p,
        ));
        let mut topic = null_mut();
        ok(rdds_participant_create_topic(
            p,
            c("inners").as_ptr(),
            inner.0,
            &mut topic,
        ));
        let mut publisher = null_mut();
        ok(rdds_participant_create_publisher(p, null(), &mut publisher));
        let parts = [c("p*")];
        let names = [parts[0].as_ptr()];
        let group = RddsGroupQos {
            partition: names.as_ptr(),
            partition_len: 1,
        };
        let mut subscriber = null_mut();
        ok(rdds_participant_create_subscriber(
            p,
            &group,
            &mut subscriber,
        ));
        let pub_names = [c"pa".as_ptr()];
        let pub_group = RddsGroupQos {
            partition: pub_names.as_ptr(),
            partition_len: 1,
        };
        let mut publisher2 = null_mut();
        ok(rdds_participant_create_publisher(
            p,
            &pub_group,
            &mut publisher2,
        ));

        let mut wq = std::mem::zeroed();
        ok(rdds_writer_qos_default(&mut wq));
        wq.history_kind = 1;
        let mut rq = std::mem::zeroed();
        ok(rdds_reader_qos_default(&mut rq));
        rq.reliability_kind = 1;
        rq.history_kind = 1;

        let listener = RddsReaderListener {
            self_: null_mut(),
            on_requested_deadline_missed: None,
            on_requested_incompatible_qos: None,
            on_sample_rejected: None,
            on_liveliness_changed: None,
            on_data_available: Some(on_data_available),
            on_subscription_matched: None,
            on_sample_lost: None,
            release: Some(on_release),
        };
        let mut r = null_mut();
        ok(rdds_subscriber_create_reader(
            subscriber,
            topic,
            &rq,
            &listener,
            1 << 10,
            &mut r,
        ));
        // Not matched: the publisher has no partition.
        let mut w0 = null_mut();
        ok(rdds_publisher_create_writer(
            publisher,
            topic,
            &wq,
            null(),
            0,
            &mut w0,
        ));
        let mut w = null_mut();
        ok(rdds_publisher_create_writer(
            publisher2,
            topic,
            &wq,
            null(),
            0,
            &mut w,
        ));
        ok(rdds_writer_wait_for_subscriptions(w, 1, WAIT));
        let mut matched = RddsMatchedStatus::default();
        ok(rdds_writer_publication_matched_status(w, &mut matched));
        assert_eq!(matched.current_count, 1);
        let mut m0 = RddsMatchedStatus::default();
        ok(rdds_writer_publication_matched_status(w0, &mut m0));
        assert_eq!(m0.current_count, 0);

        let mut cond = null_mut();
        ok(rdds_reader_create_readcondition(r, 2, 3, 7, &mut cond));
        let mut ws = null_mut();
        ok(rdds_waitset_new(&mut ws));
        ok(rdds_waitset_attach_condition(ws, cond));

        write_inner(w, 1, "one");
        write_inner(w, 2, "two");
        write_inner(w, 1, "uno");
        ok(rdds_writer_wait_for_acknowledgments(w, WAIT));

        let mut active = [usize::MAX; 4];
        let mut n = 0;
        ok(rdds_waitset_wait(ws, WAIT, active.as_mut_ptr(), 4, &mut n));
        assert_eq!((n, active[0]), (1, 0));

        let mut samples = null_mut();
        ok(rdds_reader_select_w_condition(r, true, cond, &mut samples));
        assert_eq!(rdds_samples_count(samples), 3);
        let mut got = Vec::new();
        for i in 0..3 {
            let mut info = std::mem::zeroed::<RddsSampleInfo>();
            ok(rdds_samples_info(samples, i, &mut info));
            assert!(info.valid_data);
            assert_eq!(info.instance_state, 1);
            let mut src = null_mut();
            ok(rdds_samples_data(samples, i, &mut src));
            let id = rdds_in_int32(src);
            let (mut s, mut len) = (null(), 0);
            rdds_in_string(src, &mut s, &mut len);
            let s = std::str::from_utf8(std::slice::from_raw_parts(s.cast::<u8>(), len))
                .unwrap()
                .to_owned();
            ok(rdds_in_status(src));
            rdds_in_release(src);
            got.push((id, s, info.instance_handle));
        }
        rdds_samples_release(samples);
        got.sort_by(|a, b| a.1.cmp(&b.1));
        assert_eq!(
            got.iter().map(|g| (g.0, g.1.as_str())).collect::<Vec<_>>(),
            [(1, "one"), (2, "two"), (1, "uno")]
        );
        assert_eq!(got[0].2, got[2].2);
        assert_ne!(got[0].2, got[1].2);
        assert!(DATA_AVAILABLE.load(Ordering::SeqCst) >= 1);

        // The instance of key 2, by lookup, and its key back.
        let mut key = null_mut();
        ok(rdds_out_new(&mut key));
        rdds_out_int32(key, 2);
        rdds_out_string(key, c"".as_ptr(), 0);
        let mut h = RddsInstanceHandle::default();
        ok(rdds_reader_lookup_instance(r, key, &mut h));
        assert_eq!(h, got[1].2);
        let mut kv = null_mut();
        ok(rdds_reader_get_key_value(r, h, &mut kv));
        assert_eq!(rdds_in_int32(kv), 2);
        rdds_in_release(kv);

        // Dispose it: a sample without data.
        ok(rdds_writer_dispose(w, key, null()));
        rdds_out_release(key);
        ok(rdds_reader_wait_for_data(r, WAIT));
        let mut samples = null_mut();
        ok(rdds_reader_select(
            r,
            true,
            3,
            3,
            7,
            SELECT_ANY,
            null(),
            &mut samples,
        ));
        assert_eq!(rdds_samples_count(samples), 1);
        let mut info = std::mem::zeroed::<RddsSampleInfo>();
        ok(rdds_samples_info(samples, 0, &mut info));
        assert_eq!((info.valid_data, info.instance_state), (false, 2));
        let mut src = null_mut();
        assert_eq!(rdds_samples_data(samples, 0, &mut src), NO_DATA);
        rdds_samples_release(samples);

        ok(rdds_reader_set_listener(r, null(), 0));
        assert_eq!(RELEASED.load(Ordering::SeqCst), 1);

        rdds_waitset_release(ws);
        rdds_condition_release(cond);
        rdds_writer_release(w);
        rdds_writer_release(w0);
        rdds_reader_release(r);
        rdds_publisher_release(publisher);
        rdds_publisher_release(publisher2);
        rdds_subscriber_release(subscriber);
        rdds_topic_release(topic);
        rdds_participant_release(p);
    }
}

#[test]
fn bad_arguments_fail_with_a_message() {
    unsafe {
        let mut t = null_mut();
        assert_eq!(rdds_type_primitive(0x20, &mut t), BAD_PARAMETER);
        let msg = CStr::from_ptr(rdds_last_error_message());
        assert!(msg.to_str().unwrap().contains("not a primitive"), "{msg:?}");
        assert_eq!(
            rdds_type_hashes(null(), null_mut(), null_mut()),
            BAD_PARAMETER
        );
    }
}
