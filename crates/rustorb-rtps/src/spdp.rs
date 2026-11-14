//! The Simple Participant Discovery Protocol (DDSI-RTPS 2.5, 8.5.3, 9.6.2
//! and 9.6.3).
//!
//! [`ParticipantData`] is `SPDPdiscoveredParticipantData` and its PL_CDR
//! form. [`Spdp`] holds the built-in SPDP writer and reader of one
//! participant without doing any I/O: the caller feeds it received
//! messages and the time, sends what it asks to send, and gets back
//! [`ParticipantEvent`]s.

use std::collections::hash_map::Entry;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

use crate::error::DecodeError;
use crate::message::{Header, Message};
use crate::parameter::{ParameterList, SecurityParameters};
use crate::pl::{PlBuilder, PlValues, decode_pl_payload, encode_pl_payload, pid};
use crate::submessage::{Data, InfoTimestamp, PayloadKind, SerializedPayload, Submessage};
use crate::types::{
    BuiltinEndpointSet, Count, Duration, EntityId, Guid, GuidPrefix, Locator, ProtocolVersion,
    SequenceNumber, Time, VendorId,
};
use crate::wire::Endianness;

/// `SPDPdiscoveredParticipantData` (Table 8.78).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParticipantData {
    /// The participant's GUID prefix (sent as `PID_PARTICIPANT_GUID`).
    pub guid_prefix: GuidPrefix,
    /// `domainId`. `None` when not sent, which means "the receiver's
    /// domain" (Table 9.19).
    pub domain_id: Option<u32>,
    /// `domainTag`; empty by default.
    pub domain_tag: String,
    /// `protocolVersion`.
    pub protocol_version: ProtocolVersion,
    /// `vendorId`.
    pub vendor_id: VendorId,
    /// `expectsInlineQos`.
    pub expects_inline_qos: bool,
    /// `metatrafficUnicastLocatorList`.
    pub metatraffic_unicast_locators: Vec<Locator>,
    /// `metatrafficMulticastLocatorList`.
    pub metatraffic_multicast_locators: Vec<Locator>,
    /// `defaultUnicastLocatorList`.
    pub default_unicast_locators: Vec<Locator>,
    /// `defaultMulticastLocatorList`.
    pub default_multicast_locators: Vec<Locator>,
    /// `availableBuiltinEndpoints`.
    pub available_builtin_endpoints: BuiltinEndpointSet,
    /// `leaseDuration`; `{100, 0}` by default.
    pub lease_duration: Duration,
    /// `manualLivelinessCount`.
    pub manual_liveliness_count: Option<Count>,
    /// `builtinEndpointQos`.
    pub builtin_endpoint_qos: Option<u32>,
    /// `user_data` of the DDS participant QoS.
    pub user_data: Vec<u8>,
    /// `PID_ENTITY_NAME`, if any.
    pub entity_name: Option<String>,
    /// The parameters of DDS Security: tokens and security info.
    pub security: SecurityParameters,
}

impl ParticipantData {
    /// The default lease duration (Table 9.19).
    pub const DEFAULT_LEASE_DURATION: Duration = Duration::from_secs(100);

    /// Data with the given prefix and vendor, and defaults for the rest.
    pub fn new(guid_prefix: GuidPrefix, vendor_id: VendorId) -> Self {
        ParticipantData {
            guid_prefix,
            domain_id: None,
            domain_tag: String::new(),
            protocol_version: ProtocolVersion::CURRENT,
            vendor_id,
            expects_inline_qos: false,
            metatraffic_unicast_locators: Vec::new(),
            metatraffic_multicast_locators: Vec::new(),
            default_unicast_locators: Vec::new(),
            default_multicast_locators: Vec::new(),
            available_builtin_endpoints: BuiltinEndpointSet(
                BuiltinEndpointSet::PARTICIPANT_ANNOUNCER
                    | BuiltinEndpointSet::PARTICIPANT_DETECTOR,
            ),
            lease_duration: Self::DEFAULT_LEASE_DURATION,
            manual_liveliness_count: None,
            builtin_endpoint_qos: None,
            user_data: Vec::new(),
            entity_name: None,
            security: SecurityParameters::default(),
        }
    }

    /// The participant's GUID.
    pub fn guid(&self) -> Guid {
        Guid::new(self.guid_prefix, EntityId::PARTICIPANT)
    }

    /// The parameter list. Values equal to their defaults are left out,
    /// as is the separate guidPrefix (Table 9.17).
    pub fn to_parameter_list(&self, endianness: Endianness) -> ParameterList {
        let b = PlBuilder::new(endianness);
        let mut l = ParameterList::new();
        l.push(b.guid(pid::PARTICIPANT_GUID, self.guid()));
        if let Some(d) = self.domain_id {
            l.push(b.u32(pid::DOMAIN_ID, d));
        }
        if !self.domain_tag.is_empty() {
            l.push(b.string(pid::DOMAIN_TAG, &self.domain_tag));
        }
        l.push(b.protocol_version(pid::PROTOCOL_VERSION, self.protocol_version));
        l.push(b.vendor_id(pid::VENDORID, self.vendor_id));
        if self.expects_inline_qos {
            l.push(b.bool(pid::EXPECTS_INLINE_QOS, true));
        }
        for (id, list) in [
            (
                pid::METATRAFFIC_UNICAST_LOCATOR,
                &self.metatraffic_unicast_locators,
            ),
            (
                pid::METATRAFFIC_MULTICAST_LOCATOR,
                &self.metatraffic_multicast_locators,
            ),
            (pid::DEFAULT_UNICAST_LOCATOR, &self.default_unicast_locators),
            (
                pid::DEFAULT_MULTICAST_LOCATOR,
                &self.default_multicast_locators,
            ),
        ] {
            for loc in list {
                l.push(b.locator(id, loc));
            }
        }
        l.push(b.u32(
            pid::BUILTIN_ENDPOINT_SET,
            self.available_builtin_endpoints.0,
        ));
        if self.lease_duration != Self::DEFAULT_LEASE_DURATION {
            l.push(b.duration(pid::PARTICIPANT_LEASE_DURATION, self.lease_duration));
        }
        if let Some(c) = self.manual_liveliness_count {
            l.push(b.u32(pid::PARTICIPANT_MANUAL_LIVELINESS_COUNT, c.0 as u32));
        }
        if let Some(q) = self.builtin_endpoint_qos {
            l.push(b.u32(pid::BUILTIN_ENDPOINT_QOS, q));
        }
        if !self.user_data.is_empty() {
            l.push(b.octets(pid::USER_DATA, &self.user_data));
        }
        if let Some(n) = &self.entity_name {
            l.push(b.string(pid::ENTITY_NAME, n));
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

    /// Reads the data from a parameter list.
    ///
    /// `PID_PARTICIPANT_GUID` is required. Unknown parameters are skipped,
    /// except protocol-defined ones with the must-understand bit
    /// (Table 9.6), which make the whole data invalid. Vendor-specific
    /// parameters are always skipped, since this implementation has no
    /// vendor-specific parameters of its own.
    pub fn from_parameter_list(
        list: &ParameterList,
        endianness: Endianness,
    ) -> Result<Self, DecodeError> {
        let v = PlValues::new(endianness);
        let mut d = ParticipantData::new(GuidPrefix::UNKNOWN, VendorId::UNKNOWN);
        let mut guid = None;
        for p in &list.parameters {
            match p.id {
                pid::PARTICIPANT_GUID => guid = Some(v.guid(p)?),
                pid::DOMAIN_ID => d.domain_id = Some(v.u32(p)?),
                pid::DOMAIN_TAG => d.domain_tag = v.string(p)?,
                pid::PROTOCOL_VERSION => d.protocol_version = v.protocol_version(p)?,
                pid::VENDORID => d.vendor_id = v.vendor_id(p)?,
                pid::EXPECTS_INLINE_QOS => d.expects_inline_qos = v.bool(p)?,
                pid::METATRAFFIC_UNICAST_LOCATOR => {
                    d.metatraffic_unicast_locators.push(v.locator(p)?)
                }
                pid::METATRAFFIC_MULTICAST_LOCATOR => {
                    d.metatraffic_multicast_locators.push(v.locator(p)?)
                }
                pid::DEFAULT_UNICAST_LOCATOR => d.default_unicast_locators.push(v.locator(p)?),
                pid::DEFAULT_MULTICAST_LOCATOR => d.default_multicast_locators.push(v.locator(p)?),
                pid::BUILTIN_ENDPOINT_SET => {
                    d.available_builtin_endpoints = BuiltinEndpointSet(v.u32(p)?)
                }
                pid::PARTICIPANT_LEASE_DURATION => d.lease_duration = v.duration(p)?,
                pid::PARTICIPANT_MANUAL_LIVELINESS_COUNT => {
                    d.manual_liveliness_count = Some(Count(v.u32(p)? as i32))
                }
                pid::BUILTIN_ENDPOINT_QOS => d.builtin_endpoint_qos = Some(v.u32(p)?),
                pid::USER_DATA => d.user_data = v.octets(p)?,
                pid::ENTITY_NAME => d.entity_name = Some(v.string(p)?),
                id if SecurityParameters::is_security(id) => d.security.push(p.clone(), endianness),
                id if !id.is_vendor_specific() && id.must_understand() => {
                    return Err(DecodeError::MustUnderstand(id));
                }
                _ => {}
            }
        }
        let guid = guid.ok_or(DecodeError::MissingParameter(pid::PARTICIPANT_GUID))?;
        d.guid_prefix = guid.prefix;
        Ok(d)
    }

    /// Reads the data from a serialized payload.
    pub fn from_payload(bytes: &[u8]) -> Result<Self, DecodeError> {
        let (list, e) = decode_pl_payload(bytes)?;
        Self::from_parameter_list(&list, e)
    }
}

/// Settings of [`Spdp`].
#[derive(Clone, Debug)]
pub struct SpdpConfig {
    /// The domain id of the local participant.
    pub domain_id: u32,
    /// How often to announce (`resendPeriod`); 30 s by default
    /// (9.6.2.4.2). It should be shorter than the lease duration.
    pub announce_period: std::time::Duration,
    /// Where to announce (`readerLocators`): by default the discovery
    /// multicast locator (9.6.2.4.1).
    pub announce_locators: Vec<Locator>,
}

impl SpdpConfig {
    /// The defaults, announcing to `announce_locators`.
    pub fn new(domain_id: u32, announce_locators: Vec<Locator>) -> Self {
        SpdpConfig {
            domain_id,
            announce_period: std::time::Duration::from_secs(30),
            announce_locators,
        }
    }
}

/// Why a remote participant was removed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RemovalReason {
    /// Its lease duration passed without an announcement.
    LeaseExpired,
    /// It said it is going away (disposed or unregistered).
    Disposed,
}

/// A change in the set of remote participants.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParticipantEvent {
    /// A participant was seen for the first time.
    Discovered(ParticipantData),
    /// A known participant announced different data.
    Updated(ParticipantData),
    /// A participant is gone.
    Removed(GuidPrefix, RemovalReason),
}

#[derive(Debug)]
struct Remote {
    data: ParticipantData,
    expires: Option<Instant>,
}

/// The SPDP built-in writer and reader of one participant.
///
/// The writer is a best-effort stateless writer with one change; it is
/// resent every `announce_period` (8.5.3.1). The reader keeps the remote
/// participants and drops those whose lease has passed (8.5.3.3.2).
#[derive(Debug)]
pub struct Spdp {
    local: ParticipantData,
    config: SpdpConfig,
    sequence_number: SequenceNumber,
    next_announce: Option<Instant>,
    remote: HashMap<GuidPrefix, Remote>,
    /// The authenticated participants whose data comes on
    /// DCPSParticipantsSecure (DDS Security 1.2, 7.5.1.6).
    secured: HashSet<GuidPrefix>,
}

/// `StatusInfo_t` flags (9.6.4.9).
const STATUS_DISPOSED: u8 = 0x01;
const STATUS_UNREGISTERED: u8 = 0x02;

impl Spdp {
    /// The SPDP endpoints of `local`. The first announcement is due at
    /// once.
    pub fn new(local: ParticipantData, config: SpdpConfig) -> Self {
        Spdp {
            local,
            config,
            sequence_number: SequenceNumber(1),
            next_announce: None,
            remote: HashMap::new(),
            secured: HashSet::new(),
        }
    }

    /// The local participant's data.
    pub fn local(&self) -> &ParticipantData {
        &self.local
    }

    /// Replaces the local data. It is announced at the next
    /// [`Spdp::poll_announce`] with a new sequence number.
    pub fn set_local(&mut self, data: ParticipantData) {
        self.local = data;
        self.sequence_number.0 += 1;
        self.next_announce = None;
    }

    /// The announcement message: the header, INFO_TS and a DATA with the
    /// local participant data, from the SPDP writer to any reader
    /// (8.4.8.1.4: `DATA.readerId := ENTITYID_UNKNOWN`).
    pub fn announcement(&self, endianness: Endianness) -> Message {
        let mut msg = Message::new(Header::new(self.local.vendor_id, self.local.guid_prefix));
        msg.submessages
            .push(Submessage::InfoTimestamp(InfoTimestamp {
                timestamp: Some(Time::now()),
            }));
        msg.submessages.push(Submessage::Data(Data {
            reader_id: EntityId::UNKNOWN,
            writer_id: EntityId::SPDP_BUILTIN_PARTICIPANT_ANNOUNCER,
            writer_sn: self.sequence_number,
            inline_qos: None,
            serialized_payload: Some(SerializedPayload {
                kind: PayloadKind::Data,
                bytes: self.local.to_payload(endianness),
            }),
            non_standard_payload: false,
        }));
        msg
    }

    /// The message that tells the others that the local participant is
    /// going away, and the locators to send it to: a DATA with
    /// `PID_STATUS_INFO` disposed and unregistered, the key hash in-line
    /// and the key, `PID_PARTICIPANT_GUID`, as the payload (9.6.3.2). It
    /// takes the next sequence number.
    pub fn dispose(&mut self) -> (Vec<u8>, &[Locator]) {
        self.sequence_number.0 += 1;
        let e = Endianness::NATIVE;
        let guid = self.local.guid();
        let mut qos = ParameterList::new();
        qos.push(crate::parameter::Parameter::new(
            pid::KEY_HASH,
            guid.to_bytes(),
        ));
        qos.push(crate::parameter::Parameter::new(
            pid::STATUS_INFO,
            [0, 0, 0, STATUS_DISPOSED | STATUS_UNREGISTERED],
        ));
        let mut key = ParameterList::new();
        key.push(PlBuilder::new(e).guid(pid::PARTICIPANT_GUID, guid));
        let mut msg = Message::new(Header::new(self.local.vendor_id, self.local.guid_prefix));
        msg.submessages
            .push(Submessage::InfoTimestamp(InfoTimestamp {
                timestamp: Some(Time::now()),
            }));
        msg.submessages.push(Submessage::Data(Data {
            reader_id: EntityId::UNKNOWN,
            writer_id: EntityId::SPDP_BUILTIN_PARTICIPANT_ANNOUNCER,
            writer_sn: self.sequence_number,
            inline_qos: Some(qos),
            serialized_payload: Some(SerializedPayload {
                kind: PayloadKind::Key,
                bytes: encode_pl_payload(&key, e),
            }),
            non_standard_payload: false,
        }));
        let bytes = msg.encode(e).expect("a dispose is always encodable");
        (bytes, &self.config.announce_locators)
    }

    /// If an announcement is due at `now`, returns the encoded message and
    /// the locators to send it to, and schedules the next one.
    pub fn poll_announce(&mut self, now: Instant) -> Option<(Vec<u8>, &[Locator])> {
        if self.next_announce.is_some_and(|t| now < t) {
            return None;
        }
        self.next_announce = Some(now + self.config.announce_period);
        let bytes = self
            .announcement(Endianness::NATIVE)
            .encode(Endianness::NATIVE)
            .expect("an announcement is always encodable");
        Some((bytes, &self.config.announce_locators))
    }

    /// The next time [`Spdp::poll_announce`] or [`Spdp::remove_expired`]
    /// has something to do.
    pub fn next_deadline(&self) -> Option<Instant> {
        let expiry = self.remote.values().filter_map(|r| r.expires).min();
        match (self.next_announce, expiry) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    /// The remote participants currently known.
    pub fn participants(&self) -> impl Iterator<Item = &ParticipantData> {
        self.remote.values().map(|r| &r.data)
    }

    /// Looks up a remote participant.
    pub fn participant(&self, prefix: &GuidPrefix) -> Option<&ParticipantData> {
        self.remote.get(prefix).map(|r| &r.data)
    }

    /// Removes participants whose lease has passed.
    pub fn remove_expired(&mut self, now: Instant) -> Vec<ParticipantEvent> {
        let gone: Vec<GuidPrefix> = self
            .remote
            .iter()
            .filter(|(_, r)| r.expires.is_some_and(|t| t <= now))
            .map(|(k, _)| *k)
            .collect();
        gone.into_iter()
            .map(|k| {
                self.remote.remove(&k);
                self.secured.remove(&k);
                ParticipantEvent::Removed(k, RemovalReason::LeaseExpired)
            })
            .collect()
    }

    /// Processes one received RTPS message and returns what changed.
    ///
    /// Only DATA from SPDP writers is looked at. Messages and submessages
    /// that are invalid, data for another domain or domain tag, and the
    /// local participant's own announcements are ignored. The receiver
    /// state follows 8.3.4 as far as SPDP needs: INFO_DST to another
    /// participant makes the DATA after it not for us.
    pub fn handle_message(&mut self, bytes: &[u8], now: Instant) -> Vec<ParticipantEvent> {
        let Ok((_, submessages)) = Message::parse(bytes) else {
            return Vec::new();
        };
        let mut events = Vec::new();
        let mut for_us = true;
        for sub in submessages {
            let Ok(sub) = sub else { break };
            match sub {
                Submessage::InfoDestination(d) => {
                    for_us = d.guid_prefix == GuidPrefix::UNKNOWN
                        || d.guid_prefix == self.local.guid_prefix;
                }
                Submessage::Data(d)
                    if for_us && d.writer_id == EntityId::SPDP_BUILTIN_PARTICIPANT_ANNOUNCER =>
                {
                    if let Some(e) = self.handle_data(&d, now) {
                        events.push(e);
                    }
                }
                _ => {}
            }
        }
        events
    }

    fn handle_data(&mut self, d: &Data, now: Instant) -> Option<ParticipantEvent> {
        let status = d
            .inline_qos
            .as_ref()
            .and_then(|q| q.get(pid::STATUS_INFO))
            .and_then(|p| p.value.get(3).copied())
            .unwrap_or(0);
        if status & (STATUS_DISPOSED | STATUS_UNREGISTERED) != 0 {
            let prefix = self.disposed_key(d)?;
            if self.secured.contains(&prefix) {
                return None;
            }
            self.remote.remove(&prefix)?;
            return Some(ParticipantEvent::Removed(prefix, RemovalReason::Disposed));
        }
        let payload = d.serialized_payload.as_ref()?;
        if payload.kind != PayloadKind::Data {
            return None;
        }
        let data = ParticipantData::from_payload(&payload.bytes).ok()?;
        if data.guid_prefix == self.local.guid_prefix
            || data.domain_id.is_some_and(|id| id != self.config.domain_id)
            || data.domain_tag != self.local.domain_tag
        {
            return None;
        }
        if self.secured.contains(&data.guid_prefix) {
            // Only its liveliness is taken from SPDP.
            if let Some(r) = self.remote.get_mut(&data.guid_prefix) {
                r.expires = r.data.lease_duration.to_std().map(|d| now + d);
            }
            return None;
        }
        self.update(data, now)
    }

    /// Takes in what an authenticated participant sent on
    /// DCPSParticipantsSecure (DDS Security 1.2, 7.5.1.6). From then on
    /// its data and its deletion are only taken from there: what SPDP
    /// says of it only keeps it alive.
    pub fn handle_secure(
        &mut self,
        event: crate::spdp_secure::SecureParticipantEvent,
        now: Instant,
    ) -> Option<ParticipantEvent> {
        use crate::spdp_secure::SecureParticipantEvent as S;
        match event {
            S::Data(data) => {
                let data = *data;
                if data.guid_prefix == self.local.guid_prefix
                    || data.domain_id.is_some_and(|id| id != self.config.domain_id)
                    || data.domain_tag != self.local.domain_tag
                {
                    return None;
                }
                self.secured.insert(data.guid_prefix);
                self.update(data, now)
            }
            S::Disposed(prefix) => {
                self.secured.remove(&prefix);
                self.remote.remove(&prefix)?;
                Some(ParticipantEvent::Removed(prefix, RemovalReason::Disposed))
            }
        }
    }

    /// Records the data of a remote participant.
    fn update(&mut self, data: ParticipantData, now: Instant) -> Option<ParticipantEvent> {
        let expires = data.lease_duration.to_std().map(|d| now + d);
        match self.remote.entry(data.guid_prefix) {
            Entry::Occupied(mut o) => {
                let r = o.get_mut();
                r.expires = expires;
                if r.data == data {
                    None
                } else {
                    r.data = data.clone();
                    Some(ParticipantEvent::Updated(data))
                }
            }
            Entry::Vacant(v) => {
                v.insert(Remote {
                    data: data.clone(),
                    expires,
                });
                // Answering a new participant at once shortens start-up
                // (8.5.3.1, optional).
                self.next_announce = None;
                Some(ParticipantEvent::Discovered(data))
            }
        }
    }

    /// The participant a dispose or unregister refers to: from the
    /// payload's `PID_PARTICIPANT_GUID` (9.6.3.2), or else from the
    /// in-line `PID_KEY_HASH`, which for a GUID key is the GUID itself
    /// (9.6.4.8 step 5.1).
    fn disposed_key(&self, d: &Data) -> Option<GuidPrefix> {
        if let Some(p) = &d.serialized_payload {
            if let Ok((list, e)) = decode_pl_payload(&p.bytes) {
                if let Some(g) = list.get(pid::PARTICIPANT_GUID) {
                    return PlValues::new(e).guid(g).ok().map(|g| g.prefix);
                }
            }
        }
        let hash = d.inline_qos.as_ref()?.get(pid::KEY_HASH)?;
        let mut prefix = [0u8; 12];
        prefix.copy_from_slice(hash.value.get(..12)?);
        Some(GuidPrefix(prefix))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parameter::Parameter;
    use std::time::Duration as StdDuration;

    fn data(n: u8) -> ParticipantData {
        let mut d = ParticipantData::new(GuidPrefix([n; 12]), VendorId([0x01, 0x99]));
        d.domain_id = Some(7);
        d.metatraffic_unicast_locators = vec![Locator::udpv4("10.0.0.1:9160".parse().unwrap())];
        d.metatraffic_multicast_locators =
            vec![Locator::udpv4("239.255.0.1:9150".parse().unwrap())];
        d.default_unicast_locators = vec![Locator::udpv4("10.0.0.1:9161".parse().unwrap())];
        d.lease_duration = Duration::from_secs(10);
        d
    }

    fn spdp(n: u8) -> Spdp {
        Spdp::new(data(n), SpdpConfig::new(7, vec![]))
    }

    #[test]
    fn participant_data_round_trip() {
        let mut d = data(1);
        d.domain_tag = "tag".into();
        d.expects_inline_qos = true;
        d.manual_liveliness_count = Some(Count(3));
        d.builtin_endpoint_qos = Some(1);
        d.user_data = vec![1, 2, 3];
        d.entity_name = Some("p".into());
        d.default_multicast_locators = vec![Locator::udpv4("239.255.0.1:9151".parse().unwrap())];
        for e in [Endianness::Big, Endianness::Little] {
            assert_eq!(ParticipantData::from_payload(&d.to_payload(e)).unwrap(), d);
        }
    }

    #[test]
    fn participant_data_rules() {
        let e = Endianness::Little;
        let b = PlBuilder::new(e);
        // Missing GUID.
        let l = ParameterList::new();
        assert_eq!(
            ParticipantData::from_parameter_list(&l, e),
            Err(DecodeError::MissingParameter(pid::PARTICIPANT_GUID))
        );
        let mut l = ParameterList::new();
        l.push(b.guid(pid::PARTICIPANT_GUID, data(2).guid()));
        // Unknown and vendor-specific parameters are skipped, even with
        // the must-understand bit when vendor-specific.
        l.push(Parameter::new(ParameterId(0x0fff), [0; 4]));
        l.push(Parameter::new(ParameterId(0xc001), [0; 4]));
        let d = ParticipantData::from_parameter_list(&l, e).unwrap();
        assert_eq!(d.guid_prefix, GuidPrefix([2; 12]));
        assert_eq!(d.domain_id, None);
        assert_eq!(d.lease_duration, ParticipantData::DEFAULT_LEASE_DURATION);
        // An unknown protocol parameter that must be understood.
        l.push(Parameter::new(ParameterId(0x4fff), [0; 4]));
        assert_eq!(
            ParticipantData::from_parameter_list(&l, e),
            Err(DecodeError::MustUnderstand(ParameterId(0x4fff)))
        );
    }

    use crate::parameter::ParameterId;

    #[test]
    fn announce_schedule() {
        let mut s = spdp(1);
        s.config.announce_locators = vec![Locator::udpv4("239.255.0.1:9150".parse().unwrap())];
        let t0 = Instant::now();
        let (bytes, to) = s.poll_announce(t0).unwrap();
        assert_eq!(to.len(), 1);
        let msg = Message::decode(&bytes).unwrap();
        assert_eq!(msg.header.guid_prefix, GuidPrefix([1; 12]));
        let Submessage::Data(d) = &msg.submessages[1] else {
            panic!()
        };
        assert_eq!(d.reader_id, EntityId::UNKNOWN);
        assert_eq!(d.writer_id, EntityId::SPDP_BUILTIN_PARTICIPANT_ANNOUNCER);
        assert_eq!(
            ParticipantData::from_payload(&d.serialized_payload.as_ref().unwrap().bytes).unwrap(),
            data(1)
        );
        assert!(s.poll_announce(t0 + StdDuration::from_secs(29)).is_none());
        assert!(s.poll_announce(t0 + StdDuration::from_secs(30)).is_some());
        assert_eq!(s.next_deadline(), Some(t0 + StdDuration::from_secs(60)));
    }

    #[test]
    fn discover_update_and_expire() {
        let mut a = spdp(1);
        let mut b = spdp(2);
        let t0 = Instant::now();
        let (msg_b, _) = b.poll_announce(t0).unwrap();
        let (_, _) = a.poll_announce(t0).unwrap();

        let ev = a.handle_message(&msg_b, t0);
        assert_eq!(ev, [ParticipantEvent::Discovered(data(2))]);
        // A new participant makes the next announcement due at once.
        assert!(a.poll_announce(t0).is_some());
        // The same data again: no event, but the lease is renewed.
        let t1 = t0 + StdDuration::from_secs(8);
        assert!(a.handle_message(&msg_b, t1).is_empty());
        assert!(a.remove_expired(t0 + StdDuration::from_secs(12)).is_empty());

        let mut changed = data(2);
        changed.user_data = vec![9];
        b.set_local(changed.clone());
        let (msg_b2, _) = b.poll_announce(t1).unwrap();
        assert_eq!(
            a.handle_message(&msg_b2, t1),
            [ParticipantEvent::Updated(changed)]
        );
        assert_eq!(a.participants().count(), 1);

        assert_eq!(
            a.remove_expired(t1 + StdDuration::from_secs(10)),
            [ParticipantEvent::Removed(
                GuidPrefix([2; 12]),
                RemovalReason::LeaseExpired
            )]
        );
        assert_eq!(a.participants().count(), 0);
    }

    #[test]
    fn ignored_announcements() {
        let mut a = spdp(1);
        let t0 = Instant::now();
        // Own announcement.
        let (own, _) = a.poll_announce(t0).unwrap();
        assert!(a.handle_message(&own, t0).is_empty());
        // Another domain.
        let mut other = data(3);
        other.domain_id = Some(8);
        let mut s = Spdp::new(other, SpdpConfig::new(8, vec![]));
        let (m, _) = s.poll_announce(t0).unwrap();
        assert!(a.handle_message(&m, t0).is_empty());
        // Another domain tag.
        let mut other = data(4);
        other.domain_tag = "x".into();
        let mut s = Spdp::new(other, SpdpConfig::new(7, vec![]));
        let (m, _) = s.poll_announce(t0).unwrap();
        assert!(a.handle_message(&m, t0).is_empty());
        // No domain id means "the receiver's domain".
        let mut other = data(5);
        other.domain_id = None;
        let mut s = Spdp::new(other, SpdpConfig::new(7, vec![]));
        let (m, _) = s.poll_announce(t0).unwrap();
        assert_eq!(a.handle_message(&m, t0).len(), 1);
        // Addressed to another participant with INFO_DST.
        let mut msg = Spdp::new(data(6), SpdpConfig::new(7, vec![])).announcement(Endianness::Big);
        msg.submessages.insert(
            0,
            Submessage::InfoDestination(crate::submessage::InfoDestination {
                guid_prefix: GuidPrefix([9; 12]),
            }),
        );
        assert!(
            a.handle_message(&msg.encode(Endianness::Big).unwrap(), t0)
                .is_empty()
        );
        // Garbage.
        assert!(a.handle_message(b"RTPS", t0).is_empty());
    }

    #[test]
    fn dispose_removes() {
        let mut a = spdp(1);
        let t0 = Instant::now();
        let (m, _) = spdp(2).poll_announce(t0).unwrap();
        assert_eq!(a.handle_message(&m, t0).len(), 1);

        // A dispose carrying only the key hash in-line.
        let mut msg = Message::new(Header::new(VendorId([1, 1]), GuidPrefix([2; 12])));
        let mut qos = ParameterList::new();
        qos.push(Parameter::new(pid::KEY_HASH, data(2).guid().to_bytes()));
        qos.push(Parameter::new(pid::STATUS_INFO, [0, 0, 0, 3]));
        msg.submessages.push(Submessage::Data(Data {
            reader_id: EntityId::UNKNOWN,
            writer_id: EntityId::SPDP_BUILTIN_PARTICIPANT_ANNOUNCER,
            writer_sn: SequenceNumber(2),
            inline_qos: Some(qos),
            serialized_payload: None,
            non_standard_payload: false,
        }));
        let bytes = msg.encode(Endianness::Little).unwrap();
        assert_eq!(
            a.handle_message(&bytes, t0),
            [ParticipantEvent::Removed(
                GuidPrefix([2; 12]),
                RemovalReason::Disposed
            )]
        );
        assert_eq!(a.participants().count(), 0);
    }

    #[test]
    fn our_dispose_removes() {
        let mut a = spdp(1);
        let mut b = spdp(2);
        let t0 = Instant::now();
        let (m, _) = b.poll_announce(t0).unwrap();
        assert_eq!(a.handle_message(&m, t0).len(), 1);
        let (bytes, _) = b.dispose();
        assert_eq!(
            a.handle_message(&bytes, t0),
            [ParticipantEvent::Removed(
                GuidPrefix([2; 12]),
                RemovalReason::Disposed
            )]
        );
    }

    #[test]
    fn secured_participants_change_only_through_the_secure_topic() {
        use crate::spdp_secure::SecureParticipantEvent as S;
        let mut a = spdp(1);
        let mut b = spdp(2);
        let t0 = Instant::now();
        let (m, _) = b.poll_announce(t0).unwrap();
        assert_eq!(a.handle_message(&m, t0).len(), 1);
        // The same data on the secure topic changes nothing.
        assert_eq!(a.handle_secure(S::Data(Box::new(data(2))), t0), None);
        // A change on SPDP is ignored; it only keeps the lease.
        let mut changed = data(2);
        changed.user_data = vec![9];
        b.set_local(changed.clone());
        let t1 = t0 + StdDuration::from_secs(8);
        let (m, _) = b.poll_announce(t1).unwrap();
        assert!(a.handle_message(&m, t1).is_empty());
        assert!(a.remove_expired(t0 + StdDuration::from_secs(11)).is_empty());
        assert_eq!(a.participant(&GuidPrefix([2; 12])), Some(&data(2)));
        // So is a dispose.
        let (bytes, _) = b.dispose();
        assert!(a.handle_message(&bytes, t1).is_empty());
        // Both are taken from the secure topic.
        assert_eq!(
            a.handle_secure(S::Data(Box::new(changed.clone())), t1),
            Some(ParticipantEvent::Updated(changed))
        );
        assert_eq!(
            a.handle_secure(S::Disposed(GuidPrefix([2; 12])), t1),
            Some(ParticipantEvent::Removed(
                GuidPrefix([2; 12]),
                RemovalReason::Disposed
            ))
        );
        // Data of the local participant or another domain is ignored.
        assert_eq!(a.handle_secure(S::Data(Box::new(data(1))), t1), None);
        let mut other = data(3);
        other.domain_id = Some(8);
        assert_eq!(a.handle_secure(S::Data(Box::new(other)), t1), None);
    }

    #[test]
    fn infinite_lease_never_expires() {
        let mut a = spdp(1);
        let mut d = data(2);
        d.lease_duration = Duration::INFINITE;
        let t0 = Instant::now();
        let (m, _) = Spdp::new(d, SpdpConfig::new(7, vec![]))
            .poll_announce(t0)
            .unwrap();
        assert_eq!(a.handle_message(&m, t0).len(), 1);
        assert!(
            a.remove_expired(t0 + StdDuration::from_secs(1_000_000))
                .is_empty()
        );
    }
}
