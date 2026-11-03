//! PL_CDR: parameter lists whose values are CDR, as used by the payloads
//! of the discovery built-in endpoints (DDSI-RTPS 2.5, 9.6.3 and 10.3).
//!
//! The payload starts with the 4-octet SerializedPayloadHeader
//! (`PL_CDR_BE` or `PL_CDR_LE`), followed by a parameter list ending with
//! `PID_SENTINEL`. Each value is its own CDR stream: alignment restarts at
//! the start of the value (9.4.2.11.1).

use crate::error::DecodeError;
use crate::parameter::{Parameter, ParameterId, ParameterList};
use crate::types::{Duration, EntityId, Guid, GuidPrefix, Locator, ProtocolVersion, VendorId};
use crate::wire::{Endianness, Reader, Writer};

/// Parameter ids (DDSI-RTPS 2.5, Tables 9.18 and 9.20).
pub mod pid {
    use crate::parameter::ParameterId;

    /// `PID_PAD`.
    pub const PAD: ParameterId = ParameterId(0x0000);
    /// `PID_SENTINEL`.
    pub const SENTINEL: ParameterId = ParameterId(0x0001);
    /// `PID_PARTICIPANT_LEASE_DURATION`.
    pub const PARTICIPANT_LEASE_DURATION: ParameterId = ParameterId(0x0002);
    /// `PID_TIME_BASED_FILTER`.
    pub const TIME_BASED_FILTER: ParameterId = ParameterId(0x0004);
    /// `PID_TOPIC_NAME`.
    pub const TOPIC_NAME: ParameterId = ParameterId(0x0005);
    /// `PID_OWNERSHIP_STRENGTH`.
    pub const OWNERSHIP_STRENGTH: ParameterId = ParameterId(0x0006);
    /// `PID_TYPE_NAME`.
    pub const TYPE_NAME: ParameterId = ParameterId(0x0007);
    /// `PID_DOMAIN_ID`.
    pub const DOMAIN_ID: ParameterId = ParameterId(0x000f);
    /// `PID_PROTOCOL_VERSION`.
    pub const PROTOCOL_VERSION: ParameterId = ParameterId(0x0015);
    /// `PID_VENDORID`.
    pub const VENDORID: ParameterId = ParameterId(0x0016);
    /// `PID_RELIABILITY`.
    pub const RELIABILITY: ParameterId = ParameterId(0x001a);
    /// `PID_LIVELINESS`.
    pub const LIVELINESS: ParameterId = ParameterId(0x001b);
    /// `PID_DURABILITY`.
    pub const DURABILITY: ParameterId = ParameterId(0x001d);
    /// `PID_DURABILITY_SERVICE`.
    pub const DURABILITY_SERVICE: ParameterId = ParameterId(0x001e);
    /// `PID_OWNERSHIP`.
    pub const OWNERSHIP: ParameterId = ParameterId(0x001f);
    /// `PID_PRESENTATION`.
    pub const PRESENTATION: ParameterId = ParameterId(0x0021);
    /// `PID_DEADLINE`.
    pub const DEADLINE: ParameterId = ParameterId(0x0023);
    /// `PID_DESTINATION_ORDER`.
    pub const DESTINATION_ORDER: ParameterId = ParameterId(0x0025);
    /// `PID_LATENCY_BUDGET`.
    pub const LATENCY_BUDGET: ParameterId = ParameterId(0x0027);
    /// `PID_PARTITION`.
    pub const PARTITION: ParameterId = ParameterId(0x0029);
    /// `PID_LIFESPAN`.
    pub const LIFESPAN: ParameterId = ParameterId(0x002b);
    /// `PID_USER_DATA`.
    pub const USER_DATA: ParameterId = ParameterId(0x002c);
    /// `PID_GROUP_DATA`.
    pub const GROUP_DATA: ParameterId = ParameterId(0x002d);
    /// `PID_TOPIC_DATA`.
    pub const TOPIC_DATA: ParameterId = ParameterId(0x002e);
    /// `PID_UNICAST_LOCATOR`.
    pub const UNICAST_LOCATOR: ParameterId = ParameterId(0x002f);
    /// `PID_MULTICAST_LOCATOR`.
    pub const MULTICAST_LOCATOR: ParameterId = ParameterId(0x0030);
    /// `PID_DEFAULT_UNICAST_LOCATOR`.
    pub const DEFAULT_UNICAST_LOCATOR: ParameterId = ParameterId(0x0031);
    /// `PID_METATRAFFIC_UNICAST_LOCATOR`.
    pub const METATRAFFIC_UNICAST_LOCATOR: ParameterId = ParameterId(0x0032);
    /// `PID_METATRAFFIC_MULTICAST_LOCATOR`.
    pub const METATRAFFIC_MULTICAST_LOCATOR: ParameterId = ParameterId(0x0033);
    /// `PID_PARTICIPANT_MANUAL_LIVELINESS_COUNT`.
    pub const PARTICIPANT_MANUAL_LIVELINESS_COUNT: ParameterId = ParameterId(0x0034);
    /// `PID_CONTENT_FILTER_PROPERTY`.
    pub const CONTENT_FILTER_PROPERTY: ParameterId = ParameterId(0x0035);
    /// `PID_HISTORY`.
    pub const HISTORY: ParameterId = ParameterId(0x0040);
    /// `PID_RESOURCE_LIMITS`.
    pub const RESOURCE_LIMITS: ParameterId = ParameterId(0x0041);
    /// `PID_EXPECTS_INLINE_QOS`.
    pub const EXPECTS_INLINE_QOS: ParameterId = ParameterId(0x0043);
    /// `PID_DEFAULT_MULTICAST_LOCATOR`.
    pub const DEFAULT_MULTICAST_LOCATOR: ParameterId = ParameterId(0x0048);
    /// `PID_TRANSPORT_PRIORITY`.
    pub const TRANSPORT_PRIORITY: ParameterId = ParameterId(0x0049);
    /// `PID_PARTICIPANT_GUID`.
    pub const PARTICIPANT_GUID: ParameterId = ParameterId(0x0050);
    /// `PID_GROUP_GUID`.
    pub const GROUP_GUID: ParameterId = ParameterId(0x0052);
    /// `PID_GROUP_ENTITY_ID`.
    pub const GROUP_ENTITY_ID: ParameterId = ParameterId(0x0053);
    /// `PID_CONTENT_FILTER_INFO`.
    pub const CONTENT_FILTER_INFO: ParameterId = ParameterId(0x0055);
    /// `PID_COHERENT_SET`.
    pub const COHERENT_SET: ParameterId = ParameterId(0x0056);
    /// `PID_DIRECTED_WRITE`.
    pub const DIRECTED_WRITE: ParameterId = ParameterId(0x0057);
    /// `PID_BUILTIN_ENDPOINT_SET`.
    pub const BUILTIN_ENDPOINT_SET: ParameterId = ParameterId(0x0058);
    /// `PID_PROPERTY_LIST`.
    pub const PROPERTY_LIST: ParameterId = ParameterId(0x0059);
    /// `PID_ENDPOINT_GUID`.
    pub const ENDPOINT_GUID: ParameterId = ParameterId(0x005a);
    /// `PID_TYPE_MAX_SIZE_SERIALIZED`.
    pub const TYPE_MAX_SIZE_SERIALIZED: ParameterId = ParameterId(0x0060);
    /// `PID_ORIGINAL_WRITER_INFO`.
    pub const ORIGINAL_WRITER_INFO: ParameterId = ParameterId(0x0061);
    /// `PID_ENTITY_NAME`.
    pub const ENTITY_NAME: ParameterId = ParameterId(0x0062);
    /// `PID_GROUP_COHERENT_SET`.
    pub const GROUP_COHERENT_SET: ParameterId = ParameterId(0x0063);
    /// `PID_GROUP_SEQ_NUM`.
    pub const GROUP_SEQ_NUM: ParameterId = ParameterId(0x0064);
    /// `PID_WRITER_GROUP_INFO`.
    pub const WRITER_GROUP_INFO: ParameterId = ParameterId(0x0065);
    /// `PID_SECURE_WRITER_GROUP_INFO`.
    pub const SECURE_WRITER_GROUP_INFO: ParameterId = ParameterId(0x0066);
    /// `PID_KEY_HASH`.
    pub const KEY_HASH: ParameterId = ParameterId(0x0070);
    /// `PID_STATUS_INFO`.
    pub const STATUS_INFO: ParameterId = ParameterId(0x0071);
    /// `PID_BUILTIN_ENDPOINT_QOS`.
    pub const BUILTIN_ENDPOINT_QOS: ParameterId = ParameterId(0x0077);
    /// `PID_DOMAIN_TAG`.
    pub const DOMAIN_TAG: ParameterId = ParameterId(0x4014);
}

/// `PL_CDR_BE` (Table 10.1).
pub const PL_CDR_BE: [u8; 2] = [0x00, 0x02];
/// `PL_CDR_LE` (Table 10.1).
pub const PL_CDR_LE: [u8; 2] = [0x00, 0x03];

/// Writes a payload: the `PL_CDR_*` header, the list and the sentinel.
/// The options are zero (10.3).
pub fn encode_pl_payload(list: &ParameterList, endianness: Endianness) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(match endianness {
        Endianness::Big => &PL_CDR_BE,
        Endianness::Little => &PL_CDR_LE,
    });
    out.extend_from_slice(&[0, 0]);
    list.encode(&mut out, endianness, true)
        .expect("values built by this module are short");
    out
}

/// Reads a payload written with `PL_CDR_BE` or `PL_CDR_LE`. The options
/// are ignored (10.3).
pub fn decode_pl_payload(bytes: &[u8]) -> Result<(ParameterList, Endianness), DecodeError> {
    if bytes.len() < 4 {
        return Err(DecodeError::UnexpectedEnd);
    }
    let id = [bytes[0], bytes[1]];
    let endianness = match id {
        PL_CDR_BE => Endianness::Big,
        PL_CDR_LE => Endianness::Little,
        _ => return Err(DecodeError::UnknownRepresentation(id)),
    };
    let (list, _) = ParameterList::decode(&bytes[4..], endianness, true)?;
    Ok((list, endianness))
}

fn bad(id: ParameterId) -> DecodeError {
    DecodeError::InvalidParameterValue(id)
}

fn read_string(r: &mut Reader<'_>) -> Option<String> {
    let len = r.u32().ok()? as usize;
    let bytes = r.bytes(len).ok()?;
    let bytes = match bytes.split_last() {
        Some((0, rest)) => rest,
        Some(_) => return None,
        // A length of 0 is not valid CDR, but means the empty string.
        None => bytes,
    };
    String::from_utf8(bytes.to_vec()).ok()
}

fn write_string(w: &mut Writer<'_>, v: &str) {
    w.u32(v.len() as u32 + 1);
    w.bytes(v.as_bytes());
    w.bytes(&[0]);
}

/// Reads parameter values in the byte order of their list.
#[derive(Clone, Copy, Debug)]
pub struct PlValues {
    endianness: Endianness,
}

impl PlValues {
    /// Values in the given byte order.
    pub fn new(endianness: Endianness) -> Self {
        PlValues { endianness }
    }

    fn reader<'a>(&self, p: &'a Parameter) -> Reader<'a> {
        Reader::new(&p.value, self.endianness)
    }

    /// `unsigned long` or `long` as `u32`.
    pub fn u32(&self, p: &Parameter) -> Result<u32, DecodeError> {
        self.reader(p).u32().map_err(|_| bad(p.id))
    }

    /// `boolean`: one octet, non-zero is true.
    pub fn bool(&self, p: &Parameter) -> Result<bool, DecodeError> {
        p.value.first().map(|b| *b != 0).ok_or(bad(p.id))
    }

    /// `string`: length (with the terminating NUL), octets, NUL.
    pub fn string(&self, p: &Parameter) -> Result<String, DecodeError> {
        read_string(&mut self.reader(p)).ok_or(bad(p.id))
    }

    /// `sequence<string>`, as in `PartitionQosPolicy`. Each string's
    /// length is aligned to 4 from the start of the value.
    pub fn strings(&self, p: &Parameter) -> Result<Vec<String>, DecodeError> {
        let mut r = self.reader(p);
        let n = r.u32().map_err(|_| bad(p.id))? as usize;
        // Each string takes at least 4 octets, which bounds `n`.
        if n > r.remaining() / 4 {
            return Err(bad(p.id));
        }
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            let pad = (4 - r.position() % 4) % 4;
            r.skip(pad).map_err(|_| bad(p.id))?;
            out.push(read_string(&mut r).ok_or(bad(p.id))?);
        }
        Ok(out)
    }

    /// `sequence<octet>`.
    pub fn octets(&self, p: &Parameter) -> Result<Vec<u8>, DecodeError> {
        let mut r = self.reader(p);
        let len = r.u32().map_err(|_| bad(p.id))? as usize;
        r.bytes(len).map(<[u8]>::to_vec).map_err(|_| bad(p.id))
    }

    /// `GUID_t`.
    pub fn guid(&self, p: &Parameter) -> Result<Guid, DecodeError> {
        let mut r = self.reader(p);
        let prefix = GuidPrefix::read(&mut r).map_err(|_| bad(p.id))?;
        let entity_id = EntityId::read(&mut r).map_err(|_| bad(p.id))?;
        Ok(Guid::new(prefix, entity_id))
    }

    /// `EntityId_t`.
    pub fn entity_id(&self, p: &Parameter) -> Result<EntityId, DecodeError> {
        EntityId::read(&mut self.reader(p)).map_err(|_| bad(p.id))
    }

    /// `Locator_t`.
    pub fn locator(&self, p: &Parameter) -> Result<Locator, DecodeError> {
        Locator::read(&mut self.reader(p)).map_err(|_| bad(p.id))
    }

    /// `Duration_t`.
    pub fn duration(&self, p: &Parameter) -> Result<Duration, DecodeError> {
        let mut r = self.reader(p);
        let seconds = r.i32().map_err(|_| bad(p.id))?;
        let fraction = r.u32().map_err(|_| bad(p.id))?;
        Ok(Duration { seconds, fraction })
    }

    /// `ProtocolVersion_t`.
    pub fn protocol_version(&self, p: &Parameter) -> Result<ProtocolVersion, DecodeError> {
        ProtocolVersion::read(&mut self.reader(p)).map_err(|_| bad(p.id))
    }

    /// `VendorId_t`.
    pub fn vendor_id(&self, p: &Parameter) -> Result<VendorId, DecodeError> {
        VendorId::read(&mut self.reader(p)).map_err(|_| bad(p.id))
    }
}

/// Builds parameter values in a byte order.
#[derive(Clone, Copy, Debug)]
pub struct PlBuilder {
    endianness: Endianness,
}

impl PlBuilder {
    /// Values in the given byte order.
    pub fn new(endianness: Endianness) -> Self {
        PlBuilder { endianness }
    }

    fn build(&self, id: ParameterId, f: impl FnOnce(&mut Writer<'_>)) -> Parameter {
        let mut value = Vec::new();
        f(&mut Writer::new(&mut value, self.endianness));
        Parameter::new(id, value)
    }

    /// `unsigned long` or `long`.
    pub fn u32(&self, id: ParameterId, v: u32) -> Parameter {
        self.build(id, |w| w.u32(v))
    }

    /// `boolean`.
    pub fn bool(&self, id: ParameterId, v: bool) -> Parameter {
        Parameter::new(id, [u8::from(v)])
    }

    /// `string`.
    pub fn string(&self, id: ParameterId, v: &str) -> Parameter {
        self.build(id, |w| write_string(w, v))
    }

    /// `sequence<string>`.
    pub fn strings(&self, id: ParameterId, v: &[String]) -> Parameter {
        self.build(id, |w| {
            w.u32(v.len() as u32);
            for s in v {
                w.align4();
                write_string(w, s);
            }
        })
    }

    /// `sequence<octet>`.
    pub fn octets(&self, id: ParameterId, v: &[u8]) -> Parameter {
        self.build(id, |w| {
            w.u32(v.len() as u32);
            w.bytes(v);
        })
    }

    /// `GUID_t`.
    pub fn guid(&self, id: ParameterId, v: Guid) -> Parameter {
        Parameter::new(id, v.to_bytes())
    }

    /// `EntityId_t`.
    pub fn entity_id(&self, id: ParameterId, v: EntityId) -> Parameter {
        Parameter::new(id, v.to_bytes())
    }

    /// `Locator_t`.
    pub fn locator(&self, id: ParameterId, v: &Locator) -> Parameter {
        self.build(id, |w| v.write(w))
    }

    /// `Duration_t`.
    pub fn duration(&self, id: ParameterId, v: Duration) -> Parameter {
        self.build(id, |w| {
            w.i32(v.seconds);
            w.u32(v.fraction);
        })
    }

    /// `ProtocolVersion_t`.
    pub fn protocol_version(&self, id: ParameterId, v: ProtocolVersion) -> Parameter {
        Parameter::new(id, [v.major, v.minor])
    }

    /// `VendorId_t`.
    pub fn vendor_id(&self, id: ParameterId, v: VendorId) -> Parameter {
        Parameter::new(id, v.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The example of 10.6, built with this module.
    #[test]
    fn example_10_6() {
        let b = PlBuilder::new(Endianness::Little);
        let guid = Guid::new(
            GuidPrefix([0xc0, 0xa8, 0x02, 0x05, 0, 0, 0x3a, 0x20, 0, 0, 0, 0x02]),
            EntityId::from_bytes([0x80, 0, 0, 0x07]),
        );
        let mut list = ParameterList::new();
        list.push(b.guid(pid::ENDPOINT_GUID, guid));
        list.push(b.string(pid::TOPIC_NAME, "Square"));
        list.push(b.string(pid::TYPE_NAME, "ShapeType"));
        list.push(b.u32(pid::DESTINATION_ORDER, 1));
        list.push(b.duration(pid::DEADLINE, Duration::from_secs(3)));
        let bytes = encode_pl_payload(&list, Endianness::Little);
        let want: [u8; 84] = [
            0x00, 0x03, 0x00, 0x00, // header
            0x5a, 0x00, 0x10, 0x00, 0xc0, 0xa8, 0x02, 0x05, 0x00, 0x00, 0x3a, 0x20, 0x00, 0x00,
            0x00, 0x02, 0x80, 0x00, 0x00, 0x07, // PID_ENDPOINT_GUID
            0x05, 0x00, 0x0c, 0x00, 0x07, 0x00, 0x00, 0x00, b'S', b'q', b'u', b'a', b'r', b'e',
            0x00, 0x00, // PID_TOPIC_NAME (padding written as zero)
            0x07, 0x00, 0x10, 0x00, 0x0a, 0x00, 0x00, 0x00, b'S', b'h', b'a', b'p', b'e', b'T',
            b'y', b'p', b'e', 0x00, 0x00, 0x00, // PID_TYPE_NAME
            0x25, 0x00, 0x04, 0x00, 0x01, 0x00, 0x00, 0x00, // PID_DESTINATION_ORDER
            0x23, 0x00, 0x08, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            0x00, // DEADLINE
            0x01, 0x00, 0x00, 0x00, // PID_SENTINEL
        ];
        assert_eq!(bytes, want);

        let (back, e) = decode_pl_payload(&bytes).unwrap();
        assert_eq!(e, Endianness::Little);
        let v = PlValues::new(e);
        assert_eq!(v.guid(back.get(pid::ENDPOINT_GUID).unwrap()).unwrap(), guid);
        assert_eq!(
            v.string(back.get(pid::TOPIC_NAME).unwrap()).unwrap(),
            "Square"
        );
        assert_eq!(
            v.string(back.get(pid::TYPE_NAME).unwrap()).unwrap(),
            "ShapeType"
        );
        assert_eq!(
            v.duration(back.get(pid::DEADLINE).unwrap()).unwrap(),
            Duration::from_secs(3)
        );
    }

    #[test]
    fn values_round_trip_big_endian() {
        let b = PlBuilder::new(Endianness::Big);
        let v = PlValues::new(Endianness::Big);
        let loc = Locator::udpv4("10.1.2.3:7410".parse().unwrap());
        assert_eq!(
            v.locator(&b.locator(pid::UNICAST_LOCATOR, &loc)).unwrap(),
            loc
        );
        assert_eq!(
            v.u32(&b.u32(pid::DOMAIN_ID, 0x1234_5678)).unwrap(),
            0x1234_5678
        );
        assert!(v.bool(&b.bool(pid::EXPECTS_INLINE_QOS, true)).unwrap());
        assert_eq!(
            v.octets(&b.octets(pid::USER_DATA, &[1, 2, 3])).unwrap(),
            [1, 2, 3]
        );
        assert_eq!(v.string(&b.string(pid::DOMAIN_TAG, "")).unwrap(), "");
        let parts = vec!["a".to_string(), "".to_string(), "xyz12".to_string()];
        let p = b.strings(pid::PARTITION, &parts);
        // count, "a\0" + pad, "\0" + pad, "xyz12\0".
        assert_eq!(p.value.len(), 4 + 8 + 8 + 10);
        assert_eq!(v.strings(&p).unwrap(), parts);
        assert!(
            v.strings(&Parameter::new(pid::PARTITION, [0, 0, 0, 9]))
                .is_err()
        );
        let d = Duration {
            seconds: -2,
            fraction: 7,
        };
        assert_eq!(v.duration(&b.duration(pid::DEADLINE, d)).unwrap(), d);
    }

    #[test]
    fn bad_values() {
        let v = PlValues::new(Endianness::Little);
        let short = Parameter::new(pid::DOMAIN_ID, [1, 2]);
        assert_eq!(
            v.u32(&short),
            Err(DecodeError::InvalidParameterValue(pid::DOMAIN_ID))
        );
        // String without its NUL.
        let s = Parameter::new(pid::TOPIC_NAME, [2, 0, 0, 0, b'a', b'b', 0, 0]);
        assert!(v.string(&s).is_err());
        // String longer than the value.
        let s = Parameter::new(pid::TOPIC_NAME, [9, 0, 0, 0, b'a', 0, 0, 0]);
        assert!(v.string(&s).is_err());
        assert!(matches!(
            decode_pl_payload(&[0x00, 0x01, 0, 0, 1, 0, 0, 0]),
            Err(DecodeError::UnknownRepresentation(_))
        ));
    }
}
