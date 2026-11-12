//! The types of the service `example_interfaces/srv/AddTwoInts`, written
//! by hand in the shape that `rustorb-idl -brust -Wbdds_only` generates
//! for `interop/idl/AddTwoInts.idl`.

const _: () = ::rustorb_typesupport::GENERATED_CODE_VERSION_1;

pub mod example_interfaces {
    pub mod srv {
        pub mod dds {
            /// IDL struct `example_interfaces::srv::dds_::AddTwoInts_Request_`.
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
            pub struct AddTwoIntsRequest {
                pub a: i64,
                pub b: i64,
            }

            impl AddTwoIntsRequest {
                pub const REPOSITORY_ID: &'static str =
                    "IDL:example_interfaces/srv/dds_/AddTwoInts_Request_:1.0";
            }

            impl ::rustorb_typesupport::TypeSupport for AddTwoIntsRequest {
                type Rust = AddTwoIntsRequest;

                fn write(
                    v: &AddTwoIntsRequest,
                    enc: &mut ::rustorb_typesupport::Encoder,
                ) -> ::rustorb_typesupport::Result<()> {
                    let s = ::rustorb_typesupport::aggregate::begin(
                        enc,
                        ::rustorb_typesupport::Extensibility::Appendable,
                    );
                    <i64 as ::rustorb_typesupport::TypeSupport>::write(&v.a, enc)?;
                    <i64 as ::rustorb_typesupport::TypeSupport>::write(&v.b, enc)?;
                    ::rustorb_typesupport::aggregate::end(enc, s)
                }

                fn read(
                    dec: &mut ::rustorb_typesupport::Decoder<'_>,
                ) -> ::rustorb_typesupport::Result<AddTwoIntsRequest> {
                    let s = ::rustorb_typesupport::aggregate::begin_read(
                        dec,
                        ::rustorb_typesupport::Extensibility::Appendable,
                    )?;
                    let v = AddTwoIntsRequest {
                        a: if ::rustorb_typesupport::aggregate::has_more(dec, &s) {
                            <i64 as ::rustorb_typesupport::TypeSupport>::read(dec)?
                        } else {
                            Default::default()
                        },
                        b: if ::rustorb_typesupport::aggregate::has_more(dec, &s) {
                            <i64 as ::rustorb_typesupport::TypeSupport>::read(dec)?
                        } else {
                            Default::default()
                        },
                    };
                    ::rustorb_typesupport::aggregate::end_read(dec, s)?;
                    Ok(v)
                }

                fn write_key(
                    v: &AddTwoIntsRequest,
                    enc: &mut ::rustorb_typesupport::Encoder,
                ) -> ::rustorb_typesupport::Result<()> {
                    <i64 as ::rustorb_typesupport::TypeSupport>::write_key(&v.a, enc)?;
                    <i64 as ::rustorb_typesupport::TypeSupport>::write_key(&v.b, enc)?;
                    Ok(())
                }

                fn max_key_end(offset: usize) -> Option<usize> {
                    let offset = <i64 as ::rustorb_typesupport::TypeSupport>::max_key_end(offset)?;
                    let offset = <i64 as ::rustorb_typesupport::TypeSupport>::max_key_end(offset)?;
                    Some(offset)
                }
            }

            impl ::rustorb_typesupport::TopicType for AddTwoIntsRequest {
                const TYPE_NAME: &'static str =
                    "example_interfaces::srv::dds_::AddTwoInts_Request_";
                const EXTENSIBILITY: ::rustorb_typesupport::Extensibility =
                    ::rustorb_typesupport::Extensibility::Appendable;
                const KEYED: bool = false;
            }

            /// IDL struct `example_interfaces::srv::dds_::AddTwoInts_Response_`.
            #[derive(Debug, Clone, PartialEq, Eq, Hash, Default)]
            pub struct AddTwoIntsResponse {
                pub sum: i64,
            }

            impl AddTwoIntsResponse {
                pub const REPOSITORY_ID: &'static str =
                    "IDL:example_interfaces/srv/dds_/AddTwoInts_Response_:1.0";
            }

            impl ::rustorb_typesupport::TypeSupport for AddTwoIntsResponse {
                type Rust = AddTwoIntsResponse;

                fn write(
                    v: &AddTwoIntsResponse,
                    enc: &mut ::rustorb_typesupport::Encoder,
                ) -> ::rustorb_typesupport::Result<()> {
                    let s = ::rustorb_typesupport::aggregate::begin(
                        enc,
                        ::rustorb_typesupport::Extensibility::Appendable,
                    );
                    <i64 as ::rustorb_typesupport::TypeSupport>::write(&v.sum, enc)?;
                    ::rustorb_typesupport::aggregate::end(enc, s)
                }

                fn read(
                    dec: &mut ::rustorb_typesupport::Decoder<'_>,
                ) -> ::rustorb_typesupport::Result<AddTwoIntsResponse> {
                    let s = ::rustorb_typesupport::aggregate::begin_read(
                        dec,
                        ::rustorb_typesupport::Extensibility::Appendable,
                    )?;
                    let v = AddTwoIntsResponse {
                        sum: if ::rustorb_typesupport::aggregate::has_more(dec, &s) {
                            <i64 as ::rustorb_typesupport::TypeSupport>::read(dec)?
                        } else {
                            Default::default()
                        },
                    };
                    ::rustorb_typesupport::aggregate::end_read(dec, s)?;
                    Ok(v)
                }

                fn write_key(
                    v: &AddTwoIntsResponse,
                    enc: &mut ::rustorb_typesupport::Encoder,
                ) -> ::rustorb_typesupport::Result<()> {
                    <i64 as ::rustorb_typesupport::TypeSupport>::write_key(&v.sum, enc)?;
                    Ok(())
                }

                fn max_key_end(offset: usize) -> Option<usize> {
                    let offset = <i64 as ::rustorb_typesupport::TypeSupport>::max_key_end(offset)?;
                    Some(offset)
                }
            }

            impl ::rustorb_typesupport::TopicType for AddTwoIntsResponse {
                const TYPE_NAME: &'static str =
                    "example_interfaces::srv::dds_::AddTwoInts_Response_";
                const EXTENSIBILITY: ::rustorb_typesupport::Extensibility =
                    ::rustorb_typesupport::Extensibility::Appendable;
                const KEYED: bool = false;
            }
        }
    }
}
