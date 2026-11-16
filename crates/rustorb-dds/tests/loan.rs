//! Loaned samples (zero-copy) between participants of one host, and how
//! they fall back to copies.

#![cfg(feature = "shm")]
#![allow(missing_docs)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use rustorb_dds::{
    DataReader, DataReaderQos, DataWriter, DataWriterQos, DomainParticipant, History,
    ParticipantConfig, PlainType, ReadLoan, Reliability, Sample,
};
use rustorb_typesupport::aggregate;
use rustorb_typesupport::{Array, Decoder, Encoder, Extensibility, Result, TopicType, TypeSupport};
use rustorb_xtypes::types::{
    ArrayType, DescribeType, Element, Primitive, StructMember, StructType, Type,
};

const WAIT: Duration = Duration::from_secs(10);

/// `@final struct Frame { @key unsigned long id; unsigned long len;
/// unsigned long long stamp; octet data[N]; }`, keyed or not; or, if
/// `SWAPPED`, with `id` and `len` swapped in memory, so that its layout
/// is not its serialized form.
macro_rules! frame {
    ($name:ident, $type_name:literal, $n:literal, $keyed:literal, $swapped:literal) => {
        #[repr(C)]
        #[derive(Clone, Copy, Debug, PartialEq)]
        pub struct $name {
            pub id: u32,
            pub len: u32,
            pub stamp: u64,
            pub data: [u8; $n],
        }

        // SAFETY: repr(C) integers and an octet array.
        unsafe impl PlainType for $name {}

        impl Stamped for $name {
            fn set(&mut self, i: u32) {
                self.id = i;
                self.data[$n - 1] = i as u8;
            }
            fn get(&self) -> u32 {
                assert_eq!(self.data[$n - 1], self.id as u8);
                self.id
            }
        }

        impl TypeSupport for $name {
            type Rust = $name;

            fn write(v: &$name, enc: &mut Encoder) -> Result<()> {
                let s = aggregate::begin(enc, Extensibility::Final);
                let (a, b) = if $swapped {
                    (&v.len, &v.id)
                } else {
                    (&v.id, &v.len)
                };
                <u32 as TypeSupport>::write(a, enc)?;
                <u32 as TypeSupport>::write(b, enc)?;
                <u64 as TypeSupport>::write(&v.stamp, enc)?;
                <Array<u8, $n> as TypeSupport>::write(&v.data, enc)?;
                aggregate::end(enc, s)
            }

            fn read(dec: &mut Decoder<'_>) -> Result<$name> {
                let s = aggregate::begin_read(dec, Extensibility::Final)?;
                let a = <u32 as TypeSupport>::read(dec)?;
                let b = <u32 as TypeSupport>::read(dec)?;
                let (id, len) = if $swapped { (b, a) } else { (a, b) };
                let v = $name {
                    id,
                    len,
                    stamp: <u64 as TypeSupport>::read(dec)?,
                    data: <Array<u8, $n> as TypeSupport>::read(dec)?,
                };
                aggregate::end_read(dec, s)?;
                Ok(v)
            }

            fn write_key(v: &$name, enc: &mut Encoder) -> Result<()> {
                if $keyed {
                    <u32 as TypeSupport>::write_key(&v.id, enc)
                } else {
                    Self::write(v, enc)
                }
            }

            fn max_key_end(offset: usize) -> Option<usize> {
                <u32 as TypeSupport>::max_key_end(offset)
            }
        }

        impl TopicType for $name {
            const TYPE_NAME: &'static str = $type_name;
            const EXTENSIBILITY: Extensibility = Extensibility::Final;
            const KEYED: bool = $keyed;
        }

        impl DescribeType for $name {
            fn describe_type() -> Type {
                let u32_member = |name: &str, id: u32| {
                    StructMember::new(name, id, Type::Primitive(Primitive::Uint32))
                };
                let (a, b) = if $swapped {
                    ("len", "id")
                } else {
                    ("id", "len")
                };
                Type::from(StructType {
                    name: $type_name.into(),
                    extensibility: rustorb_xtypes::types::Extensibility::Final,
                    members: vec![
                        StructMember {
                            key: $keyed && !$swapped,
                            ..u32_member(a, 0)
                        },
                        u32_member(b, 1),
                        StructMember::new("stamp", 2, Type::Primitive(Primitive::Uint64)),
                        StructMember::new(
                            "data",
                            3,
                            Type::Array(Arc::new(ArrayType {
                                element: Element::new(Type::Primitive(Primitive::Uint8)),
                                dimensions: vec![$n],
                            })),
                        ),
                    ],
                    ..Default::default()
                })
            }
        }
    };
}

/// A sample that tells which it is.
trait Stamped {
    fn set(&mut self, i: u32);
    fn get(&self) -> u32;
}

frame!(Frame, "Loan::Frame", 16384, false, false);
frame!(KeyedFrame, "Loan::KeyedFrame", 1024, true, false);
frame!(Swapped, "Loan::Swapped", 64, false, true);

fn participant(domain: u32, shared_memory: bool) -> DomainParticipant {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    c.shared_memory = shared_memory;
    DomainParticipant::with_config(c).unwrap()
}

fn writer<T: TopicType + DescribeType + Send + Sync + 'static>(
    p: &DomainParticipant,
    topic: &str,
    history: History,
) -> DataWriter<T> {
    let t = p.create_topic::<T>(topic).unwrap();
    p.create_publisher()
        .unwrap()
        .create_datawriter_with_qos(
            &t,
            DataWriterQos {
                history,
                ..DataWriterQos::default()
            },
        )
        .unwrap()
}

fn reader<T: TopicType + DescribeType + Send + Sync + 'static>(
    p: &DomainParticipant,
    topic: &str,
) -> DataReader<T> {
    let t = p.create_topic::<T>(topic).unwrap();
    p.create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &t,
            DataReaderQos {
                reliability: Reliability::reliable(),
                history: History::KeepAll,
                ..DataReaderQos::default()
            },
        )
        .unwrap()
}

/// Takes loaned samples until there are `n`.
fn take_n<T: PlainType>(r: &DataReader<T>, n: usize) -> Vec<Sample<ReadLoan<T>>> {
    let deadline = Instant::now() + WAIT;
    let mut got = Vec::new();
    while got.len() < n && Instant::now() < deadline {
        got.extend(r.take_loaned().into_iter().filter(|s| s.data.is_some()));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(got.len(), n);
    got
}

fn write_frame(w: &DataWriter<Frame>, i: u32) -> bool {
    let mut f = w.loan_sample().unwrap();
    let shared = f.is_shared();
    f.id = i;
    f.len = 3;
    f.stamp = u64::from(i) * 1000;
    f.data[..3].copy_from_slice(b"abc");
    f.data[16383] = i as u8;
    w.write_loaned(f).unwrap();
    shared
}

fn check_frame(f: &Frame, i: u32) {
    assert_eq!((f.id, f.len, f.stamp), (i, 3, u64::from(i) * 1000));
    assert_eq!(&f.data[..3], b"abc");
    assert_eq!(f.data[16383], i as u8);
}

#[test]
fn loaned_samples_are_read_in_place() {
    let (a, b) = (participant(94, true), participant(94, true));
    let w = writer::<Frame>(&a, "Frames", History::KeepLast(4));
    let remote = reader::<Frame>(&b, "Frames");
    let local = reader::<Frame>(&a, "Frames");
    assert!(w.wait_for_subscriptions(2, WAIT));
    assert!(remote.wait_for_publications(1, WAIT));
    for i in 0..10 {
        assert!(write_frame(&w, i), "a slot of the pool");
        // Not too far ahead of the readers that hold no samples yet.
        std::thread::sleep(Duration::from_millis(2));
    }
    for r in [&remote, &local] {
        let got = take_n(r, 10);
        for (i, s) in got.iter().enumerate() {
            let f = s.data.as_ref().unwrap();
            assert!(f.is_shared(), "read in place");
            check_frame(f, i as u32);
        }
    }
    // A plain reader gets the same samples, deserialized.
    write_frame(&w, 10);
    let deadline = Instant::now() + WAIT;
    let mut got = Vec::new();
    while got.is_empty() && Instant::now() < deadline {
        got = remote.take();
        std::thread::sleep(Duration::from_millis(5));
    }
    check_frame(got[0].data.as_ref().unwrap(), 10);
}

#[test]
fn slots_are_held_until_the_readers_drop_them() {
    let (a, b) = (participant(95, true), participant(95, true));
    let w = writer::<Frame>(&a, "Held", History::KeepLast(1));
    let r = reader::<Frame>(&b, "Held");
    assert!(w.wait_for_subscriptions(1, WAIT));
    assert!(r.wait_for_publications(1, WAIT));
    // The reader holds every sample: the pool (18 slots for KEEP_LAST 1)
    // runs out, and the writer lends samples on the heap, copied.
    let mut held = Vec::new();
    let mut heap = 0;
    for i in 0..30 {
        if !write_frame(&w, i) {
            heap += 1;
        }
        held.extend(take_n(&r, 1));
    }
    assert!(heap > 0, "the pool ran out");
    for (i, s) in held.iter().enumerate() {
        check_frame(s.data.as_ref().unwrap(), i as u32);
    }
    assert!(held[0].data.as_ref().unwrap().is_shared());
    assert!(!held[29].data.as_ref().unwrap().is_shared());
    // Dropped, the slots are free again.
    drop(held);
    assert!(write_frame(&w, 30));
    let got = take_n(&r, 1);
    assert!(got[0].data.as_ref().unwrap().is_shared());
    check_frame(got[0].data.as_ref().unwrap(), 30);
}

#[test]
fn copied_to_a_reader_without_shared_memory() {
    let (a, b) = (participant(96, true), participant(96, false));
    let w = writer::<Frame>(&a, "Copied", History::KeepLast(4));
    let r = reader::<Frame>(&b, "Copied");
    let local = reader::<Frame>(&a, "Copied");
    assert!(w.wait_for_subscriptions(2, WAIT));
    assert!(r.wait_for_publications(1, WAIT));
    for i in 0..5 {
        write_frame(&w, i);
        std::thread::sleep(Duration::from_millis(2));
    }
    for (i, s) in take_n(&r, 5).iter().enumerate() {
        let f = s.data.as_ref().unwrap();
        assert!(!f.is_shared(), "sent over UDP");
        check_frame(f, i as u32);
    }
    // The reader of the same participant still reads in place.
    assert!(
        take_n(&local, 5)
            .iter()
            .all(|s| s.data.as_ref().unwrap().is_shared())
    );
}

#[test]
fn keyed_samples() {
    let (a, b) = (participant(97, true), participant(97, true));
    let w = writer::<KeyedFrame>(&a, "Keyed", History::KeepLast(2));
    let r = reader::<KeyedFrame>(&b, "Keyed");
    assert!(w.wait_for_subscriptions(1, WAIT));
    assert!(r.wait_for_publications(1, WAIT));
    for i in 0..6u32 {
        let mut f = w.loan_sample().unwrap();
        assert!(f.is_shared());
        f.id = i % 3;
        f.len = i;
        f.data[1023] = i as u8;
        w.write_loaned(f).unwrap();
        std::thread::sleep(Duration::from_millis(2));
    }
    let got = take_n(&r, 6);
    for (i, s) in got.iter().enumerate() {
        let f = s.data.as_ref().unwrap();
        assert!(f.is_shared());
        assert_eq!(
            (f.id, f.len, f.data[1023]),
            (i as u32 % 3, i as u32, i as u8)
        );
    }
    let instances: std::collections::BTreeSet<_> =
        got.iter().map(|s| s.info.instance_handle).collect();
    assert_eq!(instances.len(), 3);
    // The reader keeps each instance's key, past the sample.
    let k = r.get_key_value(got[4].info.instance_handle).unwrap();
    assert_eq!(k.id, 1);
}

#[test]
fn a_layout_that_is_not_the_serialized_form_is_serialized() {
    let (a, b) = (participant(98, true), participant(98, true));
    let w = writer::<Swapped>(&a, "Swapped", History::KeepLast(4));
    let r = reader::<Swapped>(&b, "Swapped");
    assert!(w.wait_for_subscriptions(1, WAIT));
    assert!(r.wait_for_publications(1, WAIT));
    for i in 0..3 {
        let mut f = w.loan_sample().unwrap();
        f.id = i;
        f.len = 100 + i;
        w.write_loaned(f).unwrap();
    }
    for (i, s) in take_n(&r, 3).iter().enumerate() {
        let f = s.data.as_ref().unwrap();
        assert!(!f.is_shared());
        assert_eq!((f.id, f.len), (i as u32, 100 + i as u32));
    }
}

#[test]
fn heap_loans_without_shared_memory() {
    let (a, b) = (participant(99, false), participant(99, false));
    let w = writer::<Frame>(&a, "Heap", History::KeepLast(4));
    let r = reader::<Frame>(&b, "Heap");
    assert!(w.wait_for_subscriptions(1, WAIT));
    assert!(r.wait_for_publications(1, WAIT));
    assert!(!write_frame(&w, 7));
    let got = take_n(&r, 1);
    assert!(!got[0].data.as_ref().unwrap().is_shared());
    check_frame(got[0].data.as_ref().unwrap(), 7);
}

frame!(Bench4K, "Loan::Bench4K", 4096, false, false);
frame!(Bench64K, "Loan::Bench64K", 65536, false, false);
frame!(Bench1M, "Loan::Bench1M", 1048576, false, false);

/// The time per sample of `n` samples, each written and then taken by a
/// reader of another participant, with loans or with `write` and `take`.
fn bench<T>(domain: u32, n: u32, loaned: bool) -> Duration
where
    T: PlainType + Stamped + TopicType + DescribeType + Send + Sync,
{
    let (a, b) = (participant(domain, true), participant(domain, true));
    let topic = if loaned { "BenchLoan" } else { "BenchCopy" };
    let w = writer::<T>(&a, topic, History::KeepLast(8));
    let r = reader::<T>(&b, topic);
    assert!(w.wait_for_subscriptions(1, WAIT));
    assert!(r.wait_for_publications(1, WAIT));
    let mut value = zeroed::<T>();
    let warmup = n / 10;
    let mut start = Instant::now();
    for i in 0..n + warmup {
        if i == warmup {
            start = Instant::now();
        }
        if loaned {
            let mut s = w.loan_sample().unwrap();
            s.set(i);
            w.write_loaned(s).unwrap();
        } else {
            value.set(i);
            w.write(&value).unwrap();
        }
        loop {
            let got = if loaned {
                r.take_loaned()
                    .into_iter()
                    .find_map(|s| s.data.map(|d| d.get()))
            } else {
                r.take().into_iter().find_map(|s| s.data.map(|d| d.get()))
            };
            if let Some(g) = got {
                assert_eq!(g, i);
                break;
            }
            r.wait_for_data(WAIT);
        }
    }
    start.elapsed() / n
}

/// A zero `T` on the heap.
fn zeroed<T: PlainType>() -> Box<T> {
    let layout = std::alloc::Layout::new::<T>();
    // SAFETY: zeroed, a valid `PlainType`; owned by the box.
    unsafe { Box::from_raw(std::alloc::alloc_zeroed(layout) as *mut T) }
}

/// `cargo test --release -p rustorb-dds --test loan -- --ignored --nocapture`
#[test]
#[ignore]
fn compare_loans_and_copies() {
    // Large samples: the default stack of a test thread is too small.
    std::thread::Builder::new()
        .stack_size(64 << 20)
        .spawn(|| {
            let mut domain = 80;
            let mut row = |name: &str, size: usize, f: &dyn Fn(u32, bool) -> Duration| {
                let copy = f(domain, false);
                let loan = f(domain + 1, true);
                domain += 2;
                println!(
                    "{name:>4} ({size:>7} octets): write+take {:>9.1?}, loaned {:>9.1?} ({:.1}x)",
                    copy,
                    loan,
                    copy.as_secs_f64() / loan.as_secs_f64()
                );
            };
            row("4K", 4096 + 16, &|d, l| bench::<Bench4K>(d, 2000, l));
            row("64K", 65536 + 16, &|d, l| bench::<Bench64K>(d, 1000, l));
            row("1M", 1048576 + 16, &|d, l| bench::<Bench1M>(d, 200, l));
        })
        .unwrap()
        .join()
        .unwrap();
}
