//! The sample types, written by hand in the shape that
//! `rustorb-idl -brust -Wbdds` generates for `bench/idl/Perf.idl`:
//!
//! ```idl
//! module RustorbPerf {
//!     @final struct Sample {
//!         unsigned long long seq;
//!         unsigned long long stamp;
//!         sequence<octet> payload;
//!     };
//!     // One for each size N of `FIXED_SIZES`, for loaned samples.
//!     @final struct FixedN {
//!         unsigned long long seq;
//!         unsigned long long stamp;
//!         octet data[N];
//!     };
//! };
//! ```

use std::sync::{Arc, OnceLock};

use rustorb_dds::{PlainType, TopicType};
use rustorb_typesupport::{
    Array, Decoder, Encoder, Extensibility, Result, Seq, TypeSupport, aggregate,
};
use rustorb_xtypes::types::{
    ArrayType, DescribeType, Element, Primitive, StructMember, StructType, Type,
};

/// What the measurements need of a sample: its number and the time
/// it was sent.
pub trait Stamped {
    /// The sequence number the sender gave it.
    fn seq(&self) -> u64;
    /// The time it was sent, in ns since the sender's start.
    fn stamp(&self) -> u64;
    /// The octets of payload.
    fn payload_len(&self) -> usize;
    /// Sets the number and the time.
    fn set(&mut self, seq: u64, stamp: u64);
}

/// A loanable sample type.
pub trait Fixed:
    PlainType + Stamped + TopicType + DescribeType + TypeSupport<Rust = Self> + Send + Sync
{
}

impl<T> Fixed for T where
    T: PlainType + Stamped + TopicType + DescribeType + TypeSupport<Rust = T> + Send + Sync
{
}

/// Something to do with one of the `Fixed` types (`with_fixed`).
pub trait FixedVisitor {
    /// What it gives.
    type Out;
    /// Does it with `T`.
    fn visit<T: Fixed>(self) -> Self::Out;
}

/// IDL struct `RustorbPerf::Sample`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sample {
    /// The sender's sequence number.
    pub seq: u64,
    /// The time the sender sent it (ns since its start).
    pub stamp: u64,
    /// Octets of no meaning, for the size.
    pub payload: Vec<u8>,
}

impl Stamped for Sample {
    fn seq(&self) -> u64 {
        self.seq
    }
    fn stamp(&self) -> u64 {
        self.stamp
    }
    fn payload_len(&self) -> usize {
        self.payload.len()
    }
    fn set(&mut self, seq: u64, stamp: u64) {
        self.seq = seq;
        self.stamp = stamp;
    }
}

impl TypeSupport for Sample {
    type Rust = Sample;

    fn write(v: &Sample, enc: &mut Encoder) -> Result<()> {
        let s = aggregate::begin(enc, Extensibility::Final);
        <u64 as TypeSupport>::write(&v.seq, enc)?;
        <u64 as TypeSupport>::write(&v.stamp, enc)?;
        <Seq<u8> as TypeSupport>::write(&v.payload, enc)?;
        aggregate::end(enc, s)
    }

    fn read(dec: &mut Decoder<'_>) -> Result<Sample> {
        let s = aggregate::begin_read(dec, Extensibility::Final)?;
        let v = Sample {
            seq: <u64 as TypeSupport>::read(dec)?,
            stamp: <u64 as TypeSupport>::read(dec)?,
            payload: <Seq<u8> as TypeSupport>::read(dec)?,
        };
        aggregate::end_read(dec, s)?;
        Ok(v)
    }

    fn write_key(v: &Sample, enc: &mut Encoder) -> Result<()> {
        Self::write(v, enc)
    }

    fn max_key_end(_offset: usize) -> Option<usize> {
        None
    }
}

impl TopicType for Sample {
    const TYPE_NAME: &'static str = "RustorbPerf::Sample";
    const EXTENSIBILITY: Extensibility = Extensibility::Final;
    const KEYED: bool = false;
}

impl DescribeType for Sample {
    fn describe_type() -> Type {
        static TYPE: OnceLock<Type> = OnceLock::new();
        TYPE.get_or_init(|| {
            Type::from(StructType {
                name: Sample::TYPE_NAME.into(),
                extensibility: rustorb_xtypes::types::Extensibility::Final,
                members: vec![
                    StructMember::new("seq", 0, Type::Primitive(Primitive::Uint64)),
                    StructMember::new("stamp", 1, Type::Primitive(Primitive::Uint64)),
                    // IDL octet is TK_BYTE (DDS-XTypes 1.3 7.3.1.2.1.1),
                    // not TK_UINT8.
                    StructMember::new(
                        "payload",
                        2,
                        Type::sequence(Type::Primitive(Primitive::Byte)),
                    ),
                ],
                ..Default::default()
            })
        })
        .clone()
    }
}

/// Declares `RustorbPerf::Fixed<N>`, a sample of a fixed size that can be
/// loaned: its memory layout is its serialized form.
macro_rules! fixed {
    ($($name:ident = $n:literal),* $(,)?) => {
        $(
            #[doc = concat!("IDL struct `RustorbPerf::", stringify!($name), "`.")]
            #[repr(C)]
            #[derive(Clone, Copy, Debug)]
            pub struct $name {
                /// The sender's sequence number.
                pub seq: u64,
                /// The time the sender sent it (ns since its start).
                pub stamp: u64,
                /// Octets of no meaning, for the size.
                pub data: [u8; $n],
            }

            // SAFETY: repr(C), two u64 then octets: no padding, and the
            // XCDR1 form of the @final struct (8-aligned u64s from the
            // start of the payload, then the octets).
            unsafe impl PlainType for $name {}

            impl Stamped for $name {
                fn seq(&self) -> u64 {
                    self.seq
                }
                fn stamp(&self) -> u64 {
                    self.stamp
                }
                fn payload_len(&self) -> usize {
                    $n
                }
                fn set(&mut self, seq: u64, stamp: u64) {
                    self.seq = seq;
                    self.stamp = stamp;
                }
            }

            impl TypeSupport for $name {
                type Rust = $name;

                fn write(v: &$name, enc: &mut Encoder) -> Result<()> {
                    let s = aggregate::begin(enc, Extensibility::Final);
                    <u64 as TypeSupport>::write(&v.seq, enc)?;
                    <u64 as TypeSupport>::write(&v.stamp, enc)?;
                    <Array<u8, $n> as TypeSupport>::write(&v.data, enc)?;
                    aggregate::end(enc, s)
                }

                fn read(dec: &mut Decoder<'_>) -> Result<$name> {
                    let s = aggregate::begin_read(dec, Extensibility::Final)?;
                    let seq = <u64 as TypeSupport>::read(dec)?;
                    let stamp = <u64 as TypeSupport>::read(dec)?;
                    let data = <Array<u8, $n> as TypeSupport>::read(dec)?;
                    aggregate::end_read(dec, s)?;
                    Ok($name { seq, stamp, data })
                }

                fn write_key(v: &$name, enc: &mut Encoder) -> Result<()> {
                    Self::write(v, enc)
                }

                fn max_key_end(_offset: usize) -> Option<usize> {
                    None
                }
            }

            impl TopicType for $name {
                const TYPE_NAME: &'static str = concat!("RustorbPerf::", stringify!($name));
                const EXTENSIBILITY: Extensibility = Extensibility::Final;
                const KEYED: bool = false;
            }

            impl DescribeType for $name {
                fn describe_type() -> Type {
                    Type::from(StructType {
                        name: <$name as TopicType>::TYPE_NAME.into(),
                        extensibility: rustorb_xtypes::types::Extensibility::Final,
                        members: vec![
                            StructMember::new("seq", 0, Type::Primitive(Primitive::Uint64)),
                            StructMember::new("stamp", 1, Type::Primitive(Primitive::Uint64)),
                            StructMember::new(
                                "data",
                                2,
                                Type::Array(Arc::new(ArrayType {
                                    element: Element::new(Type::Primitive(Primitive::Byte)),
                                    dimensions: vec![$n],
                                })),
                            ),
                        ],
                        ..Default::default()
                    })
                }
            }
        )*

        /// The payload sizes that loaned samples can have.
        pub const FIXED_SIZES: &[usize] = &[$($n),*];

        /// Calls `visitor.visit::<FixedN>()` for the type of payload
        /// size `size`, or returns `None` when there is none.
        pub fn with_fixed<V: FixedVisitor>(size: usize, visitor: V) -> Option<V::Out> {
            match size {
                $($n => Some(visitor.visit::<$name>()),)*
                _ => None,
            }
        }
    };
}

fixed! {
    Fixed32 = 32,
    Fixed64 = 64,
    Fixed128 = 128,
    Fixed256 = 256,
    Fixed512 = 512,
    Fixed1024 = 1024,
    Fixed2048 = 2048,
    Fixed4096 = 4096,
    Fixed8192 = 8192,
    Fixed16384 = 16384,
    Fixed32768 = 32768,
    Fixed65536 = 65536,
    Fixed131072 = 131072,
    Fixed262144 = 262144,
    Fixed524288 = 524288,
    Fixed1048576 = 1048576,
}
