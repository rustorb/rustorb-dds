//! The data types of the TypeLookup service (DDS-XTypes 1.3 clause
//! 7.6.3.3): the requests and replies with which participants ask each
//! other for TypeObjects, and the DDS-RPC headers around them.
//!
//! The headers and the request and reply structs are written as FINAL
//! types and the unions with a DHEADER (APPENDABLE), as the other
//! implementations do; the `_In` and `_Out` structs are MUTABLE with
//! `@hashid` member IDs (7.3.1.2.1.2). The payload is XCDR2, with the
//! encapsulation identifier of DDS-XTypes Table 60 (`0x0007` for little
//! endian).

#![allow(missing_docs)] // The members are those of the IDL, by name.

use rustorb_cdr::xcdr::XcdrFormat;
use rustorb_cdr::{ByteOrder, Decoder, Encoder, Encoding};

use crate::type_object::{
    TypeIdentifier, TypeIdentifierPair, TypeIdentifierTypeObjectPair, TypeIdentifierWithSize,
};
use crate::xcdr2::Xcdr2;
use crate::{Error, Result};

/// `TypeLookup_getTypes_HashId`: the discriminator of a getTypes call.
pub const GET_TYPES_HASH_ID: i32 = 0x018252d3;
/// `TypeLookup_getDependencies_HashId`: the discriminator of a
/// getTypeDependencies call.
pub const GET_DEPENDENCIES_HASH_ID: i32 = 0x05aafb31;

/// `DDS_RETCODE_OK`, the discriminator of a successful result.
pub const RETCODE_OK: i32 = 0;

/// The prefix of the service instance name; the participant GUID in
/// lower-case hexadecimal follows (7.6.3.3.4).
pub const INSTANCE_NAME_PREFIX: &str = "dds.builtin.TOS.";

/// The member ID that `@hashid` gives a member named `name`
/// (7.3.1.2.1.2): the first four octets of the MD5 of the name, read
/// little endian, with the top four bits cleared.
pub fn hashid(name: &str) -> u32 {
    let digest = rustorb_typesupport::md5(name.as_bytes());
    u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]) & 0x0fff_ffff
}

/// The instance name of the TypeLookup service of the participant
/// `guid` (its 16 octets, the participant entity included).
pub fn instance_name(guid: &[u8; 16]) -> String {
    let mut name = String::from(INSTANCE_NAME_PREFIX);
    for b in guid {
        name.push_str(&format!("{b:02x}"));
    }
    name
}

/// `dds::SampleIdentity`: a writer GUID and a sequence number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SampleIdentity {
    pub writer_guid: [u8; 16],
    pub sequence_number: i64,
}

impl Xcdr2 for SampleIdentity {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        enc.write_octets(&self.writer_guid);
        enc.write_i32((self.sequence_number >> 32) as i32);
        enc.write_u32(self.sequence_number as u32);
        Ok(())
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        let writer_guid = <[u8; 16]>::decode(dec)?;
        let high = dec.read_i32()?;
        let low = dec.read_u32()?;
        Ok(SampleIdentity {
            writer_guid,
            sequence_number: (i64::from(high) << 32) | i64::from(low),
        })
    }
}

/// `dds::rpc::RequestHeader`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RequestHeader {
    pub request_id: SampleIdentity,
    /// `string<255>`.
    pub instance_name: String,
}

impl Xcdr2 for RequestHeader {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        self.request_id.encode(enc)?;
        self.instance_name.encode(enc)
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        Ok(RequestHeader {
            request_id: Xcdr2::decode(dec)?,
            instance_name: Xcdr2::decode(dec)?,
        })
    }
}

/// `dds::rpc::RemoteExceptionCode_t::REMOTE_EX_OK`.
pub const REMOTE_EX_OK: i32 = 0;
/// `REMOTE_EX_UNSUPPORTED`.
pub const REMOTE_EX_UNSUPPORTED: i32 = 1;
/// `REMOTE_EX_UNKNOWN_OPERATION`.
pub const REMOTE_EX_UNKNOWN_OPERATION: i32 = 4;

/// `dds::rpc::ReplyHeader`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ReplyHeader {
    /// The request this replies to.
    pub related_request_id: SampleIdentity,
    /// A `RemoteExceptionCode_t`.
    pub remote_ex: i32,
}

impl Xcdr2 for ReplyHeader {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        self.related_request_id.encode(enc)?;
        enc.write_i32(self.remote_ex);
        Ok(())
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        Ok(ReplyHeader {
            related_request_id: Xcdr2::decode(dec)?,
            remote_ex: dec.read_i32()?,
        })
    }
}

// ---------- MUTABLE members -----------------------------------------------

/// Writes a member that is a sequence. A sequence of non-primitive
/// elements starts with its DHEADER, which serves as the NEXTINT of an
/// EMHEADER with length code 5 (7.4.3.4.8), as other implementations
/// write it; other sequences get a length code chosen by the encoder.
fn member<T: Xcdr2>(enc: &mut Encoder, id: u32, value: &Vec<T>) -> Result<()> {
    if T::PRIMITIVE {
        let mark = enc.begin_member(id, false)?;
        value.encode(enc)?;
        enc.end_member(mark)?;
    } else {
        enc.write_u32((5 << 28) | (id & 0x0fff_ffff));
        value.encode(enc)?;
    }
    Ok(())
}

/// Reads the members of a MUTABLE struct, giving each to `read` with its
/// ID; `read` returns false for members it does not know, which are
/// skipped unless they must be understood.
fn members(
    dec: &mut Decoder<'_>,
    mut read: impl FnMut(u32, &mut Decoder<'_>) -> Result<bool>,
) -> Result<()> {
    let delimited = dec.read_dheader()?;
    while delimited.has_more(dec) {
        let header = dec.read_member_header()?;
        if !read(header.id, dec)? && header.must_understand {
            return Err(Error::Invalid("unknown must-understand member"));
        }
        dec.end_member(header)?;
    }
    dec.end_dheader(delimited)?;
    Ok(())
}

/// A MUTABLE struct of this module: its members by `@hashid`.
macro_rules! mutable_struct {
    (
        $(#[$meta:meta])*
        $name:ident {
            $( $(#[$fmeta:meta])* $field:ident : $ty:ty = $hash:literal $(optional $opt:tt)? ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Default)]
        pub struct $name {
            $( $(#[$fmeta])* pub $field: mutable_struct!(@ty $ty $(, $opt)?), )*
        }

        impl Xcdr2 for $name {
            fn encode(&self, enc: &mut Encoder) -> Result<()> {
                let mark = enc.begin_dheader();
                $( mutable_struct!(@encode enc, hashid($hash), &self.$field $(, $opt)?); )*
                enc.end_dheader(mark)?;
                Ok(())
            }
            fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
                let mut value = $name::default();
                members(dec, |id, dec| {
                    $(
                        if id == hashid($hash) {
                            value.$field = mutable_struct!(@decode dec $(, $opt)?);
                            return Ok(true);
                        }
                    )*
                    Ok(false)
                })?;
                Ok(value)
            }
        }
    };
    (@ty $ty:ty) => { $ty };
    (@ty $ty:ty, $opt:tt) => { Option<$ty> };
    (@encode $enc:ident, $id:expr, $v:expr) => { member($enc, $id, $v)? };
    (@encode $enc:ident, $id:expr, $v:expr, $opt:tt) => {
        if let Some(v) = $v {
            member($enc, $id, v)?;
        }
    };
    (@decode $dec:ident) => { Xcdr2::decode($dec)? };
    (@decode $dec:ident, $opt:tt) => { Some(Xcdr2::decode($dec)?) };
}

mutable_struct! {
    /// `TypeLookup_getTypes_In`.
    GetTypesIn {
        /// The direct hash TypeIdentifiers whose TypeObjects are asked for.
        type_ids: Vec<TypeIdentifier> = "type_ids",
    }
}

mutable_struct! {
    /// `TypeLookup_getTypes_Out`.
    GetTypesOut {
        types: Vec<TypeIdentifierTypeObjectPair> = "types",
        complete_to_minimal: Vec<TypeIdentifierPair> = "complete_to_minimal",
    }
}

mutable_struct! {
    /// `TypeLookup_getTypeDependencies_In`.
    GetTypeDependenciesIn {
        type_ids: Vec<TypeIdentifier> = "type_ids",
        /// `sequence<octet, 32>`: absent in a first request.
        continuation_point: Vec<u8> = "continuation_point" optional (),
    }
}

mutable_struct! {
    /// `TypeLookup_getTypeDependencies_Out`.
    GetTypeDependenciesOut {
        dependent_typeids: Vec<TypeIdentifierWithSize> = "dependent_typeids",
        /// Absent when the list is complete.
        continuation_point: Vec<u8> = "continuation_point" optional (),
    }
}

// ---------- Unions ----------------------------------------------------------

/// `TypeLookup_Call`.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeLookupCall {
    GetTypes(GetTypesIn),
    GetTypeDependencies(GetTypeDependenciesIn),
    /// An operation this version does not know: its discriminator.
    Unknown(i32),
}

impl Xcdr2 for TypeLookupCall {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        let mark = enc.begin_dheader();
        match self {
            TypeLookupCall::GetTypes(v) => {
                enc.write_i32(GET_TYPES_HASH_ID);
                v.encode(enc)?;
            }
            TypeLookupCall::GetTypeDependencies(v) => {
                enc.write_i32(GET_DEPENDENCIES_HASH_ID);
                v.encode(enc)?;
            }
            TypeLookupCall::Unknown(d) => enc.write_i32(*d),
        }
        enc.end_dheader(mark)?;
        Ok(())
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        let delimited = dec.read_dheader()?;
        let value = match dec.read_i32()? {
            GET_TYPES_HASH_ID => TypeLookupCall::GetTypes(Xcdr2::decode(dec)?),
            GET_DEPENDENCIES_HASH_ID => TypeLookupCall::GetTypeDependencies(Xcdr2::decode(dec)?),
            other => TypeLookupCall::Unknown(other),
        };
        dec.end_dheader(delimited)?;
        Ok(value)
    }
}

/// `TypeLookup_getTypes_Result` and `TypeLookup_getTypeDependencies_Result`:
/// the output when the discriminator is `DDS_RETCODE_OK`, or the other
/// return code, which has no member.
fn encode_result<T: Xcdr2>(enc: &mut Encoder, result: &std::result::Result<T, i32>) -> Result<()> {
    let mark = enc.begin_dheader();
    match result {
        Ok(v) => {
            enc.write_i32(RETCODE_OK);
            v.encode(enc)?;
        }
        Err(code) => enc.write_i32(*code),
    }
    enc.end_dheader(mark)?;
    Ok(())
}

fn decode_result<T: Xcdr2>(dec: &mut Decoder<'_>) -> Result<std::result::Result<T, i32>> {
    let delimited = dec.read_dheader()?;
    let value = match dec.read_i32()? {
        RETCODE_OK => Ok(T::decode(dec)?),
        other => Err(other),
    };
    dec.end_dheader(delimited)?;
    Ok(value)
}

/// `TypeLookup_Return`. An `Err` is a return code other than
/// `DDS_RETCODE_OK`.
#[derive(Debug, Clone, PartialEq)]
pub enum TypeLookupReturn {
    GetTypes(std::result::Result<GetTypesOut, i32>),
    GetTypeDependencies(std::result::Result<GetTypeDependenciesOut, i32>),
    /// An operation this version does not know: its discriminator.
    Unknown(i32),
}

impl Xcdr2 for TypeLookupReturn {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        let mark = enc.begin_dheader();
        match self {
            TypeLookupReturn::GetTypes(r) => {
                enc.write_i32(GET_TYPES_HASH_ID);
                encode_result(enc, r)?;
            }
            TypeLookupReturn::GetTypeDependencies(r) => {
                enc.write_i32(GET_DEPENDENCIES_HASH_ID);
                encode_result(enc, r)?;
            }
            TypeLookupReturn::Unknown(d) => enc.write_i32(*d),
        }
        enc.end_dheader(mark)?;
        Ok(())
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        let delimited = dec.read_dheader()?;
        let value = match dec.read_i32()? {
            GET_TYPES_HASH_ID => TypeLookupReturn::GetTypes(decode_result(dec)?),
            GET_DEPENDENCIES_HASH_ID => TypeLookupReturn::GetTypeDependencies(decode_result(dec)?),
            other => TypeLookupReturn::Unknown(other),
        };
        dec.end_dheader(delimited)?;
        Ok(value)
    }
}

// ---------- Request and reply -----------------------------------------------

/// `TypeLookup_Request`.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeLookupRequest {
    pub header: RequestHeader,
    pub data: TypeLookupCall,
}

/// `TypeLookup_Reply`.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeLookupReply {
    pub header: ReplyHeader,
    pub data: TypeLookupReturn,
}

impl Xcdr2 for TypeLookupRequest {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        self.header.encode(enc)?;
        self.data.encode(enc)
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        Ok(TypeLookupRequest {
            header: Xcdr2::decode(dec)?,
            data: Xcdr2::decode(dec)?,
        })
    }
}

impl Xcdr2 for TypeLookupReply {
    fn encode(&self, enc: &mut Encoder) -> Result<()> {
        self.header.encode(enc)?;
        self.data.encode(enc)
    }
    fn decode(dec: &mut Decoder<'_>) -> Result<Self> {
        Ok(TypeLookupReply {
            header: Xcdr2::decode(dec)?,
            data: Xcdr2::decode(dec)?,
        })
    }
}

/// Writes `value` as a serialized payload: the encapsulation header of
/// PLAIN_CDR2 little endian with the identifier of Table 60 (`0x0007`),
/// the value, and padding to 4 octets recorded in the options.
fn to_payload<T: Xcdr2>(value: &T) -> Result<Vec<u8>> {
    let mut enc = Encoder::xcdr_payload(XcdrFormat::PlainCdr2, ByteOrder::Little);
    value.encode(&mut enc)?;
    let mut bytes = enc.finish_xcdr_payload();
    bytes[1] = 0x07;
    Ok(bytes)
}

/// Reads a payload written by [`to_payload`] or with any XCDR2
/// encapsulation identifier; octets after the value are ignored.
fn from_payload<T: Xcdr2>(payload: &[u8]) -> Result<T> {
    let (mut dec, header) = Decoder::xcdr_payload(payload)?;
    if header.id.format.encoding() != Encoding::Xcdr2 {
        return Err(Error::Invalid("TypeLookup payload not in XCDR2"));
    }
    T::decode(&mut dec)
}

impl TypeLookupRequest {
    /// The serialized payload of the request.
    pub fn to_payload(&self) -> Result<Vec<u8>> {
        to_payload(self)
    }

    /// Reads a serialized payload.
    pub fn from_payload(payload: &[u8]) -> Result<TypeLookupRequest> {
        from_payload(payload)
    }
}

impl TypeLookupReply {
    /// The serialized payload of the reply.
    pub fn to_payload(&self) -> Result<Vec<u8>> {
        to_payload(self)
    }

    /// Reads a serialized payload.
    pub fn from_payload(payload: &[u8]) -> Result<TypeLookupReply> {
        from_payload(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::type_object::TypeObject;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn hashids_of_the_members() {
        // As found in requests and replies of other implementations.
        assert_eq!(hashid("type_ids"), 0x0c53_6065);
        assert_eq!(hashid("types"), 0x0280_4ad1);
        assert_eq!(hashid("complete_to_minimal"), 0x0b8e_6577);
    }

    #[test]
    fn instance_names() {
        let mut guid = [0; 16];
        guid[..12].copy_from_slice(&hex("01100b2ef41826454e11bc15"));
        guid[12..].copy_from_slice(&[0, 0, 1, 0xc1]);
        assert_eq!(
            instance_name(&guid),
            "dds.builtin.TOS.01100b2ef41826454e11bc15000001c1"
        );
    }

    // A getTypes request and its reply, captured between two Cyclone DDS
    // 0.10.4 participants (one minimal hash asked for).
    const REQUEST: &str = "000700012e0b1001452618f415bc114ec30003000000000001000000310000006464732e6275696c74696e2e544f532e30313130383435623234613636306639396632383633643730303030303163310000000023000000d35282011b0000006560535c1300000001000000f1ecd054f6c026aef956e79d4e38ab00";
    const REPLY: &str = "000700005b841001f960a624d763289fc301030000000000010000000000000070000000d3528201680000000000000060000000d14a80524c00000001000000f1ecd054f6c026aef956e79d4e38ab0034000000f1510400010000000000000024000000020000000b00000001000000010004b1bc248a000c00000002000000010070001cb251ec77658e5b0400000000000000";

    #[test]
    fn reads_a_get_types_request() {
        let request = TypeLookupRequest::from_payload(&hex(REQUEST)).unwrap();
        assert_eq!(request.header.request_id.sequence_number, 1);
        assert_eq!(
            request.header.instance_name,
            "dds.builtin.TOS.0110845b24a660f99f2863d7000001c1"
        );
        let TypeLookupCall::GetTypes(call) = &request.data else {
            panic!("{:?}", request.data);
        };
        assert_eq!(call.type_ids.len(), 1);
        assert!(matches!(call.type_ids[0], TypeIdentifier::Minimal(_)));
        // Written back the same.
        assert_eq!(request.to_payload().unwrap(), hex(REQUEST));
    }

    #[test]
    fn reads_a_get_types_reply() {
        let reply = TypeLookupReply::from_payload(&hex(REPLY)).unwrap();
        assert_eq!(reply.header.remote_ex, REMOTE_EX_OK);
        let TypeLookupReturn::GetTypes(Ok(out)) = &reply.data else {
            panic!("{:?}", reply.data);
        };
        assert_eq!(out.types.len(), 1);
        assert!(out.complete_to_minimal.is_empty());
        let pair = &out.types[0];
        assert!(matches!(pair.type_object, TypeObject::Minimal(_)));
        assert_eq!(
            pair.type_object.type_identifier().unwrap(),
            pair.type_identifier
        );
        // Written back the same, but for the reply header: the related
        // request comes first.
        assert_eq!(reply.to_payload().unwrap(), hex(REPLY));
    }

    #[test]
    fn dependencies_round_trip() {
        let request = TypeLookupRequest {
            header: RequestHeader {
                request_id: SampleIdentity {
                    writer_guid: [3; 16],
                    sequence_number: (5 << 32) | 7,
                },
                instance_name: instance_name(&[9; 16]),
            },
            data: TypeLookupCall::GetTypeDependencies(GetTypeDependenciesIn {
                type_ids: vec![TypeIdentifier::Minimal([1; 14])],
                continuation_point: Some(vec![1, 2, 3]),
            }),
        };
        let bytes = request.to_payload().unwrap();
        assert_eq!(&bytes[..2], [0, 7]);
        assert_eq!(bytes.len() % 4, 0);
        assert_eq!(TypeLookupRequest::from_payload(&bytes).unwrap(), request);

        for data in [
            TypeLookupReturn::GetTypeDependencies(Ok(GetTypeDependenciesOut {
                dependent_typeids: vec![TypeIdentifierWithSize {
                    type_id: TypeIdentifier::Minimal([2; 14]),
                    typeobject_serialized_size: 40,
                }],
                continuation_point: None,
            })),
            TypeLookupReturn::GetTypes(Err(1)),
            TypeLookupReturn::Unknown(77),
        ] {
            let reply = TypeLookupReply {
                header: ReplyHeader {
                    related_request_id: request.header.request_id,
                    remote_ex: REMOTE_EX_OK,
                },
                data,
            };
            let bytes = reply.to_payload().unwrap();
            assert_eq!(TypeLookupReply::from_payload(&bytes).unwrap(), reply);
        }
    }
}
