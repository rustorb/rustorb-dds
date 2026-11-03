//! Reading and writing the types of this crate in XCDR encoding version 2
//! (DDS-XTypes 1.3 clause 7.4.3), on top of rustorb-cdr.
//!
//! The types of Annex B are declared in IDL and are serialized as their
//! IDL says (7.3.4.5). [`Xcdr2`] is that serialization, written by hand
//! for each type: FINAL types have no header, APPENDABLE types a DHEADER,
//! MUTABLE types a DHEADER and an EMHEADER per member, sequences of
//! non-primitive elements a DHEADER, and `@optional` members of FINAL and
//! APPENDABLE types a boolean that says whether the member follows.

use rustorb_cdr::{ByteOrder, Decoder, Encoder, Encoding, LongDouble};

use crate::{Error, Result};

/// A value that has an XCDR2 serialization.
pub trait Xcdr2: Sized {
    /// Whether the type is a primitive type, whose sequences and arrays
    /// have no DHEADER (7.4.3.5.3, rules 17 and 19).
    const PRIMITIVE: bool = false;

    /// Writes the value.
    fn encode(&self, enc: &mut Encoder) -> Result<()>;

    /// Reads a value.
    fn decode(dec: &mut Decoder<'_>) -> Result<Self>;
}

/// Serializes `value` in XCDR2 with `order`, without an encapsulation
/// header: alignment counts from the first octet.
pub fn to_bytes<T: Xcdr2>(value: &T, order: ByteOrder) -> Result<Vec<u8>> {
    let mut enc = Encoder::new(order, Encoding::Xcdr2);
    value.encode(&mut enc)?;
    Ok(enc.into_bytes())
}

/// Reads a `T` serialized in XCDR2 with `order` and no encapsulation
/// header. Octets after it are an error.
pub fn from_bytes<T: Xcdr2>(buf: &[u8], order: ByteOrder) -> Result<T> {
    let mut dec = Decoder::new(buf, order, Encoding::Xcdr2);
    let value = T::decode(&mut dec)?;
    if !dec.is_empty() {
        return Err(Error::Invalid("octets after the value"));
    }
    Ok(value)
}

macro_rules! primitive {
    ($t:ty, $w:ident, $r:ident) => {
        impl Xcdr2 for $t {
            const PRIMITIVE: bool = true;
            fn encode(&self, enc: &mut Encoder) -> Result<()> {
                enc.$w(*self);
                Ok(())
            }
            fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
                Ok(dec.$r()?)
            }
        }
    };
}

primitive!(u8, write_octet, read_octet);
primitive!(bool, write_bool, read_bool);
primitive!(i16, write_i16, read_i16);
primitive!(u16, write_u16, read_u16);
primitive!(i32, write_i32, read_i32);
primitive!(u32, write_u32, read_u32);
primitive!(i64, write_i64, read_i64);
primitive!(u64, write_u64, read_u64);
primitive!(f32, write_f32, read_f32);
primitive!(f64, write_f64, read_f64);
primitive!(LongDouble, write_long_double, read_long_double);

impl Xcdr2 for i8 {
    const PRIMITIVE: bool = true;
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        enc.write_octet(*self as u8);
        Ok(())
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        Ok(dec.read_octet()? as i8)
    }
}

impl<const N: usize> Xcdr2 for [u8; N] {
    const PRIMITIVE: bool = true;
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        enc.write_octets(self);
        Ok(())
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        let bytes = dec.read_octets(N)?;
        let mut out = [0; N];
        out.copy_from_slice(bytes);
        Ok(out)
    }
}

impl Xcdr2 for String {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        enc.write_string(self)?;
        Ok(())
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        Ok(dec.read_string()?)
    }
}

impl<T: Xcdr2> Xcdr2 for Vec<T> {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        let mark = (!T::PRIMITIVE).then(|| enc.begin_dheader());
        enc.write_len(self.len())?;
        for item in self {
            item.encode(enc)?;
        }
        if let Some(mark) = mark {
            enc.end_dheader(mark)?;
        }
        Ok(())
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        let delimited = if T::PRIMITIVE {
            None
        } else {
            Some(dec.read_dheader()?)
        };
        let len = dec.read_len(1)?;
        let mut out = Vec::with_capacity(len.min(dec.remaining()));
        for _ in 0..len {
            out.push(T::decode(dec)?);
        }
        if let Some(delimited) = delimited {
            dec.end_dheader(delimited)?;
        }
        Ok(out)
    }
}

/// An `@optional` member of a FINAL or APPENDABLE type: a boolean, then the
/// value when it is true (7.4.3.5.3, rule 13 for XCDR2).
impl<T: Xcdr2> Xcdr2 for Option<T> {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        enc.write_bool(self.is_some());
        if let Some(value) = self {
            value.encode(enc)?;
        }
        Ok(())
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        if dec.read_bool()? {
            Ok(Some(T::decode(dec)?))
        } else {
            Ok(None)
        }
    }
}

/// `@external`: serialized as the value itself.
impl<T: Xcdr2> Xcdr2 for Box<T> {
    const PRIMITIVE: bool = T::PRIMITIVE;
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        (**self).encode(enc)
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        Ok(Box::new(T::decode(dec)?))
    }
}

/// Declares a struct of Annex B and its [`Xcdr2`] serialization: `final`
/// writes the members one after the other, `appendable` puts a DHEADER in
/// front of them and skips members a newer version added.
macro_rules! xstruct {
    (
        $(#[$meta:meta])*
        $kind:ident $name:ident {
            $( $(#[$fmeta:meta])* $field:ident : $ty:ty ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        pub struct $name {
            $( $(#[$fmeta])* pub $field: $ty, )*
        }

        impl $crate::xcdr2::Xcdr2 for $name {
            #[allow(unused_variables)] // empty structs
            fn encode(&self, enc: &mut ::rustorb_cdr::Encoder) -> $crate::Result<()> {
                let mark = xstruct!(@begin $kind enc);
                $( $crate::xcdr2::Xcdr2::encode(&self.$field, enc)?; )*
                xstruct!(@end $kind enc mark);
                Ok(())
            }
            #[allow(unused_variables)]
            fn decode(dec: &mut ::rustorb_cdr::Decoder<'_>) -> $crate::Result<Self> {
                let delimited = xstruct!(@open $kind dec);
                let value = $name {
                    $( $field: $crate::xcdr2::Xcdr2::decode(dec)?, )*
                };
                xstruct!(@close $kind dec delimited);
                Ok(value)
            }
        }
    };
    (@begin final $enc:ident) => { () };
    (@begin appendable $enc:ident) => { $enc.begin_dheader() };
    (@end final $enc:ident $mark:ident) => { let () = $mark; };
    (@end appendable $enc:ident $mark:ident) => { $enc.end_dheader($mark)? };
    (@open final $dec:ident) => { () };
    (@open appendable $dec:ident) => { $dec.read_dheader()? };
    (@close final $dec:ident $d:ident) => { let () = $d; };
    (@close appendable $dec:ident $d:ident) => { $dec.end_dheader($d)? };
}

pub(crate) use xstruct;

/// Writes an empty MUTABLE struct (the `Extended*` placeholders of Annex
/// B): a DHEADER of 0.
pub(crate) fn encode_empty_mutable(enc: &mut Encoder) -> Result<()> {
    let mark = enc.begin_dheader();
    enc.end_dheader(mark)?;
    Ok(())
}

/// Reads and skips a MUTABLE struct, whatever members it has.
pub(crate) fn skip_mutable(dec: &mut Decoder<'_>) -> Result<()> {
    let delimited = dec.read_dheader()?;
    dec.end_dheader(delimited)?;
    Ok(())
}
