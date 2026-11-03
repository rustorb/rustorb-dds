//! The sample type of the hello world, written by hand in the shape that
//! `rustorb-idl -brust -Wbdds` generates for this IDL:
//!
//! ```idl
//! module HelloWorld {
//!     @final struct Greeting {
//!         unsigned long index;
//!         string message;
//!     };
//! };
//! ```

#![allow(dead_code)]

/// IDL struct `HelloWorld::Greeting`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
pub struct Greeting {
    pub index: u32,
    pub message: String,
}

impl ::rustorb_typesupport::TypeSupport for Greeting {
    type Rust = Greeting;

    fn write(
        v: &Greeting,
        enc: &mut ::rustorb_typesupport::Encoder,
    ) -> ::rustorb_typesupport::Result<()> {
        let s = ::rustorb_typesupport::aggregate::begin(
            enc,
            ::rustorb_typesupport::Extensibility::Final,
        );
        <u32 as ::rustorb_typesupport::TypeSupport>::write(&v.index, enc)?;
        <::rustorb_typesupport::Str as ::rustorb_typesupport::TypeSupport>::write(&v.message, enc)?;
        ::rustorb_typesupport::aggregate::end(enc, s)
    }

    fn read(
        dec: &mut ::rustorb_typesupport::Decoder<'_>,
    ) -> ::rustorb_typesupport::Result<Greeting> {
        let s = ::rustorb_typesupport::aggregate::begin_read(
            dec,
            ::rustorb_typesupport::Extensibility::Final,
        )?;
        let v = Greeting {
            index: <u32 as ::rustorb_typesupport::TypeSupport>::read(dec)?,
            message: <::rustorb_typesupport::Str as ::rustorb_typesupport::TypeSupport>::read(dec)?,
        };
        ::rustorb_typesupport::aggregate::end_read(dec, s)?;
        Ok(v)
    }

    fn write_key(
        v: &Greeting,
        enc: &mut ::rustorb_typesupport::Encoder,
    ) -> ::rustorb_typesupport::Result<()> {
        <u32 as ::rustorb_typesupport::TypeSupport>::write_key(&v.index, enc)?;
        <::rustorb_typesupport::Str as ::rustorb_typesupport::TypeSupport>::write_key(
            &v.message, enc,
        )?;
        Ok(())
    }

    fn max_key_end(offset: usize) -> Option<usize> {
        let offset = <u32 as ::rustorb_typesupport::TypeSupport>::max_key_end(offset)?;
        let offset =
            <::rustorb_typesupport::Str as ::rustorb_typesupport::TypeSupport>::max_key_end(
                offset,
            )?;
        Some(offset)
    }
}

impl ::rustorb_typesupport::TopicType for Greeting {
    const TYPE_NAME: &'static str = "HelloWorld::Greeting";
    const EXTENSIBILITY: ::rustorb_typesupport::Extensibility =
        ::rustorb_typesupport::Extensibility::Final;
    const KEYED: bool = false;
}

/// The DDS-XTypes description, as `rustorb-idl -Wbxtypes` writes it:
/// what `DomainParticipant::register_type` announces.
impl ::rustorb_xtypes::types::DescribeType for Greeting {
    fn describe_type() -> ::rustorb_xtypes::types::Type {
        use ::rustorb_xtypes::types::*;
        static TYPE: ::std::sync::OnceLock<Type> = ::std::sync::OnceLock::new();
        TYPE.get_or_init(|| {
            Type::from(StructType {
                name: "HelloWorld::Greeting".into(),
                extensibility: Extensibility::Final,
                members: vec![
                    StructMember::new("index", 0, Type::Primitive(Primitive::Uint32)),
                    StructMember::new("message", 1, Type::string()),
                ],
                ..Default::default()
            })
        })
        .clone()
    }
}
