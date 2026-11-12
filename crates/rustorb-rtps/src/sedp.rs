//! The Simple Endpoint Discovery Protocol (DDSI-RTPS 2.5, 8.5.4, 8.5.5
//! and 9.6.3).
//!
//! [`EndpointData`] is `DiscoveredWriterData` and `DiscoveredReaderData`
//! and their PL_CDR form. [`Sedp`] holds the four built-in SEDP endpoints
//! of a participant (publications and subscriptions, each a writer and a
//! reader), without doing any I/O. They are reliable, using
//! [`crate::reliable`]. The optional topic endpoints (8.5.4.3) are not
//! provided.
//!
//! [`Sedp`] also matches local and remote endpoints: the same topic name,
//! consistent types (DDS-XTypes 1.3, 7.6.3.4: the reader's type
//! assignable from the writer's when both announce `PID_TYPE_INFORMATION`,
//! else the same type name), compatible RELIABILITY, DURABILITY, DEADLINE,
//! LIVELINESS and DATA_REPRESENTATION, and a shared partition
//! ([`crate::qos`]). A pair that would match but for its QoS is reported as
//! [`SedpEvent::IncompatibleQos`], one whose types are not consistent as
//! [`SedpEvent::InconsistentTopic`]. Assignability is decided on the
//! TypeObjects in the [`TypeRegistry`] ([`Sedp::registry_mut`]); a pair
//! whose types are not all there waits, unmatched, until they are
//! ([`Sedp::unresolved_types`]: what to ask for with
//! [`crate::typelookup`]).
//!
//! The caller connects it to SPDP: [`Sedp::add_participant`] when a
//! participant is discovered or updated, [`Sedp::remove_participant`]
//! when it is gone, and adds [`Sedp::BUILTIN_ENDPOINTS`] to the local
//! `availableBuiltinEndpoints`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use crate::error::DecodeError;
use crate::parameter::{Parameter, ParameterList, SecurityParameters};
use crate::pl::{PlBuilder, PlValues, decode_pl_payload, encode_pl_payload, pid};
use crate::qos::{
    DurabilityKind, Liveliness, QosPolicyId, Reliability, RxO, incompatible_policies,
    partitions_match,
};
use crate::receiver::receive;
use crate::reliable::{
    Change, Outgoing, ReaderConfig, ReliableReader, ReliableWriter, WriterConfig,
};
use crate::spdp::ParticipantData;
use crate::submessage::{PayloadKind, SerializedPayload, Submessage};
use crate::types::{
    BuiltinEndpointSet, Duration, EntityId, Guid, GuidPrefix, Locator, SequenceNumber, VendorId,
};
use crate::wire::Endianness;

use rustorb_cdr::{ByteOrder, Decoder, Encoding};
use rustorb_xtypes::assignability::{
    Consistency, EndpointType, TypeConsistency, TypeConsistencyKind, check_consistency,
};
use rustorb_xtypes::registry::TypeRegistry;
use rustorb_xtypes::type_object::{TypeIdentifier, TypeInformation};
use rustorb_xtypes::xcdr2::Xcdr2;

/// Data representation identifiers (DDS-XTypes 1.3, 7.6.3.1.1).
pub mod data_representation {
    /// `XCDR_DATA_REPRESENTATION`: Extended CDR, encoding version 1.
    pub const XCDR: i16 = 0;
    /// `XML_DATA_REPRESENTATION`.
    pub const XML: i16 = 1;
    /// `XCDR2_DATA_REPRESENTATION`: Extended CDR, encoding version 2.
    pub const XCDR2: i16 = 2;
}

/// `DiscoveredWriterData` or `DiscoveredReaderData` (8.5.4.4, 9.6.3.2):
/// the parts of `PublicationBuiltinTopicData` or
/// `SubscriptionBuiltinTopicData` modelled here, with the writer or
/// reader proxy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointData {
    /// `key`, the endpoint's GUID (`PID_ENDPOINT_GUID`).
    pub guid: Guid,
    /// `participant_key` (`PID_PARTICIPANT_GUID`).
    pub participant_guid: Guid,
    /// `topic_name`.
    pub topic_name: String,
    /// `type_name`.
    pub type_name: String,
    /// RELIABILITY.
    pub reliability: Reliability,
    /// DURABILITY.
    pub durability: DurabilityKind,
    /// DEADLINE period; infinite by default.
    pub deadline: Duration,
    /// LIVELINESS.
    pub liveliness: Liveliness,
    /// PARTITION; empty means the default partition.
    pub partitions: Vec<String>,
    /// USER_DATA.
    pub user_data: Vec<u8>,
    /// `unicastLocatorList`. When both lists are empty, the participant's
    /// default locators apply.
    pub unicast_locators: Vec<Locator>,
    /// `multicastLocatorList`.
    pub multicast_locators: Vec<Locator>,
    /// `expectsInlineQos` (readers only).
    pub expects_inline_qos: bool,
    /// `type_information` (DDS-XTypes 1.3, 7.6.3.2.2), when announced.
    pub type_information: Option<TypeInformation>,
    /// DATA_REPRESENTATION ([`data_representation`]): a writer uses and
    /// offers the first, a reader accepts any. Empty means XCDR only.
    pub data_representation: Vec<i16>,
    /// TYPE_CONSISTENCY_ENFORCEMENT (readers only), when announced.
    pub type_consistency: Option<TypeConsistency>,
    /// The content filter of a reader of a content filtered topic
    /// (`PID_CONTENT_FILTER_PROPERTY`, 8.7.3.1), for writer side
    /// filtering.
    pub content_filter: Option<ContentFilterProperty>,
    /// `related_datareader_key` of a writer, `related_datawriter_key` of
    /// a reader (`PID_RELATED_ENTITY_GUID`, DDS-RPC 1.0, 7.6.2.1): the
    /// other endpoint of the same client or service.
    pub related_entity_guid: Option<Guid>,
    /// `service_instance_name` (`PID_SERVICE_INSTANCE_NAME`, DDS-RPC 1.0,
    /// 7.6.2.1), of the endpoints of a service.
    pub service_instance_name: Option<String>,
    /// The parameters of DDS Security, such as the endpoint security
    /// info.
    pub security: SecurityParameters,
}

/// `ContentFilterProperty_t` (DDSI-RTPS 2.5, 8.7.3.1, Table 8.86): the
/// filter of a reader of a content filtered topic.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ContentFilterProperty {
    /// The name of the content filtered topic.
    pub content_filtered_topic_name: String,
    /// The name of its related topic.
    pub related_topic_name: String,
    /// The filter class: `DDSSQL` for the filters of DDS 1.4, Annex B.
    pub filter_class_name: String,
    /// The filter expression.
    pub filter_expression: String,
    /// The expression parameters.
    pub expression_parameters: Vec<String>,
}

impl ContentFilterProperty {
    /// The filter signature (9.6.4.1): the MD5 of the strings, each with
    /// its NUL.
    pub fn signature(&self) -> [u8; 16] {
        let mut bytes = Vec::new();
        for s in [
            &self.content_filtered_topic_name,
            &self.related_topic_name,
            &self.filter_class_name,
            &self.filter_expression,
        ]
        .into_iter()
        .chain(&self.expression_parameters)
        {
            bytes.extend_from_slice(s.as_bytes());
            bytes.push(0);
        }
        rustorb_typesupport::md5(&bytes)
    }
}

impl EndpointData {
    /// A writer with the DDS defaults: RELIABLE, VOLATILE.
    pub fn writer(guid: Guid, topic_name: &str, type_name: &str) -> Self {
        Self::new(guid, topic_name, type_name, Reliability::writer_default())
    }

    /// A reader with the DDS defaults: BEST_EFFORT, VOLATILE.
    pub fn reader(guid: Guid, topic_name: &str, type_name: &str) -> Self {
        Self::new(guid, topic_name, type_name, Reliability::reader_default())
    }

    fn new(guid: Guid, topic_name: &str, type_name: &str, reliability: Reliability) -> Self {
        EndpointData {
            guid,
            participant_guid: Guid::new(guid.prefix, EntityId::PARTICIPANT),
            topic_name: topic_name.to_owned(),
            type_name: type_name.to_owned(),
            reliability,
            durability: DurabilityKind::Volatile,
            deadline: Duration::INFINITE,
            liveliness: Liveliness::default(),
            partitions: Vec::new(),
            user_data: Vec::new(),
            unicast_locators: Vec::new(),
            multicast_locators: Vec::new(),
            expects_inline_qos: false,
            type_information: None,
            data_representation: Vec::new(),
            type_consistency: None,
            content_filter: None,
            related_entity_guid: None,
            service_instance_name: None,
            security: SecurityParameters::default(),
        }
    }

    /// The data representation a writer uses: the first it offers, XCDR
    /// when it offers none.
    pub fn offered_representation(&self) -> i16 {
        self.data_representation
            .first()
            .copied()
            .unwrap_or(data_representation::XCDR)
    }

    /// Whether a reader accepts the data representation `id`.
    pub fn accepts_representation(&self, id: i16) -> bool {
        if self.data_representation.is_empty() {
            id == data_representation::XCDR
        } else {
            self.data_representation.contains(&id)
        }
    }

    /// The reader's type consistency enforcement as matching applies it:
    /// what it announced; when it announced none, the default if it
    /// announced its type information (an implementation of DDS-XTypes
    /// 1.2 or later, such as Cyclone DDS 0.10, which leaves the policy out
    /// when it has its default value), and otherwise
    /// [`TypeConsistency::NOT_ANNOUNCED`] (7.6.3.4.1).
    pub fn effective_type_consistency(&self) -> TypeConsistency {
        match (&self.type_consistency, &self.type_information) {
            (Some(c), _) => *c,
            (None, Some(_)) => TypeConsistency::default(),
            (None, None) => TypeConsistency::NOT_ANNOUNCED,
        }
    }

    fn endpoint_type(&self) -> EndpointType<'_> {
        EndpointType {
            name: &self.type_name,
            information: self.type_information.as_ref(),
        }
    }

    /// The parameter list. RELIABILITY and DURABILITY are always written;
    /// other values equal to their defaults are left out, as is the
    /// separate proxy GUID (Table 9.17).
    pub fn to_parameter_list(&self, endianness: Endianness) -> ParameterList {
        let b = PlBuilder::new(endianness);
        let mut l = ParameterList::new();
        l.push(b.guid(pid::ENDPOINT_GUID, self.guid));
        l.push(b.guid(pid::PARTICIPANT_GUID, self.participant_guid));
        l.push(b.string(pid::TOPIC_NAME, &self.topic_name));
        l.push(b.string(pid::TYPE_NAME, &self.type_name));
        l.push(self.reliability.to_parameter(&b));
        l.push(self.durability.to_parameter(&b));
        if self.deadline != Duration::INFINITE {
            l.push(b.duration(pid::DEADLINE, self.deadline));
        }
        if self.liveliness != Liveliness::default() {
            l.push(self.liveliness.to_parameter(&b));
        }
        if !self.partitions.is_empty() {
            l.push(b.strings(pid::PARTITION, &self.partitions));
        }
        if !self.user_data.is_empty() {
            l.push(b.octets(pid::USER_DATA, &self.user_data));
        }
        for loc in &self.unicast_locators {
            l.push(b.locator(pid::UNICAST_LOCATOR, loc));
        }
        for loc in &self.multicast_locators {
            l.push(b.locator(pid::MULTICAST_LOCATOR, loc));
        }
        if self.expects_inline_qos {
            l.push(b.bool(pid::EXPECTS_INLINE_QOS, true));
        }
        let order = byte_order(endianness);
        if !self.data_representation.is_empty() {
            let mut v = Vec::new();
            put_u32(&mut v, self.data_representation.len() as u32, order);
            for id in &self.data_representation {
                put_u16(&mut v, *id as u16, order);
            }
            v.resize(v.len().next_multiple_of(4), 0);
            l.push(Parameter::new(pid::DATA_REPRESENTATION, v));
        }
        if let Some(c) = &self.type_consistency {
            let mut v = Vec::new();
            let kind = match c.kind {
                TypeConsistencyKind::DisallowTypeCoercion => 0,
                TypeConsistencyKind::AllowTypeCoercion => 1,
            };
            put_u16(&mut v, kind, order);
            v.extend(
                [
                    c.ignore_sequence_bounds,
                    c.ignore_string_bounds,
                    c.ignore_member_names,
                    c.prevent_type_widening,
                    c.force_type_validation,
                ]
                .map(u8::from),
            );
            v.resize(8, 0);
            l.push(Parameter::new(pid::TYPE_CONSISTENCY_ENFORCEMENT, v));
        }
        if let Some(f) = &self.content_filter {
            l.push(b.content_filter(pid::CONTENT_FILTER_PROPERTY, f));
        }
        if let Some(g) = self.related_entity_guid {
            l.push(b.guid(pid::RELATED_ENTITY_GUID, g));
        }
        if let Some(n) = &self.service_instance_name {
            l.push(b.string(pid::SERVICE_INSTANCE_NAME, n));
        }
        if let Some(info) = &self.type_information {
            // An XCDR2 stream of its own, as Cyclone DDS 0.10 writes it.
            if let Ok(mut v) = info.to_bytes(order) {
                v.resize(v.len().next_multiple_of(4), 0);
                l.push(Parameter::new(pid::TYPE_INFORMATION, v));
            }
        }
        for p in self.security.of(endianness) {
            l.push(p.clone());
        }
        l
    }

    /// The serialized payload: `PL_CDR_*` header and parameter list.
    pub fn to_payload(&self, endianness: Endianness) -> Vec<u8> {
        encode_pl_payload(&self.to_parameter_list(endianness), endianness)
    }

    /// Reads the data from a parameter list. `writer` selects the
    /// RELIABILITY default when the parameter is absent (RELIABLE for
    /// writers, BEST_EFFORT for readers).
    ///
    /// `PID_ENDPOINT_GUID`, `PID_TOPIC_NAME` and `PID_TYPE_NAME` are
    /// required. The participant GUID defaults to the endpoint's prefix.
    /// Unknown parameters are skipped, except protocol-defined ones with
    /// the must-understand bit (Table 9.6).
    pub fn from_parameter_list(
        list: &ParameterList,
        endianness: Endianness,
        writer: bool,
    ) -> Result<Self, DecodeError> {
        let v = PlValues::new(endianness);
        let mut guid = None;
        let mut participant = None;
        let mut topic = None;
        let mut type_name = None;
        let mut d = EndpointData::new(
            Guid::UNKNOWN,
            "",
            "",
            if writer {
                Reliability::writer_default()
            } else {
                Reliability::reader_default()
            },
        );
        for p in &list.parameters {
            match p.id {
                pid::ENDPOINT_GUID => guid = Some(v.guid(p)?),
                pid::PARTICIPANT_GUID => participant = Some(v.guid(p)?),
                pid::TOPIC_NAME => topic = Some(v.string(p)?),
                pid::TYPE_NAME => type_name = Some(v.string(p)?),
                pid::RELIABILITY => d.reliability = Reliability::from_parameter(&v, p)?,
                pid::DURABILITY => d.durability = DurabilityKind::from_parameter(&v, p)?,
                pid::DEADLINE => d.deadline = v.duration(p)?,
                pid::LIVELINESS => d.liveliness = Liveliness::from_parameter(&v, p)?,
                pid::PARTITION => d.partitions = v.strings(p)?,
                pid::USER_DATA => d.user_data = v.octets(p)?,
                pid::UNICAST_LOCATOR => d.unicast_locators.push(v.locator(p)?),
                pid::MULTICAST_LOCATOR => d.multicast_locators.push(v.locator(p)?),
                pid::EXPECTS_INLINE_QOS => d.expects_inline_qos = v.bool(p)?,
                // The XTypes parameters are optional: one that cannot be
                // read is left out, as though not sent.
                pid::DATA_REPRESENTATION => {
                    d.data_representation =
                        read_representations(&p.value, endianness).unwrap_or_default()
                }
                pid::TYPE_CONSISTENCY_ENFORCEMENT => {
                    d.type_consistency = read_type_consistency(&p.value, endianness)
                }
                pid::TYPE_INFORMATION => {
                    d.type_information = read_type_information(&p.value, endianness)
                }
                // Writer side filtering is optional (8.7.3.4): a filter
                // that cannot be read is left out.
                pid::CONTENT_FILTER_PROPERTY => d.content_filter = v.content_filter(p).ok(),
                // DDS-RPC's extensions are optional too.
                pid::RELATED_ENTITY_GUID => d.related_entity_guid = v.guid(p).ok(),
                pid::SERVICE_INSTANCE_NAME => d.service_instance_name = v.string(p).ok(),
                id if SecurityParameters::is_security(id) => d.security.push(p.clone(), endianness),
                id if !id.is_vendor_specific() && id.must_understand() => {
                    return Err(DecodeError::MustUnderstand(id));
                }
                _ => {}
            }
        }
        d.guid = guid.ok_or(DecodeError::MissingParameter(pid::ENDPOINT_GUID))?;
        d.participant_guid = participant.unwrap_or(Guid::new(d.guid.prefix, EntityId::PARTICIPANT));
        d.topic_name = topic.ok_or(DecodeError::MissingParameter(pid::TOPIC_NAME))?;
        d.type_name = type_name.ok_or(DecodeError::MissingParameter(pid::TYPE_NAME))?;
        Ok(d)
    }

    /// Reads the data from a serialized payload.
    pub fn from_payload(bytes: &[u8], writer: bool) -> Result<Self, DecodeError> {
        let (list, e) = decode_pl_payload(bytes)?;
        Self::from_parameter_list(&list, e, writer)
    }

    /// The policies that a writer offers and a reader requests.
    pub fn rxo(&self) -> RxO {
        RxO {
            reliability: self.reliability.kind,
            durability: self.durability,
            deadline: self.deadline,
            liveliness: self.liveliness,
        }
    }
}

/// `DATA_REPRESENTATION_QOS_POLICY_ID` (DDS-XTypes 1.3, Annex D).
pub const DATA_REPRESENTATION_QOS_POLICY_ID: QosPolicyId = QosPolicyId(23);

/// How a writer and a reader stand towards each other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Match {
    /// They match.
    Matched,
    /// Same topic, consistent types and a shared partition, but the QoS
    /// the writer offers does not satisfy the reader for these policies.
    Incompatible(Vec<QosPolicyId>),
    /// Same topic, but types that are not consistent (DDS-XTypes 1.3,
    /// 7.6.3.4.2): an INCONSISTENT_TOPIC.
    Inconsistent,
    /// Same topic, but the TypeObjects of these types are needed to tell
    /// whether the types are consistent.
    Unresolved(Vec<TypeIdentifier>),
    /// Different topics or partitions: nothing to report.
    Unrelated,
}

/// Whether a writer and a reader match, and if not, why (DDS 1.4,
/// 2.2.3; DDS-XTypes 1.3, 7.6.3.4.2). `registry` holds the TypeObjects
/// known, for when the types differ.
pub fn match_endpoints(
    writer: &EndpointData,
    reader: &EndpointData,
    registry: &TypeRegistry,
) -> Match {
    if writer.topic_name != reader.topic_name {
        return Match::Unrelated;
    }
    match check_consistency(
        writer.endpoint_type(),
        reader.endpoint_type(),
        &reader.effective_type_consistency(),
        registry,
    ) {
        Consistency::Consistent => {}
        Consistency::Inconsistent => return Match::Inconsistent,
        Consistency::Unknown(missing) => return Match::Unresolved(missing),
    }
    if !partitions_match(&writer.partitions, &reader.partitions) {
        return Match::Unrelated;
    }
    let mut bad = incompatible_policies(&writer.rxo(), &reader.rxo());
    if !reader.accepts_representation(writer.offered_representation()) {
        bad.push(DATA_REPRESENTATION_QOS_POLICY_ID);
    }
    if bad.is_empty() {
        Match::Matched
    } else {
        Match::Incompatible(bad)
    }
}

/// Whether a writer and a reader match, with no TypeObjects known: the
/// same topic, the same type (by name, or by TypeIdentifier when both
/// announce one), compatible QoS, and a shared partition.
pub fn endpoints_match(writer: &EndpointData, reader: &EndpointData) -> bool {
    match_endpoints(writer, reader, &TypeRegistry::new()) == Match::Matched
}

fn byte_order(e: Endianness) -> ByteOrder {
    match e {
        Endianness::Big => ByteOrder::Big,
        Endianness::Little => ByteOrder::Little,
    }
}

fn put_u16(v: &mut Vec<u8>, x: u16, order: ByteOrder) {
    v.extend(match order {
        ByteOrder::Big => x.to_be_bytes(),
        ByteOrder::Little => x.to_le_bytes(),
    });
}

fn put_u32(v: &mut Vec<u8>, x: u32, order: ByteOrder) {
    v.extend(match order {
        ByteOrder::Big => x.to_be_bytes(),
        ByteOrder::Little => x.to_le_bytes(),
    });
}

/// `DataRepresentationQosPolicy`: a `sequence<short>` (in PL_CDR, an
/// APPENDABLE struct has no header).
fn read_representations(value: &[u8], e: Endianness) -> Option<Vec<i16>> {
    let mut dec = Decoder::new(value, byte_order(e), Encoding::Xcdr1);
    let n = dec.read_u32().ok()? as usize;
    if n > value.len() / 2 {
        return None;
    }
    (0..n).map(|_| dec.read_i16().ok()).collect()
}

/// `TypeConsistencyEnforcementQosPolicy`: the kind (16 bits) and five
/// booleans. Booleans that are not there keep their defaults.
fn read_type_consistency(value: &[u8], e: Endianness) -> Option<TypeConsistency> {
    let mut dec = Decoder::new(value, byte_order(e), Encoding::Xcdr1);
    let kind = match dec.read_u16().ok()? {
        0 => TypeConsistencyKind::DisallowTypeCoercion,
        1 => TypeConsistencyKind::AllowTypeCoercion,
        _ => return None,
    };
    let mut c = TypeConsistency {
        kind,
        ..TypeConsistency::default()
    };
    for field in [
        &mut c.ignore_sequence_bounds,
        &mut c.ignore_string_bounds,
        &mut c.ignore_member_names,
        &mut c.prevent_type_widening,
        &mut c.force_type_validation,
    ] {
        match dec.read_octet() {
            Ok(b) => *field = b != 0,
            Err(_) => break,
        }
    }
    Some(c)
}

/// `TypeInformation`: XCDR2, followed by up to 3 octets of padding.
fn read_type_information(value: &[u8], e: Endianness) -> Option<TypeInformation> {
    let mut dec = Decoder::new(value, byte_order(e), Encoding::Xcdr2);
    let info = TypeInformation::decode(&mut dec).ok()?;
    (dec.remaining() < 4).then_some(info)
}

/// Settings of [`Sedp`]: the timing of its reliable endpoints.
#[derive(Clone, Debug, Default)]
pub struct SedpConfig {
    /// For the built-in writers.
    pub writer: WriterConfig,
    /// For the built-in readers.
    pub reader: ReaderConfig,
}

/// A change in the remote endpoints or in the matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SedpEvent {
    /// A remote writer was seen for the first time.
    WriterDiscovered(EndpointData),
    /// A known remote writer announced different data.
    WriterUpdated(EndpointData),
    /// A remote writer is gone.
    WriterRemoved(Guid),
    /// A remote reader was seen for the first time.
    ReaderDiscovered(EndpointData),
    /// A known remote reader announced different data.
    ReaderUpdated(EndpointData),
    /// A remote reader is gone.
    ReaderRemoved(Guid),
    /// A local endpoint now matches a remote one.
    Matched {
        /// The local writer or reader.
        local: Guid,
        /// The remote reader or writer.
        remote: Guid,
    },
    /// A local endpoint no longer matches a remote one.
    Unmatched {
        /// The local writer or reader.
        local: Guid,
        /// The remote reader or writer.
        remote: Guid,
    },
    /// A local endpoint and a remote one are on the same topic with types
    /// that are not consistent (reported when the pair becomes so).
    InconsistentTopic {
        /// The local writer or reader.
        local: Guid,
        /// The remote reader or writer.
        remote: Guid,
    },
    /// A local endpoint and a remote one would match but for their QoS
    /// (reported when the pair becomes incompatible).
    IncompatibleQos {
        /// The local writer or reader.
        local: Guid,
        /// The remote reader or writer.
        remote: Guid,
        /// The policies that differ.
        policies: Vec<QosPolicyId>,
    },
}

/// `StatusInfo_t` flags (9.6.4.9).
const STATUS_DISPOSED: u8 = 0x01;
const STATUS_UNREGISTERED: u8 = 0x02;

#[derive(Debug)]
struct Local {
    data: EndpointData,
    sn: SequenceNumber,
    /// Announced on the secure endpoints.
    secure: bool,
}

/// The four SEDP endpoints, plain or secure.
#[derive(Debug)]
struct Endpoints {
    secure: bool,
    pub_writer: ReliableWriter,
    pub_reader: ReliableReader,
    sub_writer: ReliableWriter,
    sub_reader: ReliableReader,
}

impl Endpoints {
    /// The entity ids: publications writer and reader, subscriptions
    /// writer and reader.
    fn ids(secure: bool) -> [EntityId; 4] {
        if secure {
            [
                EntityId::SEDP_BUILTIN_PUBLICATIONS_SECURE_WRITER,
                EntityId::SEDP_BUILTIN_PUBLICATIONS_SECURE_READER,
                EntityId::SEDP_BUILTIN_SUBSCRIPTIONS_SECURE_WRITER,
                EntityId::SEDP_BUILTIN_SUBSCRIPTIONS_SECURE_READER,
            ]
        } else {
            [
                EntityId::SEDP_BUILTIN_PUBLICATIONS_ANNOUNCER,
                EntityId::SEDP_BUILTIN_PUBLICATIONS_DETECTOR,
                EntityId::SEDP_BUILTIN_SUBSCRIPTIONS_ANNOUNCER,
                EntityId::SEDP_BUILTIN_SUBSCRIPTIONS_DETECTOR,
            ]
        }
    }

    /// The `availableBuiltinEndpoints` bits, in the order of `ids`.
    fn bits(secure: bool) -> [u32; 4] {
        if secure {
            [
                BuiltinEndpointSet::PUBLICATIONS_SECURE_WRITER,
                BuiltinEndpointSet::PUBLICATIONS_SECURE_READER,
                BuiltinEndpointSet::SUBSCRIPTIONS_SECURE_WRITER,
                BuiltinEndpointSet::SUBSCRIPTIONS_SECURE_READER,
            ]
        } else {
            [
                BuiltinEndpointSet::PUBLICATIONS_ANNOUNCER,
                BuiltinEndpointSet::PUBLICATIONS_DETECTOR,
                BuiltinEndpointSet::SUBSCRIPTIONS_ANNOUNCER,
                BuiltinEndpointSet::SUBSCRIPTIONS_DETECTOR,
            ]
        }
    }

    fn new(prefix: GuidPrefix, vendor_id: VendorId, config: &SedpConfig, secure: bool) -> Self {
        let [pw, pr, sw, sr] = Self::ids(secure);
        let g = |e| Guid::new(prefix, e);
        Endpoints {
            secure,
            pub_writer: ReliableWriter::new(g(pw), vendor_id, config.writer.clone()),
            pub_reader: ReliableReader::new(g(pr), vendor_id, config.reader.clone()),
            sub_writer: ReliableWriter::new(g(sw), vendor_id, config.writer.clone()),
            sub_reader: ReliableReader::new(g(sr), vendor_id, config.reader.clone()),
        }
    }

    /// The set a submessage of the SEDP writer `writer_id` belongs to,
    /// and the kind of endpoints the writer announces.
    fn find<'a>(
        plain: &'a mut Endpoints,
        secure: Option<&'a mut Endpoints>,
        writer_id: EntityId,
    ) -> Option<(&'a mut Endpoints, Kind)> {
        let set = if Self::ids(false).contains(&writer_id) {
            plain
        } else if Self::ids(true).contains(&writer_id) {
            secure?
        } else {
            return None;
        };
        let [pw, _, sw, _] = Self::ids(set.secure);
        if writer_id == pw {
            Some((set, Kind::Writer))
        } else if writer_id == sw {
            Some((set, Kind::Reader))
        } else {
            None
        }
    }

    fn writer(&mut self, kind: Kind) -> &mut ReliableWriter {
        match kind {
            Kind::Writer => &mut self.pub_writer,
            Kind::Reader => &mut self.sub_writer,
        }
    }

    fn add_participant(&mut self, p: &ParticipantData, now: Instant) {
        let set = p.available_builtin_endpoints;
        let uc = p.metatraffic_unicast_locators.clone();
        let mc = p.metatraffic_multicast_locators.clone();
        let g = |e| Guid::new(p.guid_prefix, e);
        let [pw, pr, sw, sr] = Self::ids(self.secure);
        let [bpw, bpr, bsw, bsr] = Self::bits(self.secure);
        if set.contains(bpr) {
            self.pub_writer.add_reader(g(pr), uc.clone(), mc.clone());
        }
        if set.contains(bpw) {
            self.pub_reader
                .add_writer(g(pw), uc.clone(), mc.clone(), now);
        }
        if set.contains(bsr) {
            self.sub_writer.add_reader(g(sr), uc.clone(), mc.clone());
        }
        if set.contains(bsw) {
            self.sub_reader.add_writer(g(sw), uc, mc, now);
        }
    }

    fn remove_participant(&mut self, prefix: &GuidPrefix) {
        self.pub_writer.remove_readers_of(prefix);
        self.pub_reader.remove_writers_of(prefix);
        self.sub_writer.remove_readers_of(prefix);
        self.sub_reader.remove_writers_of(prefix);
    }

    fn poll(&mut self, now: Instant) -> Vec<Outgoing> {
        let mut out = self.pub_writer.poll(now);
        out.extend(self.sub_writer.poll(now));
        out.extend(self.pub_reader.poll(now));
        out.extend(self.sub_reader.poll(now));
        out
    }

    fn next_deadline(&self) -> Option<Instant> {
        [
            self.pub_writer.next_deadline(),
            self.sub_writer.next_deadline(),
            self.pub_reader.next_deadline(),
            self.sub_reader.next_deadline(),
        ]
        .into_iter()
        .flatten()
        .min()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Writer,
    Reader,
}

/// The SEDP built-in endpoints of one participant, and the endpoint
/// matching.
#[derive(Debug)]
pub struct Sedp {
    prefix: GuidPrefix,
    vendor_id: VendorId,
    config: SedpConfig,
    plain: Endpoints,
    /// The secure endpoints (DDS Security 1.2, 7.4.1.4), when enabled.
    secure: Option<Endpoints>,
    /// Default locators of the remote participants.
    participants: HashMap<GuidPrefix, (Vec<Locator>, Vec<Locator>)>,
    local_writers: BTreeMap<Guid, Local>,
    local_readers: BTreeMap<Guid, Local>,
    remote_writers: HashMap<Guid, EndpointData>,
    remote_readers: HashMap<Guid, EndpointData>,
    /// (local, remote) pairs that match.
    matches: HashSet<(Guid, Guid)>,
    /// (local, remote) pairs that are incompatible, already reported.
    incompatible: HashSet<(Guid, Guid)>,
    /// (local, remote) pairs whose types are inconsistent, already
    /// reported.
    inconsistent: HashSet<(Guid, Guid)>,
    /// (local, remote) pairs waiting for TypeObjects.
    unresolved: HashMap<(Guid, Guid), Vec<TypeIdentifier>>,
    /// The TypeObjects known.
    registry: TypeRegistry,
}

impl Sedp {
    /// The SEDP bits of `availableBuiltinEndpoints` for the endpoints
    /// provided here (9.3.2.12).
    pub const BUILTIN_ENDPOINTS: u32 = BuiltinEndpointSet::PUBLICATIONS_ANNOUNCER
        | BuiltinEndpointSet::PUBLICATIONS_DETECTOR
        | BuiltinEndpointSet::SUBSCRIPTIONS_ANNOUNCER
        | BuiltinEndpointSet::SUBSCRIPTIONS_DETECTOR;

    /// The bits of the secure SEDP endpoints (DDS Security 1.2,
    /// 7.4.7.1), for a participant that enabled them.
    pub const BUILTIN_ENDPOINTS_SECURE: u32 = BuiltinEndpointSet::PUBLICATIONS_SECURE_WRITER
        | BuiltinEndpointSet::PUBLICATIONS_SECURE_READER
        | BuiltinEndpointSet::SUBSCRIPTIONS_SECURE_WRITER
        | BuiltinEndpointSet::SUBSCRIPTIONS_SECURE_READER;

    /// The SEDP endpoints of the participant `prefix`.
    pub fn new(prefix: GuidPrefix, vendor_id: VendorId, config: SedpConfig) -> Self {
        Sedp {
            prefix,
            vendor_id,
            plain: Endpoints::new(prefix, vendor_id, &config, false),
            secure: None,
            config,
            participants: HashMap::new(),
            local_writers: BTreeMap::new(),
            local_readers: BTreeMap::new(),
            remote_writers: HashMap::new(),
            remote_readers: HashMap::new(),
            matches: HashSet::new(),
            incompatible: HashSet::new(),
            inconsistent: HashSet::new(),
            unresolved: HashMap::new(),
            registry: TypeRegistry::new(),
        }
    }

    /// Connects the SEDP endpoints with those of a discovered participant,
    /// as far as its `availableBuiltinEndpoints` says it has them
    /// (8.5.5.1). Calling it again for the same participant updates the
    /// locators. The local participant itself is ignored.
    pub fn add_participant(&mut self, p: &ParticipantData, now: Instant) {
        if p.guid_prefix == self.prefix {
            return;
        }
        self.plain.add_participant(p, now);
        if let Some(s) = self.secure.as_mut() {
            s.add_participant(p, now);
        }
        self.participants.insert(
            p.guid_prefix,
            (
                p.default_unicast_locators.clone(),
                p.default_multicast_locators.clone(),
            ),
        );
    }

    /// Disconnects from a participant that is gone (8.5.5.2) and forgets
    /// its endpoints.
    pub fn remove_participant(&mut self, prefix: &GuidPrefix) -> Vec<SedpEvent> {
        self.plain.remove_participant(prefix);
        if let Some(s) = self.secure.as_mut() {
            s.remove_participant(prefix);
        }
        self.participants.remove(prefix);
        let mut events = Vec::new();
        let writers: Vec<Guid> = self
            .remote_writers
            .keys()
            .filter(|g| g.prefix == *prefix)
            .copied()
            .collect();
        for g in writers {
            self.remove_remote(Kind::Writer, g, &mut events);
        }
        let readers: Vec<Guid> = self
            .remote_readers
            .keys()
            .filter(|g| g.prefix == *prefix)
            .copied()
            .collect();
        for g in readers {
            self.remove_remote(Kind::Reader, g, &mut events);
        }
        events
    }

    /// Announces a local writer, or its new data if it is already
    /// announced, and matches it with the remote readers.
    pub fn add_writer(&mut self, data: EndpointData) -> Vec<SedpEvent> {
        self.add_local(Kind::Writer, data)
    }

    /// Announces a local reader, or its new data, and matches it with the
    /// remote writers.
    pub fn add_reader(&mut self, data: EndpointData) -> Vec<SedpEvent> {
        self.add_local(Kind::Reader, data)
    }

    /// Announces that a local writer is gone.
    pub fn remove_writer(&mut self, guid: &Guid) -> Vec<SedpEvent> {
        self.remove_local(Kind::Writer, guid)
    }

    /// Announces that a local reader is gone.
    pub fn remove_reader(&mut self, guid: &Guid) -> Vec<SedpEvent> {
        self.remove_local(Kind::Reader, guid)
    }

    /// Whether participant `prefix` has acknowledged the latest
    /// announcement of local writer or reader `local`: then it knows the
    /// endpoint's current data.
    pub fn is_announced_to(&self, local: &Guid, prefix: &GuidPrefix) -> bool {
        let (l, writer) = match (self.local_writers.get(local), self.local_readers.get(local)) {
            (Some(l), _) => (l, true),
            (None, Some(l)) => (l, false),
            (None, None) => return false,
        };
        let set = match (l.secure, self.secure.as_ref()) {
            (true, Some(s)) => s,
            _ => &self.plain,
        };
        let w = if writer {
            &set.pub_writer
        } else {
            &set.sub_writer
        };
        w.is_acked_by_participant(prefix, l.sn)
    }

    /// The remote writers known.
    pub fn remote_writers(&self) -> impl Iterator<Item = &EndpointData> {
        self.remote_writers.values()
    }

    /// The remote readers known.
    pub fn remote_readers(&self) -> impl Iterator<Item = &EndpointData> {
        self.remote_readers.values()
    }

    /// Looks up a remote writer.
    pub fn remote_writer(&self, guid: &Guid) -> Option<&EndpointData> {
        self.remote_writers.get(guid)
    }

    /// Looks up a remote reader.
    pub fn remote_reader(&self, guid: &Guid) -> Option<&EndpointData> {
        self.remote_readers.get(guid)
    }

    /// The TypeObjects known.
    pub fn registry(&self) -> &TypeRegistry {
        &self.registry
    }

    /// The TypeObjects known, to add to. Call
    /// [`Sedp::rematch_unresolved`] after adding.
    pub fn registry_mut(&mut self) -> &mut TypeRegistry {
        &mut self.registry
    }

    /// The TypeIdentifiers whose TypeObjects pairs of endpoints wait for,
    /// with a remote endpoint that announced each (the one to ask).
    pub fn unresolved_types(&self) -> Vec<(TypeIdentifier, Guid)> {
        let mut out: Vec<(TypeIdentifier, Guid)> = Vec::new();
        for ((_, remote), ids) in &self.unresolved {
            for id in ids {
                if !out.iter().any(|(i, _)| i == id) {
                    out.push((id.clone(), *remote));
                }
            }
        }
        out
    }

    /// Matches again the pairs that waited for TypeObjects.
    pub fn rematch_unresolved(&mut self) -> Vec<SedpEvent> {
        let mut pairs: Vec<(Guid, Guid)> = self.unresolved.keys().copied().collect();
        pairs.sort();
        let mut events = Vec::new();
        for (local, remote) in pairs {
            let kind = if self.local_writers.contains_key(&local) {
                Kind::Writer
            } else {
                Kind::Reader
            };
            self.update_match(kind, local, remote, &mut events);
        }
        events
    }

    /// The remote endpoints that match a local one.
    pub fn matched(&self, local: &Guid) -> impl Iterator<Item = &Guid> {
        self.matches
            .iter()
            .filter(move |(l, _)| l == local)
            .map(|(_, r)| r)
    }

    /// Processes one received RTPS message and returns what changed.
    pub fn handle_message(&mut self, bytes: &[u8], now: Instant) -> Vec<SedpEvent> {
        let Some((_, received)) = receive(bytes, self.prefix) else {
            return Vec::new();
        };
        let mut changes: Vec<(Kind, Change)> = Vec::new();
        for r in received {
            let src = r.source_prefix;
            let writer_id = match &r.submessage {
                Submessage::Data(d) => d.writer_id,
                Submessage::DataFrag(d) => d.writer_id,
                Submessage::Gap(g) => g.writer_id,
                Submessage::Heartbeat(h) => h.writer_id,
                Submessage::NackFrag(n) => n.writer_id,
                Submessage::AckNack(a) => a.writer_id,
                _ => continue,
            };
            let Some((set, kind)) =
                Endpoints::find(&mut self.plain, self.secure.as_mut(), writer_id)
            else {
                continue;
            };
            let w = Guid::new(src, writer_id);
            let (writer, reader) = match kind {
                Kind::Writer => (&mut set.pub_writer, &mut set.pub_reader),
                Kind::Reader => (&mut set.sub_writer, &mut set.sub_reader),
            };
            let got = match r.submessage {
                Submessage::Data(d) => reader.handle_data(w, &d, r.timestamp),
                Submessage::DataFrag(d) => reader.handle_data_frag(w, &d, r.timestamp),
                Submessage::Gap(g) => reader.handle_gap(w, &g),
                Submessage::Heartbeat(h) => reader.handle_heartbeat(w, &h, now),
                Submessage::NackFrag(n) => {
                    writer.handle_nackfrag(Guid::new(src, n.reader_id), &n, now);
                    Vec::new()
                }
                Submessage::AckNack(a) => {
                    writer.handle_acknack(Guid::new(src, a.reader_id), &a, now);
                    Vec::new()
                }
                _ => Vec::new(),
            };
            changes.extend(got.into_iter().map(|c| (kind, c)));
        }
        let mut events = Vec::new();
        for (kind, c) in changes {
            self.apply(kind, &c, &mut events);
        }
        events
    }

    /// The datagrams due at `now`.
    pub fn poll(&mut self, now: Instant) -> Vec<Outgoing> {
        let mut out = self.plain.poll(now);
        if let Some(s) = self.secure.as_mut() {
            out.extend(s.poll(now));
        }
        out
    }

    /// When [`Sedp::poll`] next has something to do, if no message comes
    /// in before.
    pub fn next_deadline(&self) -> Option<Instant> {
        [
            self.plain.next_deadline(),
            self.secure.as_ref().and_then(Endpoints::next_deadline),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    /// Adds the secure SEDP endpoints (DDS Security 1.2, 7.4.1.4): the
    /// local endpoints added with [`Sedp::add_writer_secure`] and
    /// [`Sedp::add_reader_secure`] are announced on them, and they are
    /// connected with those of the participants added after.
    pub fn enable_secure(&mut self) {
        if self.secure.is_none() {
            self.secure = Some(Endpoints::new(
                self.prefix,
                self.vendor_id,
                &self.config,
                true,
            ));
        }
    }

    /// Like [`Sedp::add_writer`], on the secure endpoints if they are
    /// enabled.
    pub fn add_writer_secure(&mut self, data: EndpointData) -> Vec<SedpEvent> {
        self.add_local_on(Kind::Writer, data, true)
    }

    /// Like [`Sedp::add_reader`], on the secure endpoints if they are
    /// enabled.
    pub fn add_reader_secure(&mut self, data: EndpointData) -> Vec<SedpEvent> {
        self.add_local_on(Kind::Reader, data, true)
    }

    fn set(&mut self, secure: bool) -> &mut Endpoints {
        match (secure, self.secure.as_mut()) {
            (true, Some(s)) => s,
            _ => &mut self.plain,
        }
    }

    fn add_local(&mut self, kind: Kind, data: EndpointData) -> Vec<SedpEvent> {
        self.add_local_on(kind, data, false)
    }

    fn add_local_on(&mut self, kind: Kind, data: EndpointData, secure: bool) -> Vec<SedpEvent> {
        let secure = secure && self.secure.is_some();
        let mut qos = ParameterList::new();
        qos.push(Parameter::new(pid::KEY_HASH, data.guid.to_bytes()));
        let payload = SerializedPayload {
            kind: PayloadKind::Data,
            bytes: data.to_payload(Endianness::NATIVE),
        };
        let old = match kind {
            Kind::Writer => self.local_writers.remove(&data.guid),
            Kind::Reader => self.local_readers.remove(&data.guid),
        };
        // The history keeps one change per endpoint (8.5.4.4).
        if let Some(old) = &old {
            self.set(old.secure).writer(kind).remove_change(old.sn);
        }
        let sn = self
            .set(secure)
            .writer(kind)
            .add_change(Some(qos), Some(payload));
        let local = Local {
            data: data.clone(),
            sn,
            secure,
        };
        match kind {
            Kind::Writer => self.local_writers.insert(data.guid, local),
            Kind::Reader => self.local_readers.insert(data.guid, local),
        };
        let mut events = Vec::new();
        let remotes: Vec<Guid> = match kind {
            Kind::Writer => self.remote_readers.keys().copied().collect(),
            Kind::Reader => self.remote_writers.keys().copied().collect(),
        };
        for r in remotes {
            self.update_match(kind, data.guid, r, &mut events);
        }
        events
    }

    fn remove_local(&mut self, kind: Kind, guid: &Guid) -> Vec<SedpEvent> {
        let old = match kind {
            Kind::Writer => self.local_writers.remove(guid),
            Kind::Reader => self.local_readers.remove(guid),
        };
        let Some(old) = old else {
            return Vec::new();
        };
        self.incompatible.retain(|(l, _)| l != guid);
        self.inconsistent.retain(|(l, _)| l != guid);
        self.unresolved.retain(|(l, _), _| l != guid);
        let writer = self.set(old.secure).writer(kind);
        writer.remove_change(old.sn);
        // A dispose: the key in-line and as a key-only payload (9.6.3.2).
        let e = Endianness::NATIVE;
        let b = PlBuilder::new(e);
        let mut qos = ParameterList::new();
        qos.push(Parameter::new(pid::KEY_HASH, guid.to_bytes()));
        qos.push(Parameter::new(
            pid::STATUS_INFO,
            [0, 0, 0, STATUS_DISPOSED | STATUS_UNREGISTERED],
        ));
        let mut key = ParameterList::new();
        key.push(b.guid(pid::ENDPOINT_GUID, *guid));
        writer.add_change(
            Some(qos),
            Some(SerializedPayload {
                kind: PayloadKind::Key,
                bytes: encode_pl_payload(&key, e),
            }),
        );
        let mut events = Vec::new();
        let pairs: Vec<(Guid, Guid)> = self
            .matches
            .iter()
            .filter(|(l, _)| l == guid)
            .copied()
            .collect();
        for (l, r) in pairs {
            self.matches.remove(&(l, r));
            events.push(SedpEvent::Unmatched {
                local: l,
                remote: r,
            });
        }
        events
    }

    /// Applies a change received from a remote SEDP writer: `kind` is the
    /// kind of the endpoints it describes.
    fn apply(&mut self, kind: Kind, c: &Change, events: &mut Vec<SedpEvent>) {
        let status = c
            .inline_qos
            .as_ref()
            .and_then(|q| q.get(pid::STATUS_INFO))
            .and_then(|p| p.value.get(3).copied())
            .unwrap_or(0);
        if status & (STATUS_DISPOSED | STATUS_UNREGISTERED) != 0 {
            if let Some(g) = disposed_key(c) {
                self.remove_remote(kind, g, events);
            }
            return;
        }
        let Some(p) = c.payload.as_ref().filter(|p| p.kind == PayloadKind::Data) else {
            return;
        };
        let Ok(mut data) = EndpointData::from_payload(&p.bytes, kind == Kind::Writer) else {
            return;
        };
        if data.guid.prefix == self.prefix {
            return;
        }
        if data.unicast_locators.is_empty() && data.multicast_locators.is_empty() {
            if let Some((uc, mc)) = self.participants.get(&data.guid.prefix) {
                data.unicast_locators = uc.clone();
                data.multicast_locators = mc.clone();
            }
        }
        let guid = data.guid;
        let remotes = match kind {
            Kind::Writer => &mut self.remote_writers,
            Kind::Reader => &mut self.remote_readers,
        };
        match remotes.insert(guid, data.clone()) {
            Some(old) if old == data => return,
            Some(_) => events.push(match kind {
                Kind::Writer => SedpEvent::WriterUpdated(data),
                Kind::Reader => SedpEvent::ReaderUpdated(data),
            }),
            None => events.push(match kind {
                Kind::Writer => SedpEvent::WriterDiscovered(data),
                Kind::Reader => SedpEvent::ReaderDiscovered(data),
            }),
        }
        let locals: Vec<Guid> = match kind {
            Kind::Writer => self.local_readers.keys().copied().collect(),
            Kind::Reader => self.local_writers.keys().copied().collect(),
        };
        let local_kind = match kind {
            Kind::Writer => Kind::Reader,
            Kind::Reader => Kind::Writer,
        };
        for l in locals {
            self.update_match(local_kind, l, guid, events);
        }
    }

    fn remove_remote(&mut self, kind: Kind, guid: Guid, events: &mut Vec<SedpEvent>) {
        let removed = match kind {
            Kind::Writer => self.remote_writers.remove(&guid),
            Kind::Reader => self.remote_readers.remove(&guid),
        };
        if removed.is_none() {
            return;
        }
        self.incompatible.retain(|(_, r)| *r != guid);
        self.inconsistent.retain(|(_, r)| *r != guid);
        self.unresolved.retain(|(_, r), _| *r != guid);
        let mut pairs: Vec<(Guid, Guid)> = self
            .matches
            .iter()
            .filter(|(_, r)| *r == guid)
            .copied()
            .collect();
        pairs.sort();
        for (l, r) in pairs {
            self.matches.remove(&(l, r));
            events.push(SedpEvent::Unmatched {
                local: l,
                remote: r,
            });
        }
        events.push(match kind {
            Kind::Writer => SedpEvent::WriterRemoved(guid),
            Kind::Reader => SedpEvent::ReaderRemoved(guid),
        });
    }

    /// Re-evaluates whether the local endpoint `local` (of `kind`) and the
    /// remote endpoint `remote` match, and reports a change.
    fn update_match(&mut self, kind: Kind, local: Guid, remote: Guid, events: &mut Vec<SedpEvent>) {
        let m = match kind {
            Kind::Writer => match (
                self.local_writers.get(&local),
                self.remote_readers.get(&remote),
            ) {
                (Some(w), Some(r)) => match_endpoints(&w.data, r, &self.registry),
                _ => Match::Unrelated,
            },
            Kind::Reader => match (
                self.remote_writers.get(&remote),
                self.local_readers.get(&local),
            ) {
                (Some(w), Some(r)) => match_endpoints(w, &r.data, &self.registry),
                _ => Match::Unrelated,
            },
        };
        let now = m == Match::Matched;
        let pair = (local, remote);
        if !matches!(m, Match::Incompatible(_)) {
            self.incompatible.remove(&pair);
        }
        if m != Match::Inconsistent {
            self.inconsistent.remove(&pair);
        }
        if !matches!(m, Match::Unresolved(_)) {
            self.unresolved.remove(&pair);
        }
        match m {
            Match::Incompatible(policies) => {
                if self.incompatible.insert(pair) {
                    events.push(SedpEvent::IncompatibleQos {
                        local,
                        remote,
                        policies,
                    });
                }
            }
            Match::Inconsistent => {
                if self.inconsistent.insert(pair) {
                    events.push(SedpEvent::InconsistentTopic { local, remote });
                }
            }
            Match::Unresolved(missing) => {
                self.unresolved.insert(pair, missing);
            }
            Match::Matched | Match::Unrelated => {}
        }
        let was = self.matches.contains(&(local, remote));
        if now && !was {
            self.matches.insert((local, remote));
            events.push(SedpEvent::Matched { local, remote });
        } else if !now && was {
            self.matches.remove(&(local, remote));
            events.push(SedpEvent::Unmatched { local, remote });
        }
    }
}

/// The endpoint a dispose or unregister refers to: from the payload's
/// `PID_ENDPOINT_GUID` (9.6.3.2), or else from the in-line
/// `PID_KEY_HASH`, which for a GUID key is the GUID itself (9.6.4.8).
fn disposed_key(c: &Change) -> Option<Guid> {
    if let Some(p) = &c.payload {
        if let Ok((list, e)) = decode_pl_payload(&p.bytes) {
            if let Some(g) = list.get(pid::ENDPOINT_GUID) {
                return PlValues::new(e).guid(g).ok();
            }
        }
    }
    let hash = c.inline_qos.as_ref()?.get(pid::KEY_HASH)?;
    let b: [u8; 16] = hash.value.get(..16)?.try_into().ok()?;
    let mut prefix = [0u8; 12];
    prefix.copy_from_slice(&b[..12]);
    Some(Guid::new(
        GuidPrefix(prefix),
        EntityId::from_bytes([b[12], b[13], b[14], b[15]]),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::qos::ReliabilityKind;
    use crate::types::entity_kind;
    use std::time::Duration as StdDuration;

    fn prefix(n: u8) -> GuidPrefix {
        GuidPrefix([n; 12])
    }

    fn wguid(n: u8) -> Guid {
        Guid::new(
            prefix(n),
            EntityId::user([0, 0, 1], entity_kind::USER_WRITER_NO_KEY),
        )
    }

    fn rguid(n: u8) -> Guid {
        Guid::new(
            prefix(n),
            EntityId::user([0, 0, 2], entity_kind::USER_READER_NO_KEY),
        )
    }

    fn participant(n: u8) -> ParticipantData {
        let mut p = ParticipantData::new(prefix(n), VendorId::UNKNOWN);
        p.available_builtin_endpoints.0 |= Sedp::BUILTIN_ENDPOINTS;
        p.metatraffic_unicast_locators = vec![Locator::udpv4(
            format!("127.0.0.1:{}", 7000 + n as u16).parse().unwrap(),
        )];
        p.default_unicast_locators = vec![Locator::udpv4(
            format!("127.0.0.1:{}", 8000 + n as u16).parse().unwrap(),
        )];
        p
    }

    fn sedp(n: u8) -> Sedp {
        Sedp::new(prefix(n), VendorId::UNKNOWN, SedpConfig::default())
    }

    /// Two participants that know each other.
    fn pair(t: Instant) -> (Sedp, Sedp) {
        let (mut a, mut b) = (sedp(1), sedp(2));
        a.add_participant(&participant(2), t);
        b.add_participant(&participant(1), t);
        (a, b)
    }

    /// Exchanges datagrams until both are quiet; `drop(i)` loses the
    /// i-th datagram. Returns the events of a and of b.
    fn pump(
        a: &mut Sedp,
        b: &mut Sedp,
        t: Instant,
        drop: impl Fn(usize) -> bool,
    ) -> (Vec<SedpEvent>, Vec<SedpEvent>) {
        let (mut ea, mut eb) = (Vec::new(), Vec::new());
        let mut i = 0;
        for round in 0..50 {
            let now = t + StdDuration::from_secs(round);
            let out_a = a.poll(now);
            let out_b = b.poll(now);
            if out_a.is_empty() && out_b.is_empty() && round > 0 {
                break;
            }
            for o in out_a {
                if !drop(i) {
                    eb.extend(b.handle_message(&o.bytes, now));
                }
                i += 1;
            }
            for o in out_b {
                if !drop(i) {
                    ea.extend(a.handle_message(&o.bytes, now));
                }
                i += 1;
            }
        }
        (ea, eb)
    }

    #[test]
    fn content_filter_signature() {
        let f = ContentFilterProperty {
            content_filtered_topic_name: "a".into(),
            related_topic_name: "b".into(),
            filter_class_name: "DDSSQL".into(),
            filter_expression: "x > %0".into(),
            expression_parameters: vec!["1".into()],
        };
        assert_eq!(
            f.signature(),
            rustorb_typesupport::md5(b"a\0b\0DDSSQL\0x > %0\0\x31\0")
        );
    }

    #[test]
    fn fast_dds_content_filter() {
        // PID_CONTENT_FILTER_PROPERTY of a Fast DDS 2.11.2 reader of a
        // ContentFilteredTopic (interop/fastdds/hello.cpp, filtered),
        // little endian, captured.
        let value = b"\x15\0\0\0hello_world_filtered\0\0\0\0\x0c\0\0\0hello_world\0\
            \x07\0\0\0DDSSQL\0\0\x25\0\0\0index > %0 AND message LIKE 'Hello%'\0\0\0\0\
            \x01\0\0\0\x02\0\0\x005\0\0\0";
        let p = Parameter::new(pid::CONTENT_FILTER_PROPERTY, value.to_vec());
        let f = crate::pl::PlValues::new(Endianness::Little)
            .content_filter(&p)
            .unwrap();
        assert_eq!(
            f,
            ContentFilterProperty {
                content_filtered_topic_name: "hello_world_filtered".into(),
                related_topic_name: "hello_world".into(),
                filter_class_name: "DDSSQL".into(),
                filter_expression: "index > %0 AND message LIKE 'Hello%'".into(),
                expression_parameters: vec!["5".into()],
            }
        );
        let b = crate::pl::PlBuilder::new(Endianness::Little);
        // The same, but for the padding of the list.
        let mut ours = b.content_filter(pid::CONTENT_FILTER_PROPERTY, &f).value;
        ours.resize(ours.len().next_multiple_of(4), 0);
        assert_eq!(ours, value);
    }

    #[test]
    fn endpoint_data_round_trip() {
        let mut d = EndpointData::reader(rguid(1), "Square", "ShapeType");
        d.reliability.kind = ReliabilityKind::Reliable;
        d.durability = DurabilityKind::TransientLocal;
        d.partitions = vec!["p1".into(), "p2".into()];
        d.user_data = vec![1, 2];
        d.unicast_locators = participant(1).default_unicast_locators;
        d.multicast_locators = vec![Locator::udpv4("239.255.0.1:7401".parse().unwrap())];
        d.expects_inline_qos = true;
        d.content_filter = Some(ContentFilterProperty {
            content_filtered_topic_name: "BigSquares".into(),
            related_topic_name: "Square".into(),
            filter_class_name: "DDSSQL".into(),
            filter_expression: "x > %0 AND color = %1".into(),
            expression_parameters: vec!["100".into(), "'BLUE'".into()],
        });
        d.related_entity_guid = Some(rguid(2));
        d.service_instance_name = Some("robot".into());
        for e in [Endianness::Big, Endianness::Little] {
            assert_eq!(
                EndpointData::from_payload(&d.to_payload(e), false).unwrap(),
                d
            );
        }
    }

    #[test]
    fn endpoint_data_defaults_and_rules() {
        let e = Endianness::Little;
        let b = PlBuilder::new(e);
        let mut l = ParameterList::new();
        l.push(b.guid(pid::ENDPOINT_GUID, wguid(3)));
        assert_eq!(
            EndpointData::from_parameter_list(&l, e, true),
            Err(DecodeError::MissingParameter(pid::TOPIC_NAME))
        );
        l.push(b.string(pid::TOPIC_NAME, "T"));
        l.push(b.string(pid::TYPE_NAME, "X"));
        // Without RELIABILITY: RELIABLE for a writer, BEST_EFFORT for a
        // reader; participant from the endpoint's prefix.
        let w = EndpointData::from_parameter_list(&l, e, true).unwrap();
        assert_eq!(w, EndpointData::writer(wguid(3), "T", "X"));
        assert_eq!(w.participant_guid.prefix, prefix(3));
        let r = EndpointData::from_parameter_list(&l, e, false).unwrap();
        assert_eq!(r.reliability.kind, ReliabilityKind::BestEffort);
        l.push(Parameter::new(
            crate::parameter::ParameterId(0x4fff),
            [0; 4],
        ));
        assert!(matches!(
            EndpointData::from_parameter_list(&l, e, true),
            Err(DecodeError::MustUnderstand(_))
        ));
    }

    #[test]
    fn match_rules() {
        let w = EndpointData::writer(wguid(1), "T", "X");
        let r = EndpointData::reader(rguid(2), "T", "X");
        assert!(endpoints_match(&w, &r));
        let mut r2 = r.clone();
        r2.topic_name = "U".into();
        assert!(!endpoints_match(&w, &r2));
        let mut r2 = r.clone();
        r2.type_name = "Y".into();
        assert!(!endpoints_match(&w, &r2));
        let mut r2 = r.clone();
        r2.durability = DurabilityKind::TransientLocal;
        assert!(!endpoints_match(&w, &r2));
        let mut w2 = w.clone();
        w2.reliability.kind = ReliabilityKind::BestEffort;
        let mut r2 = r.clone();
        r2.reliability.kind = ReliabilityKind::Reliable;
        assert!(!endpoints_match(&w2, &r2));
        assert!(endpoints_match(&w, &r2));
        let mut r2 = r.clone();
        r2.partitions = vec!["x".into()];
        assert!(!endpoints_match(&w, &r2));
    }

    #[test]
    fn discover_match_and_remove() {
        let t = Instant::now();
        let (mut a, mut b) = pair(t);
        assert!(
            a.add_writer(EndpointData::writer(wguid(1), "T", "X"))
                .is_empty()
        );
        assert!(
            b.add_reader(EndpointData::reader(rguid(2), "T", "X"))
                .is_empty()
        );
        let (ea, eb) = pump(&mut a, &mut b, t, |_| false);

        // b learns the writer, with a's default locators filled in.
        let mut w = EndpointData::writer(wguid(1), "T", "X");
        w.unicast_locators = participant(1).default_unicast_locators;
        assert_eq!(
            eb,
            [
                SedpEvent::WriterDiscovered(w),
                SedpEvent::Matched {
                    local: rguid(2),
                    remote: wguid(1)
                }
            ]
        );
        assert_eq!(ea.len(), 2);
        assert_eq!(
            ea[1],
            SedpEvent::Matched {
                local: wguid(1),
                remote: rguid(2)
            }
        );
        assert_eq!(a.matched(&wguid(1)).collect::<Vec<_>>(), [&rguid(2)]);
        assert!(a.remote_reader(&rguid(2)).is_some());

        // Removing the writer reaches b as a dispose.
        assert_eq!(
            a.remove_writer(&wguid(1)),
            [SedpEvent::Unmatched {
                local: wguid(1),
                remote: rguid(2)
            }]
        );
        let (_, eb) = pump(&mut a, &mut b, t + StdDuration::from_secs(100), |_| false);
        assert_eq!(
            eb,
            [
                SedpEvent::Unmatched {
                    local: rguid(2),
                    remote: wguid(1)
                },
                SedpEvent::WriterRemoved(wguid(1))
            ]
        );
        assert_eq!(b.remote_writers().count(), 0);

        // The participant goes away.
        assert_eq!(
            a.remove_participant(&prefix(2)),
            [SedpEvent::ReaderRemoved(rguid(2))]
        );
    }

    #[test]
    fn qos_update_changes_the_match() {
        let t = Instant::now();
        let (mut a, mut b) = pair(t);
        let mut w = EndpointData::writer(wguid(1), "T", "X");
        w.reliability.kind = ReliabilityKind::BestEffort;
        a.add_writer(w.clone());
        let mut r = EndpointData::reader(rguid(2), "T", "X");
        r.reliability.kind = ReliabilityKind::Reliable;
        b.add_reader(r);
        let (_, eb) = pump(&mut a, &mut b, t, |_| false);
        assert_eq!(eb.len(), 2, "{eb:?}");
        assert!(matches!(eb[0], SedpEvent::WriterDiscovered(_)));
        // A best-effort writer does not satisfy a reliable reader.
        assert_eq!(
            eb[1],
            SedpEvent::IncompatibleQos {
                local: rguid(2),
                remote: wguid(1),
                policies: vec![QosPolicyId::RELIABILITY],
            }
        );

        w.reliability.kind = ReliabilityKind::Reliable;
        a.add_writer(w);
        let (_, eb) = pump(&mut a, &mut b, t + StdDuration::from_secs(100), |_| false);
        assert!(matches!(eb[0], SedpEvent::WriterUpdated(_)));
        assert_eq!(
            eb[1],
            SedpEvent::Matched {
                local: rguid(2),
                remote: wguid(1)
            }
        );
    }

    #[test]
    fn survives_loss() {
        let t = Instant::now();
        let (mut a, mut b) = pair(t);
        for i in 0..5u8 {
            let g = Guid::new(
                prefix(1),
                EntityId::user([0, 1, i], entity_kind::USER_WRITER_NO_KEY),
            );
            a.add_writer(EndpointData::writer(g, "T", "X"));
        }
        b.add_reader(EndpointData::reader(rguid(2), "T", "X"));
        // Lose every other datagram at first.
        let (_, eb) = pump(&mut a, &mut b, t, |i| i < 12 && i % 2 == 0);
        let matched = eb
            .iter()
            .filter(|e| matches!(e, SedpEvent::Matched { .. }))
            .count();
        assert_eq!(matched, 5, "{eb:?}");
        assert_eq!(b.remote_writers().count(), 5);
    }

    #[test]
    fn own_and_unknown_participants_are_ignored() {
        let t = Instant::now();
        let mut a = sedp(1);
        a.add_participant(&participant(1), t);
        assert!(a.poll(t).is_empty());
        // Data from a participant not added is not taken.
        let mut c = sedp(3);
        c.add_participant(&participant(1), t);
        c.add_writer(EndpointData::writer(wguid(3), "T", "X"));
        for o in c.poll(t) {
            assert!(a.handle_message(&o.bytes, t).is_empty());
        }
        assert!(a.handle_message(b"junk", t).is_empty());
    }

    fn appendable(name: &str, members: &[&str]) -> rustorb_xtypes::types::TypeObjects {
        use rustorb_xtypes::types::*;
        let ty: Type = StructType {
            name: name.into(),
            members: members
                .iter()
                .enumerate()
                .map(|(i, m)| StructMember::new(*m, i as u32, Type::Primitive(Primitive::Int32)))
                .collect(),
            ..Default::default()
        }
        .into();
        TypeObjects::new(&ty).unwrap()
    }

    #[test]
    fn xtypes_parameters_round_trip() {
        let mut d = EndpointData::reader(rguid(1), "T", "X");
        d.type_information = Some(appendable("X", &["a", "b"]).information);
        d.data_representation = vec![data_representation::XCDR, data_representation::XCDR2];
        d.type_consistency = Some(TypeConsistency {
            ignore_member_names: true,
            ..Default::default()
        });
        for e in [Endianness::Big, Endianness::Little] {
            assert_eq!(
                EndpointData::from_payload(&d.to_payload(e), false).unwrap(),
                d
            );
        }
    }

    /// The XTypes parameters of a Cyclone DDS 0.10.4 reader of
    /// `HelloWorld::Greeting` (interop/idl/HelloWorld.idl), from a capture.
    #[test]
    fn cyclonedds_parameters() {
        let hex = |s: &str| -> Vec<u8> {
            (0..s.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
                .collect()
        };
        let e = Endianness::Little;
        let b = PlBuilder::new(e);
        let mut l = ParameterList::new();
        l.push(b.guid(pid::ENDPOINT_GUID, rguid(3)));
        l.push(b.string(pid::TOPIC_NAME, "hello_world"));
        l.push(b.string(pid::TYPE_NAME, "HelloWorld::Greeting"));
        l.push(Parameter::new(
            pid::DATA_REPRESENTATION,
            hex("0200000000000200"),
        ));
        l.push(Parameter::new(
            pid::TYPE_INFORMATION,
            hex(concat!(
                "6000000001100040280000002400000014000000f14c98d3b709b9631e344fd7",
                "0a3402003800000000000000040000000000000002100040280000002400000014",
                "000000f2d8dede3f3e15043e059145548d1b006600000000000000040000000000",
                "0000"
            )),
        ));
        let d = EndpointData::from_parameter_list(&l, e, false).unwrap();
        assert_eq!(d.data_representation, vec![0, 2]);
        assert!(d.accepts_representation(data_representation::XCDR));
        assert!(d.accepts_representation(data_representation::XCDR2));
        let info = d.type_information.as_ref().unwrap();
        assert_eq!(
            info.minimal.typeid_with_size.type_id,
            TypeIdentifier::Minimal(hex("4c98d3b709b9631e344fd70a3402").try_into().unwrap())
        );
        assert_eq!(info.minimal.typeid_with_size.typeobject_serialized_size, 56);
        // No TYPE_CONSISTENCY_ENFORCEMENT: the default, since it announced
        // its type.
        assert_eq!(d.type_consistency, None);
        assert_eq!(d.effective_type_consistency(), TypeConsistency::default());
        // A parameter that cannot be read is left out.
        let mut l2 = l.clone();
        l2.parameters.retain(|p| p.id != pid::TYPE_INFORMATION);
        l2.push(Parameter::new(pid::TYPE_INFORMATION, vec![1, 2, 3, 4]));
        let d2 = EndpointData::from_parameter_list(&l2, e, false).unwrap();
        assert_eq!(d2.type_information, None);
        assert_eq!(
            d2.effective_type_consistency(),
            TypeConsistency::NOT_ANNOUNCED
        );
    }

    #[test]
    fn type_and_representation_rules() {
        let registry = TypeRegistry::new();
        let v1 = appendable("X", &["a"]);
        let mut w = EndpointData::writer(wguid(1), "T", "X");
        let mut r = EndpointData::reader(rguid(2), "T", "X");
        // Different type names and no type information: inconsistent.
        r.type_name = "Y".into();
        assert_eq!(match_endpoints(&w, &r, &registry), Match::Inconsistent);
        // The same TypeIdentifier: names do not matter.
        w.type_information = Some(v1.information.clone());
        r.type_information = Some(v1.information.clone());
        assert_eq!(match_endpoints(&w, &r, &registry), Match::Matched);
        // A writer offering XCDR2 to a reader that takes XCDR only.
        w.data_representation = vec![data_representation::XCDR2];
        assert_eq!(
            match_endpoints(&w, &r, &registry),
            Match::Incompatible(vec![DATA_REPRESENTATION_QOS_POLICY_ID])
        );
        r.data_representation = vec![data_representation::XCDR, data_representation::XCDR2];
        assert_eq!(match_endpoints(&w, &r, &registry), Match::Matched);
        // Inconsistent types in other partitions are still reported.
        r.partitions = vec!["p".into()];
        r.type_consistency = Some(TypeConsistency {
            kind: TypeConsistencyKind::DisallowTypeCoercion,
            ..Default::default()
        });
        w.type_information = Some(appendable("X", &["a", "b"]).information);
        assert_eq!(match_endpoints(&w, &r, &registry), Match::Inconsistent);
    }

    #[test]
    fn inconsistent_and_unresolved_types() {
        let t = Instant::now();
        let (mut a, mut b) = pair(t);
        let v1 = appendable("X", &["a"]);
        let v2 = appendable("X", &["a", "b"]);
        let other = appendable("X", &["z"]);
        a.registry_mut().add_type(&v2);
        b.registry_mut().add_type(&v1);
        // b's reader of v1 and a's writer of v2: b does not know v2 yet.
        let mut w = EndpointData::writer(wguid(1), "T", "X");
        w.type_information = Some(v2.information.clone());
        a.add_writer(w);
        let mut r = EndpointData::reader(rguid(2), "T", "X");
        r.type_information = Some(v1.information.clone());
        b.add_reader(r);
        let (ea, eb) = pump(&mut a, &mut b, t, |_| false);
        assert!(
            !eb.iter().any(|e| matches!(e, SedpEvent::Matched { .. })),
            "{eb:?}"
        );
        assert!(
            !ea.iter().any(|e| matches!(e, SedpEvent::Matched { .. })),
            "{ea:?}"
        );
        assert_eq!(
            b.unresolved_types(),
            vec![(v2.minimal_id().clone(), wguid(1))]
        );
        assert_eq!(
            a.unresolved_types(),
            vec![(v1.minimal_id().clone(), rguid(2))]
        );
        // Once the TypeObjects are known (TypeLookup), they match: v1 is
        // assignable from v2.
        b.registry_mut().add_pairs(&v2.minimal).unwrap();
        assert_eq!(
            b.rematch_unresolved(),
            vec![SedpEvent::Matched {
                local: rguid(2),
                remote: wguid(1)
            }]
        );
        a.registry_mut().add_pairs(&v1.minimal).unwrap();
        assert_eq!(
            a.rematch_unresolved(),
            vec![SedpEvent::Matched {
                local: wguid(1),
                remote: rguid(2)
            }]
        );
        assert!(b.unresolved_types().is_empty());

        // A writer of a type the reader's is not assignable from: both
        // sides report an inconsistent topic once.
        let w3 = Guid::new(
            prefix(1),
            EntityId::user([0, 0, 3], entity_kind::USER_WRITER_NO_KEY),
        );
        let mut w = EndpointData::writer(w3, "T", "X");
        w.type_information = Some(other.information.clone());
        a.registry_mut().add_type(&other);
        b.registry_mut().add_pairs(&other.minimal).unwrap();
        let ea = a.add_writer(w);
        let (_, eb) = pump(&mut a, &mut b, t + StdDuration::from_secs(100), |_| false);
        assert!(ea.contains(&SedpEvent::InconsistentTopic {
            local: w3,
            remote: rguid(2)
        }));
        assert!(eb.contains(&SedpEvent::InconsistentTopic {
            local: rguid(2),
            remote: w3
        }));
        assert!(!eb.iter().any(|e| matches!(e, SedpEvent::Matched { .. })));
    }
}
