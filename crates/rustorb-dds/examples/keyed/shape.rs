//! The sample type of the instance example, written by hand in the shape
//! that `rustorb-idl -brust -Wbdds` generates for this IDL (the same as
//! `interop/idl/Keyed.idl`):
//!
//! ```idl
//! module Keyed {
//!     @final struct Shape {
//!         @key unsigned long id;
//!         unsigned long index;
//!         string message;
//!     };
//! };
//! ```

#![allow(dead_code)]

/// IDL struct `Keyed::Shape`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Shape {
    pub id: u32,
    pub index: u32,
    pub message: String,
}

impl ::rustorb_typesupport::TypeSupport for Shape {
    type Rust = Shape;

    fn write(
        v: &Shape,
        enc: &mut ::rustorb_typesupport::Encoder,
    ) -> ::rustorb_typesupport::Result<()> {
        let s = ::rustorb_typesupport::aggregate::begin(
            enc,
            ::rustorb_typesupport::Extensibility::Final,
        );
        <u32 as ::rustorb_typesupport::TypeSupport>::write(&v.id, enc)?;
        <u32 as ::rustorb_typesupport::TypeSupport>::write(&v.index, enc)?;
        <::rustorb_typesupport::Str as ::rustorb_typesupport::TypeSupport>::write(&v.message, enc)?;
        ::rustorb_typesupport::aggregate::end(enc, s)
    }

    fn read(dec: &mut ::rustorb_typesupport::Decoder<'_>) -> ::rustorb_typesupport::Result<Shape> {
        let s = ::rustorb_typesupport::aggregate::begin_read(
            dec,
            ::rustorb_typesupport::Extensibility::Final,
        )?;
        let v = Shape {
            id: <u32 as ::rustorb_typesupport::TypeSupport>::read(dec)?,
            index: <u32 as ::rustorb_typesupport::TypeSupport>::read(dec)?,
            message: <::rustorb_typesupport::Str as ::rustorb_typesupport::TypeSupport>::read(dec)?,
        };
        ::rustorb_typesupport::aggregate::end_read(dec, s)?;
        Ok(v)
    }

    fn write_key(
        v: &Shape,
        enc: &mut ::rustorb_typesupport::Encoder,
    ) -> ::rustorb_typesupport::Result<()> {
        <u32 as ::rustorb_typesupport::TypeSupport>::write_key(&v.id, enc)?;
        Ok(())
    }

    fn max_key_end(offset: usize) -> Option<usize> {
        <u32 as ::rustorb_typesupport::TypeSupport>::max_key_end(offset)
    }
}

impl ::rustorb_typesupport::TopicType for Shape {
    const TYPE_NAME: &'static str = "Keyed::Shape";
    const EXTENSIBILITY: ::rustorb_typesupport::Extensibility =
        ::rustorb_typesupport::Extensibility::Final;
    const KEYED: bool = true;
}
