//! DDS Security 1.2 (feature `security`): the builtin plugins of
//! `rustorb-dds-security` put to work in a participant.
//!
//! - Authentication (PKI-DH): the participant's GUID prefix is derived
//!   from its certificate, and every remote participant that announces an
//!   identity token goes through the handshake on the stateless message
//!   endpoints before discovery goes on with it.
//! - Access control (Permissions): the governance gives the protection of
//!   the domain and of each topic; the permissions say what each
//!   participant may join, publish and subscribe.
//! - Cryptography (AES-GCM-GMAC): after the handshake, the participants
//!   exchange keys on the volatile message secure endpoints, and protect
//!   their RTPS messages, submessages and payloads as the governance asks.
//!
//! The protection is applied to whole messages: [`Security::encode`] on
//! each message sent, [`Security::decode`] on each message received,
//! except for payloads, which are encoded when a change is written and
//! decoded when a change is read.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustorb_dds_security::Error as SecError;
use rustorb_dds_security::access::{Action, DomainRule, Grant, ProtectionKind};
use rustorb_dds_security::auth::{self, Authenticated, Handshake, Identity};
use rustorb_dds_security::crypto::{
    self, CRYPTO_TOKEN_CLASS, Decoder, Encoder, KEYMAT_PROPERTY, KeyMaterial, TransformKind,
    read_submessage, submessage_id,
};
use rustorb_dds_security::message::{self as gm, GenericMessage, MessageIdentity};
use rustorb_dds_security::pki::{Cert, read_uri};
use rustorb_dds_security::smime;
use rustorb_dds_security::token::{BinaryProperty, DataHolder};
use rustorb_dds_security::xml;
use rustorb_rtps::pl::pid;
use rustorb_rtps::receiver::receive;
use rustorb_rtps::reliable::{
    MatchedReader, Outgoing, ReaderConfig, ReliableReader, ReliableWriter, WriterConfig,
};
use rustorb_rtps::sedp::EndpointData;
use rustorb_rtps::spdp::ParticipantData;
use rustorb_rtps::{
    BuiltinEndpointSet, Data, Endianness, EntityId, Guid, GuidPrefix, Header, InfoDestination,
    Locator, Message, Parameter, PayloadKind, SecurityParameters, SequenceNumber,
    SerializedPayload, Submessage,
};

use crate::error::{Error, Result};
use crate::participant::VENDOR_ID;

/// Where the participant's security documents are (the properties of DDS
/// Security 1.2, 9.3.1.3 and 9.4.1.3): each is a URI, `file:` followed
/// by a path, or `data:,` followed by the document itself.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecurityConfig {
    /// `dds.sec.auth.identity_ca`: the certificate of the identity CA.
    pub identity_ca: String,
    /// `dds.sec.auth.identity_certificate`: the participant's
    /// certificate, issued by the identity CA.
    pub identity_certificate: String,
    /// `dds.sec.auth.private_key`: the private key of the certificate.
    pub private_key: String,
    /// `dds.sec.access.permissions_ca`: the certificate of the CA that
    /// signs the governance and the permissions.
    pub permissions_ca: String,
    /// `dds.sec.access.governance`: the signed governance document.
    pub governance: String,
    /// `dds.sec.access.permissions`: the signed permissions document.
    pub permissions: String,
}

impl SecurityConfig {
    /// The configuration given by the standard property names (as a
    /// DomainParticipantQos PROPERTY would); `None` if one is missing.
    pub fn from_properties<'a>(
        props: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Option<Self> {
        let props: HashMap<&str, &str> = props.into_iter().collect();
        let get = |n: &str| props.get(n).map(|v| v.to_string());
        Some(SecurityConfig {
            identity_ca: get("dds.sec.auth.identity_ca")?,
            identity_certificate: get("dds.sec.auth.identity_certificate")?,
            private_key: get("dds.sec.auth.private_key")?,
            permissions_ca: get("dds.sec.access.permissions_ca")?,
            governance: get("dds.sec.access.governance")?,
            permissions: get("dds.sec.access.permissions")?,
        })
    }
}

fn sec_error(what: &str) -> impl Fn(SecError) -> Error + '_ {
    move |e| Error::Security(format!("{what}: {e}"))
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// The transformation of a protection kind (10.4.1.2): AES256-GCM to
/// encrypt, AES256-GMAC to sign.
fn transform(kind: ProtectionKind) -> TransformKind {
    if kind.encrypts() {
        TransformKind::Aes256Gcm
    } else if kind.is_protected() {
        TransformKind::Aes256Gmac
    } else {
        TransformKind::None
    }
}

fn crypto_token(km: &KeyMaterial) -> DataHolder {
    DataHolder::new(CRYPTO_TOKEN_CLASS)
        .with_binary(BinaryProperty::new(KEYMAT_PROPERTY, km.to_cdr()))
}

fn key_materials(tokens: &[DataHolder]) -> Vec<KeyMaterial> {
    tokens
        .iter()
        .filter(|t| t.class_id == CRYPTO_TOKEN_CLASS)
        .filter_map(|t| KeyMaterial::from_cdr(t.binary(KEYMAT_PROPERTY)?).ok())
        .collect()
}

/// Two `u32` in the order `e`: the value of a security info parameter.
fn masks(m: (u32, u32), e: Endianness) -> Vec<u8> {
    let mut v = Vec::with_capacity(8);
    match e {
        Endianness::Little => {
            v.extend_from_slice(&m.0.to_le_bytes());
            v.extend_from_slice(&m.1.to_le_bytes());
        }
        Endianness::Big => {
            v.extend_from_slice(&m.0.to_be_bytes());
            v.extend_from_slice(&m.1.to_be_bytes());
        }
    }
    v
}

/// The security parameters of a participant or endpoint: each written in
/// both byte orders, for whichever the list is sent in.
fn parameters(mut f: impl FnMut(Endianness) -> Vec<Parameter>) -> SecurityParameters {
    let mut s = SecurityParameters::default();
    for e in [Endianness::Little, Endianness::Big] {
        for p in f(e) {
            s.push(p, e);
        }
    }
    s
}

/// A token found in the security parameters of a remote participant.
fn token_of(p: &SecurityParameters, id: rustorb_rtps::ParameterId) -> Option<DataHolder> {
    let (params, e) = p.any()?;
    let v = &params.iter().find(|p| p.id == id)?.value;
    DataHolder::from_cdr(v, e == Endianness::Little).ok()
}

/// The protection of a local endpoint.
struct LocalEndpoint {
    /// SubmessageProtection: encodes its submessages.
    submessage: Option<Encoder>,
    /// PayloadProtection (writers): encodes its payloads.
    payload: Option<Arc<Encoder>>,
    /// The key material sent to each matched remote endpoint (with its
    /// receiver-specific key, for origin authentication).
    sent: HashMap<Guid, KeyMaterial>,
    origin_authentication: bool,
}

/// The state of a remote participant.
struct Peer {
    data: ParticipantData,
    /// The handshake in progress.
    handshake: Option<Handshake>,
    /// The last handshake message sent, for sending again until the peer
    /// answers, and when to send it.
    last_sent: Option<(GenericMessage, Instant, Duration)>,
    authenticated: Option<Authenticated>,
    grant: Option<Grant>,
    /// The volatile message secure writer and reader for this peer: one
    /// each, so that its key messages go to it alone.
    volatile_writer: Option<ReliableWriter>,
    volatile_reader: Option<ReliableReader>,
    /// The key of the volatile endpoints (KxKey), both ways.
    kx: Option<(Encoder, Decoder)>,
    /// The participant key material sent to it.
    participant_sent: Option<KeyMaterial>,
    /// Its participant key material.
    participant: Option<Decoder>,
    /// Discovery may go on with it.
    ready: bool,
}

/// What happened to a remote participant, for the participant to act on.
#[derive(Debug)]
pub(crate) enum PeerEvent {
    /// It is authenticated, allowed and has its keys: discovery may go on
    /// with it.
    Ready(Box<ParticipantData>),
}

struct State {
    pdata: Vec<u8>,
    peers: HashMap<GuidPrefix, Peer>,
    stateless_sn: i64,
    volatile_sn: i64,
    endpoints: HashMap<Guid, LocalEndpoint>,
    /// The key materials of endpoints (remote ones, and the local
    /// writers' own for reading what they wrote), by endpoint.
    decoders: HashMap<Guid, Vec<Decoder>>,
    /// Remote endpoints whose submessages must come protected.
    protected_remote: HashMap<Guid, bool>,
    out: Vec<Outgoing>,
    events: Vec<PeerEvent>,
}

/// The security of a participant.
pub(crate) struct Security {
    prefix: GuidPrefix,
    domain_id: u32,
    identity: Identity,
    permissions_ca: Cert,
    /// The signed permissions document, sent in the handshake.
    permissions_doc: Vec<u8>,
    grant: Grant,
    rule: DomainRule,
    permissions_token: DataHolder,
    /// The participant's own key material, for RTPS protection.
    participant: Option<Encoder>,
    writer_config: WriterConfig,
    reader_config: ReaderConfig,
    state: Mutex<State>,
}

impl std::fmt::Debug for Security {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Security")
            .field("prefix", &self.prefix)
            .finish_non_exhaustive()
    }
}

/// How long to wait before sending a handshake message again, at first.
const FIRST_RESEND: Duration = Duration::from_millis(500);
/// The longest wait between two sends of a handshake message.
const MAX_RESEND: Duration = Duration::from_secs(8);

const STATELESS_WRITER: EntityId = EntityId::P2P_BUILTIN_PARTICIPANT_STATELESS_WRITER;
const STATELESS_READER: EntityId = EntityId::P2P_BUILTIN_PARTICIPANT_STATELESS_READER;
const VOLATILE_WRITER: EntityId = EntityId::P2P_BUILTIN_PARTICIPANT_VOLATILE_SECURE_WRITER;
const VOLATILE_READER: EntityId = EntityId::P2P_BUILTIN_PARTICIPANT_VOLATILE_SECURE_READER;

/// RTPS submessage ids (DDSI-RTPS 2.5, 9.4.5.1.1).
mod sub {
    pub const ACKNACK: u8 = 0x06;
    pub const HEARTBEAT: u8 = 0x07;
    pub const GAP: u8 = 0x08;
    pub const INFO_DST: u8 = 0x0e;
    pub const NACK_FRAG: u8 = 0x12;
    pub const HEARTBEAT_FRAG: u8 = 0x13;
    pub const DATA: u8 = 0x15;
    pub const DATA_FRAG: u8 = 0x16;
}

/// The endpoints a submessage is between: (reader id, writer id, whether
/// the writer sends it).
fn endpoints_of(id: u8, body: &[u8]) -> Option<(EntityId, EntityId, bool)> {
    let at = match id {
        sub::DATA | sub::DATA_FRAG => 4,
        sub::HEARTBEAT | sub::HEARTBEAT_FRAG | sub::GAP | sub::ACKNACK | sub::NACK_FRAG => 0,
        _ => return None,
    };
    let e = |o: usize| -> Option<EntityId> {
        Some(EntityId::from_bytes(body.get(o..o + 4)?.try_into().ok()?))
    };
    let from_writer = !matches!(id, sub::ACKNACK | sub::NACK_FRAG);
    Some((e(at)?, e(at + 4)?, from_writer))
}

/// Whether a submessage between these endpoints belongs to a
/// bootstrapping message (7.5.7), which RTPS protection leaves alone.
fn is_bootstrap(reader: EntityId, writer: EntityId) -> bool {
    [
        EntityId::SPDP_BUILTIN_PARTICIPANT_ANNOUNCER,
        EntityId::SPDP_BUILTIN_PARTICIPANT_DETECTOR,
        STATELESS_WRITER,
        STATELESS_READER,
        VOLATILE_WRITER,
        VOLATILE_READER,
    ]
    .iter()
    .any(|e| *e == reader || *e == writer)
}

fn is_volatile(reader: EntityId, writer: EntityId) -> bool {
    reader == VOLATILE_READER || writer == VOLATILE_WRITER
}

const RTPS_HEADER: usize = 20;

impl Security {
    /// Loads the identity, the governance and the permissions; checks
    /// that the participant may join `domain_id`. `candidate` is the GUID
    /// prefix the participant would have without security: the one used
    /// is derived from it and from the certificate.
    pub(crate) fn new(
        config: &SecurityConfig,
        domain_id: u32,
        candidate: GuidPrefix,
        writer_config: WriterConfig,
        reader_config: ReaderConfig,
    ) -> Result<Self> {
        let identity = Identity::from_uris(
            &config.identity_ca,
            &config.identity_certificate,
            &config.private_key,
        )
        .map_err(sec_error("identity"))?;
        let permissions_ca =
            Cert::parse(&read_uri(&config.permissions_ca).map_err(sec_error("permissions CA"))?)
                .map_err(sec_error("permissions CA"))?;
        let governance_doc = read_uri(&config.governance).map_err(sec_error("governance"))?;
        let governance = smime::verify(&governance_doc, &permissions_ca)
            .and_then(|x| xml::governance(&x))
            .map_err(sec_error("governance"))?;
        let rule = governance
            .domain_rule(domain_id, "")
            .map_err(sec_error("governance"))?
            .clone();
        let permissions_doc = read_uri(&config.permissions).map_err(sec_error("permissions"))?;
        let permissions = smime::verify(&permissions_doc, &permissions_ca)
            .and_then(|x| xml::permissions(&x))
            .map_err(sec_error("permissions"))?;
        let subject = identity.certificate().subject();
        let grant = permissions
            .grant(&subject, unix_now())
            .map_err(sec_error("permissions"))?
            .clone();
        if rule.enable_join_access_control && !grant.allows_join(domain_id) {
            return Err(Error::Security(format!(
                "{subject} may not join domain {domain_id}"
            )));
        }
        let mut guid = [0u8; 16];
        guid[..12].copy_from_slice(&candidate.0);
        guid[12..].copy_from_slice(&EntityId::PARTICIPANT.to_bytes());
        let prefix = GuidPrefix(identity.adjusted_prefix(&guid));
        let participant = rule
            .rtps_protection_kind
            .is_protected()
            .then(|| Encoder::new(KeyMaterial::generate(transform(rule.rtps_protection_kind))));
        let permissions_token = rustorb_dds_security::access::permissions_token(&permissions_ca);
        Ok(Security {
            prefix,
            domain_id,
            identity,
            permissions_ca,
            permissions_doc,
            grant,
            rule,
            permissions_token,
            participant,
            writer_config,
            reader_config,
            state: Mutex::new(State {
                pdata: Vec::new(),
                peers: HashMap::new(),
                stateless_sn: 0,
                volatile_sn: 0,
                endpoints: HashMap::new(),
                decoders: HashMap::new(),
                protected_remote: HashMap::new(),
                out: Vec::new(),
                events: Vec::new(),
            }),
        })
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The GUID prefix of the participant.
    pub(crate) fn prefix(&self) -> GuidPrefix {
        self.prefix
    }

    /// Adds the tokens, the security info and the builtin endpoints to
    /// the participant's SPDP data.
    pub(crate) fn announce(&self, data: &mut ParticipantData) {
        data.available_builtin_endpoints.0 |=
            BuiltinEndpointSet::PARTICIPANT_STATELESS_MESSAGE_WRITER
                | BuiltinEndpointSet::PARTICIPANT_STATELESS_MESSAGE_READER
                | BuiltinEndpointSet::PARTICIPANT_VOLATILE_MESSAGE_SECURE_WRITER
                | BuiltinEndpointSet::PARTICIPANT_VOLATILE_MESSAGE_SECURE_READER;
        if self.discovery_protected() {
            data.available_builtin_endpoints.0 |=
                rustorb_rtps::sedp::Sedp::BUILTIN_ENDPOINTS_SECURE;
        }
        if self.liveliness_protected() {
            data.available_builtin_endpoints.0 |= rustorb_rtps::wlp::Wlp::BUILTIN_ENDPOINTS_SECURE;
        }
        // The keys of the builtin secure endpoints (9.8.9.3).
        let mut st = self.state();
        for (local, _, _, _, kind) in self.builtin_secure() {
            st.endpoints
                .entry(Guid::new(self.prefix, local))
                .or_insert_with(|| LocalEndpoint {
                    submessage: Some(Encoder::new(KeyMaterial::generate(transform(kind)))),
                    payload: None,
                    sent: HashMap::new(),
                    origin_authentication: kind.origin_authenticated(),
                });
        }
        drop(st);
        let identity = self.identity.identity_token();
        let perm = self.permissions_token.clone();
        let m = self.rule.participant_masks();
        data.security = parameters(|e| {
            let little = e == Endianness::Little;
            vec![
                Parameter::new(pid::IDENTITY_TOKEN, identity.to_cdr_endian(little)),
                Parameter::new(pid::PERMISSIONS_TOKEN, perm.to_cdr_endian(little)),
                Parameter::new(pid::PARTICIPANT_SECURITY_INFO, masks(m, e)),
            ]
        });
        self.state().pdata = data.to_payload(Endianness::Big);
    }

    /// A participant was discovered or updated. Whether discovery may go
    /// on with it now; otherwise it goes on after the handshake and the
    /// key exchange, with a [`PeerEvent::Ready`].
    pub(crate) fn participant_discovered(&self, p: &ParticipantData, now: Instant) -> bool {
        if p.guid_prefix == self.prefix {
            return false;
        }
        let secure = token_of(&p.security, pid::IDENTITY_TOKEN)
            .is_some_and(|t| auth::is_pki_dh(&t.class_id));
        let mut st = self.state();
        if !secure {
            // An unauthenticated participant sees only what is not
            // protected, if the governance lets it in at all.
            return self.rule.allow_unauthenticated_participants;
        }
        if let Some(peer) = st.peers.get_mut(&p.guid_prefix) {
            peer.data = p.clone();
            return peer.ready;
        }
        st.peers.insert(
            p.guid_prefix,
            Peer {
                data: p.clone(),
                handshake: None,
                last_sent: None,
                authenticated: None,
                grant: None,
                volatile_writer: None,
                volatile_reader: None,
                kx: None,
                participant_sent: None,
                participant: None,
                ready: false,
            },
        );
        let local = Guid::new(self.prefix, EntityId::PARTICIPANT).to_bytes();
        let remote = Guid::new(p.guid_prefix, EntityId::PARTICIPANT).to_bytes();
        if auth::initiates(&local, &remote) {
            let st = &mut *st;
            let hs = Handshake::request(&self.identity, &self.permissions_doc, &st.pdata, None);
            let token = hs.sent.clone();
            let peer = st.peers.get_mut(&p.guid_prefix).expect("inserted");
            peer.handshake = Some(hs);
            let m = self.stateless(st, p.guid_prefix, MessageIdentity::default(), token);
            self.send_handshake(st, p.guid_prefix, m, now);
        }
        false
    }

    /// A participant is gone.
    pub(crate) fn participant_removed(&self, prefix: &GuidPrefix) {
        let mut st = self.state();
        st.peers.remove(prefix);
        st.decoders.retain(|g, _| g.prefix != *prefix);
        st.protected_remote.retain(|g, _| g.prefix != *prefix);
        for e in st.endpoints.values_mut() {
            e.sent.retain(|g, _| g.prefix != *prefix);
        }
    }

    /// A stateless message to `dest`.
    fn stateless(
        &self,
        st: &mut State,
        dest: GuidPrefix,
        related: MessageIdentity,
        token: DataHolder,
    ) -> GenericMessage {
        st.stateless_sn += 1;
        GenericMessage {
            message_identity: MessageIdentity {
                source_guid: Guid::new(self.prefix, STATELESS_WRITER).to_bytes(),
                sequence_number: st.stateless_sn,
            },
            related_message_identity: related,
            destination_participant_guid: Guid::new(dest, EntityId::PARTICIPANT).to_bytes(),
            message_class_id: gm::AUTH.into(),
            message_data: vec![token],
            ..Default::default()
        }
    }

    /// Sends a handshake message now and keeps it to send again.
    fn send_handshake(&self, st: &mut State, dest: GuidPrefix, m: GenericMessage, now: Instant) {
        let Some(peer) = st.peers.get_mut(&dest) else {
            return;
        };
        let out = stateless_datagram(self.prefix, &peer.data, &m);
        peer.last_sent = Some((m, now + FIRST_RESEND, FIRST_RESEND));
        st.out.extend(out);
    }

    /// Handles the stateless and volatile messages of a message received
    /// on a discovery channel.
    pub(crate) fn handle_discovery(&self, bytes: &[u8], now: Instant) {
        let Some((_, received)) = receive(bytes, self.prefix) else {
            return;
        };
        for r in received {
            let src = r.source_prefix;
            match r.submessage {
                Submessage::Data(d) if d.writer_id == STATELESS_WRITER => {
                    if let Some(m) = d
                        .serialized_payload
                        .as_ref()
                        .and_then(|p| GenericMessage::from_payload(&p.bytes).ok())
                    {
                        self.stateless_received(src, m, now);
                    }
                }
                Submessage::Data(d) if d.writer_id == VOLATILE_WRITER => {
                    let changes = {
                        let mut st = self.state();
                        let Some(reader) = st
                            .peers
                            .get_mut(&src)
                            .and_then(|p| p.volatile_reader.as_mut())
                        else {
                            continue;
                        };
                        reader.handle_data(Guid::new(src, d.writer_id), &d, r.timestamp)
                    };
                    for c in changes {
                        if let Some(m) = c
                            .payload
                            .as_ref()
                            .and_then(|p| GenericMessage::from_payload(&p.bytes).ok())
                        {
                            self.volatile_received(src, m, now);
                        }
                    }
                }
                Submessage::Heartbeat(h) if h.writer_id == VOLATILE_WRITER => {
                    let changes = {
                        let mut st = self.state();
                        let Some(reader) = st
                            .peers
                            .get_mut(&src)
                            .and_then(|p| p.volatile_reader.as_mut())
                        else {
                            continue;
                        };
                        reader.handle_heartbeat(Guid::new(src, h.writer_id), &h, now)
                    };
                    for c in changes {
                        if let Some(m) = c
                            .payload
                            .as_ref()
                            .and_then(|p| GenericMessage::from_payload(&p.bytes).ok())
                        {
                            self.volatile_received(src, m, now);
                        }
                    }
                }
                Submessage::Gap(g) if g.writer_id == VOLATILE_WRITER => {
                    let mut st = self.state();
                    if let Some(reader) = st
                        .peers
                        .get_mut(&src)
                        .and_then(|p| p.volatile_reader.as_mut())
                    {
                        reader.handle_gap(Guid::new(src, g.writer_id), &g);
                    }
                }
                Submessage::AckNack(a) if a.writer_id == VOLATILE_WRITER => {
                    let mut st = self.state();
                    if let Some(w) = st
                        .peers
                        .get_mut(&src)
                        .and_then(|p| p.volatile_writer.as_mut())
                    {
                        w.handle_acknack(Guid::new(src, a.reader_id), &a, now);
                        w.remove_acknowledged();
                    }
                }
                _ => {}
            }
        }
    }

    fn stateless_received(&self, src: GuidPrefix, m: GenericMessage, now: Instant) {
        let me = Guid::new(self.prefix, EntityId::PARTICIPANT).to_bytes();
        if m.destination_participant_guid != me || m.message_class_id != gm::AUTH {
            return;
        }
        let Some(token) = m.message_data.first() else {
            return;
        };
        let mut g = self.state();
        let st = &mut *g;
        let Some(peer) = st.peers.get_mut(&src) else {
            return;
        };
        match token.class_id.as_str() {
            auth::REQUEST_CLASS => {
                // The same request again: the reply was lost.
                if let (Some(hs), Some((sent, ..))) = (&peer.handshake, &peer.last_sent) {
                    if hs.sent.class_id == auth::REPLY_CLASS
                        && token.binary("challenge1") == hs.sent.binary("challenge1")
                    {
                        let resend = sent.clone();
                        st.out
                            .extend(stateless_datagram(self.prefix, &peer.data, &resend));
                        return;
                    }
                }
                match Handshake::reply(
                    &self.identity,
                    &self.permissions_doc,
                    &st.pdata,
                    token,
                    None,
                    None,
                ) {
                    Ok(hs) => {
                        let reply = hs.sent.clone();
                        peer.handshake = Some(hs);
                        peer.authenticated = None;
                        let m = self.stateless(st, src, m.message_identity, reply);
                        self.send_handshake(st, src, m, now);
                    }
                    Err(e) => log(&format!("handshake request from {src:?} rejected: {e}")),
                }
            }
            auth::REPLY_CLASS => {
                // The same reply again: the final message was lost.
                if peer.authenticated.is_some() {
                    if let Some((sent, ..)) = &peer.last_sent {
                        let resend = sent.clone();
                        st.out
                            .extend(stateless_datagram(self.prefix, &peer.data, &resend));
                    }
                    return;
                }
                let Some(hs) = peer.handshake.as_mut() else {
                    return;
                };
                match hs.process_reply(&self.identity, token) {
                    Ok((fin, a)) => {
                        let m = self.stateless(st, src, m.message_identity, fin);
                        let peer = st.peers.get_mut(&src).expect("present");
                        st.out
                            .extend(stateless_datagram(self.prefix, &peer.data, &m));
                        // Kept to answer a reply that comes again, but not
                        // sent again on its own.
                        peer.last_sent = Some((m, now + Duration::from_secs(3600), MAX_RESEND));
                        self.authenticated(st, src, a, now);
                    }
                    Err(e) => log(&format!("handshake reply from {src:?} rejected: {e}")),
                }
            }
            auth::FINAL_CLASS => {
                if peer.authenticated.is_some() {
                    return;
                }
                let Some(hs) = peer.handshake.as_ref() else {
                    return;
                };
                match hs.process_final(token) {
                    Ok(a) => {
                        peer.last_sent = None;
                        self.authenticated(st, src, a, now);
                    }
                    Err(e) => log(&format!("handshake final from {src:?} rejected: {e}")),
                }
            }
            _ => {}
        }
    }

    /// The handshake with `src` succeeded: checks its permissions, then
    /// sets up the volatile endpoints with the KxKey and sends it the
    /// participant's key material.
    fn authenticated(&self, st: &mut State, src: GuidPrefix, a: Authenticated, now: Instant) {
        let grant = match self.remote_grant(&a) {
            Ok(g) => g,
            Err(e) => {
                log(&format!("participant {src:?} not allowed: {e}"));
                st.peers.remove(&src);
                return;
            }
        };
        let kx = KeyMaterial::key_exchange(&a.challenge1, &a.challenge2, &a.shared_secret);
        let Some(peer) = st.peers.get_mut(&src) else {
            return;
        };
        let uc = peer.data.metatraffic_unicast_locators.clone();
        let mc = peer.data.metatraffic_multicast_locators.clone();
        let mut w = ReliableWriter::new(
            Guid::new(self.prefix, VOLATILE_WRITER),
            VENDOR_ID,
            self.writer_config.clone(),
        );
        w.add_matched_reader(
            Guid::new(src, VOLATILE_READER),
            MatchedReader {
                unicast: uc.clone(),
                multicast: mc.clone(),
                reliable: true,
                history: false,
            },
        );
        let mut r = ReliableReader::new(
            Guid::new(self.prefix, VOLATILE_READER),
            VENDOR_ID,
            self.reader_config.clone(),
        );
        r.add_writer(Guid::new(src, VOLATILE_WRITER), uc, mc, now);
        peer.volatile_writer = Some(w);
        peer.volatile_reader = Some(r);
        peer.kx = Some((Encoder::new(kx.clone()), Decoder::new(kx)));
        peer.authenticated = Some(a);
        peer.grant = Some(grant);
        peer.handshake = None;
        // The participant's key material, with a key of its own for this
        // peer when origin authentication is asked for.
        let tokens = match &self.participant {
            Some(enc) => {
                let km = if self.rule.rtps_protection_kind.origin_authenticated() {
                    enc.key_material().with_receiver_specific_key()
                } else {
                    enc.key_material().clone()
                };
                let t = vec![crypto_token(&km)];
                peer.participant_sent = Some(km);
                t
            }
            None => Vec::new(),
        };
        self.volatile_send(st, src, gm::PARTICIPANT_CRYPTO_TOKENS, None, None, tokens);
        let set = st
            .peers
            .get(&src)
            .map_or(0, |p| p.data.available_builtin_endpoints.0);
        for (local, remote, writer, bit, _) in self.builtin_secure() {
            if set & bit != 0 {
                self.matched_locked(
                    st,
                    Guid::new(self.prefix, local),
                    Guid::new(src, remote),
                    writer,
                );
            }
        }
        self.check_ready(st, src);
    }

    /// The grant of an authenticated participant: its permissions must be
    /// signed by the permissions CA, for its subject, and let it join.
    fn remote_grant(&self, a: &Authenticated) -> std::result::Result<Grant, SecError> {
        let doc = smime::verify(&a.peer_permissions, &self.permissions_ca)?;
        let perms = xml::permissions(&doc)?;
        let subject = a.peer_cert.subject();
        let g = perms.grant(&subject, unix_now())?.clone();
        if self.rule.enable_join_access_control && !g.allows_join(self.domain_id) {
            return Err(SecError::new(format!(
                "{subject} may not join domain {}",
                self.domain_id
            )));
        }
        Ok(g)
    }

    /// Discovery goes on with a peer once it is authenticated and, under
    /// RTPS protection, its participant key material is in.
    fn check_ready(&self, st: &mut State, src: GuidPrefix) {
        let Some(peer) = st.peers.get_mut(&src) else {
            return;
        };
        if peer.ready || peer.authenticated.is_none() {
            return;
        }
        if self.participant.is_some() && peer.participant.is_none() {
            return;
        }
        peer.ready = true;
        st.events
            .push(PeerEvent::Ready(Box::new(peer.data.clone())));
    }

    /// Writes a volatile message to `dest`.
    fn volatile_send(
        &self,
        st: &mut State,
        dest: GuidPrefix,
        class: &str,
        dest_endpoint: Option<Guid>,
        source_endpoint: Option<Guid>,
        tokens: Vec<DataHolder>,
    ) {
        st.volatile_sn += 1;
        let m = GenericMessage {
            message_identity: MessageIdentity {
                source_guid: Guid::new(self.prefix, VOLATILE_WRITER).to_bytes(),
                sequence_number: st.volatile_sn,
            },
            related_message_identity: MessageIdentity::default(),
            destination_participant_guid: Guid::new(dest, EntityId::PARTICIPANT).to_bytes(),
            destination_endpoint_guid: dest_endpoint.map_or([0; 16], Guid::to_bytes),
            source_endpoint_guid: source_endpoint.map_or([0; 16], Guid::to_bytes),
            message_class_id: class.into(),
            message_data: tokens,
        };
        if let Some(w) = st
            .peers
            .get_mut(&dest)
            .and_then(|p| p.volatile_writer.as_mut())
        {
            w.add_change(
                None,
                Some(SerializedPayload {
                    kind: PayloadKind::Data,
                    bytes: m.to_payload(),
                }),
            );
        }
    }

    fn volatile_received(&self, src: GuidPrefix, m: GenericMessage, _now: Instant) {
        let me = Guid::new(self.prefix, EntityId::PARTICIPANT).to_bytes();
        if m.destination_participant_guid != me && m.destination_participant_guid != [0; 16] {
            return;
        }
        let mut g = self.state();
        let st = &mut *g;
        match m.message_class_id.as_str() {
            gm::PARTICIPANT_CRYPTO_TOKENS => {
                let Some(peer) = st.peers.get_mut(&src) else {
                    return;
                };
                if let Some(km) = key_materials(&m.message_data).into_iter().next() {
                    peer.participant = Some(Decoder::new(km));
                }
                self.check_ready(st, src);
            }
            gm::DATAWRITER_CRYPTO_TOKENS | gm::DATAREADER_CRYPTO_TOKENS => {
                let remote = Guid::from_bytes(m.source_endpoint_guid);
                if remote.prefix != src {
                    return;
                }
                let decoders = key_materials(&m.message_data)
                    .into_iter()
                    .filter(|k| k.kind != TransformKind::None)
                    .map(Decoder::new)
                    .collect();
                st.decoders.insert(remote, decoders);
            }
            _ => {}
        }
    }

    /// The protection of a local endpoint on `topic`: checks that the
    /// permissions allow it, and makes its keys. The security info to
    /// announce with it, and the payload encoder of a writer.
    pub(crate) fn local_endpoint(
        &self,
        data: &mut EndpointData,
        writer: bool,
    ) -> Result<Option<Arc<Encoder>>> {
        let topic = &data.topic_name;
        let rule = self
            .rule
            .topic_rule(topic)
            .map_err(sec_error("governance"))?
            .clone();
        let (action, controlled) = if writer {
            (Action::Publish, rule.enable_write_access_control)
        } else {
            (Action::Subscribe, rule.enable_read_access_control)
        };
        if controlled
            && !self
                .grant
                .allows(self.domain_id, action, topic, &data.partitions)
        {
            return Err(Error::Security(format!(
                "not allowed to {} {topic}",
                if writer { "publish" } else { "subscribe to" }
            )));
        }
        let m = rule.endpoint_masks(writer);
        data.security =
            parameters(|e| vec![Parameter::new(pid::ENDPOINT_SECURITY_INFO, masks(m, e))]);
        let submessage = rule.metadata_protection_kind.is_protected().then(|| {
            Encoder::new(KeyMaterial::generate(transform(
                rule.metadata_protection_kind,
            )))
        });
        let payload = (writer && rule.data_protection_kind.is_protected()).then(|| {
            Arc::new(Encoder::new(KeyMaterial::generate(transform(
                rule.data_protection_kind,
            ))))
        });
        let mut st = self.state();
        if let Some(p) = &payload {
            // Its own changes are read back by local late joiners.
            st.decoders
                .insert(data.guid, vec![Decoder::new(p.key_material().clone())]);
        }
        st.endpoints.insert(
            data.guid,
            LocalEndpoint {
                submessage,
                payload: payload.clone(),
                sent: HashMap::new(),
                origin_authentication: rule.metadata_protection_kind.origin_authenticated(),
            },
        );
        Ok(payload)
    }

    /// Whether the domain has discovery protection: the secure SEDP
    /// endpoints are used.
    pub(crate) fn discovery_protected(&self) -> bool {
        self.rule.discovery_protection_kind.is_protected()
    }

    /// Whether the domain has liveliness protection: the secure
    /// liveliness endpoints are used.
    pub(crate) fn liveliness_protected(&self) -> bool {
        self.rule.liveliness_protection_kind.is_protected()
    }

    /// Whether the endpoints of `topic` are announced on the secure SEDP
    /// endpoints.
    pub(crate) fn topic_discovery_protected(&self, topic: &str) -> bool {
        self.discovery_protected()
            && self
                .rule
                .topic_rule(topic)
                .is_ok_and(|r| r.enable_discovery_protection)
    }

    /// The builtin secure endpoints in use (7.5.8), but for the volatile
    /// ones: (local entity, remote counterpart, local is a writer, the
    /// counterpart's bit, protection).
    fn builtin_secure(&self) -> Vec<(EntityId, EntityId, bool, u32, ProtectionKind)> {
        let mut v = Vec::new();
        let d = self.rule.discovery_protection_kind;
        if d.is_protected() {
            v.extend([
                (
                    EntityId::SEDP_BUILTIN_PUBLICATIONS_SECURE_WRITER,
                    EntityId::SEDP_BUILTIN_PUBLICATIONS_SECURE_READER,
                    true,
                    BuiltinEndpointSet::PUBLICATIONS_SECURE_READER,
                    d,
                ),
                (
                    EntityId::SEDP_BUILTIN_PUBLICATIONS_SECURE_READER,
                    EntityId::SEDP_BUILTIN_PUBLICATIONS_SECURE_WRITER,
                    false,
                    BuiltinEndpointSet::PUBLICATIONS_SECURE_WRITER,
                    d,
                ),
                (
                    EntityId::SEDP_BUILTIN_SUBSCRIPTIONS_SECURE_WRITER,
                    EntityId::SEDP_BUILTIN_SUBSCRIPTIONS_SECURE_READER,
                    true,
                    BuiltinEndpointSet::SUBSCRIPTIONS_SECURE_READER,
                    d,
                ),
                (
                    EntityId::SEDP_BUILTIN_SUBSCRIPTIONS_SECURE_READER,
                    EntityId::SEDP_BUILTIN_SUBSCRIPTIONS_SECURE_WRITER,
                    false,
                    BuiltinEndpointSet::SUBSCRIPTIONS_SECURE_WRITER,
                    d,
                ),
            ]);
        }
        let l = self.rule.liveliness_protection_kind;
        if l.is_protected() {
            v.extend([
                (
                    EntityId::P2P_BUILTIN_PARTICIPANT_MESSAGE_SECURE_WRITER,
                    EntityId::P2P_BUILTIN_PARTICIPANT_MESSAGE_SECURE_READER,
                    true,
                    BuiltinEndpointSet::PARTICIPANT_MESSAGE_SECURE_READER,
                    l,
                ),
                (
                    EntityId::P2P_BUILTIN_PARTICIPANT_MESSAGE_SECURE_READER,
                    EntityId::P2P_BUILTIN_PARTICIPANT_MESSAGE_SECURE_WRITER,
                    false,
                    BuiltinEndpointSet::PARTICIPANT_MESSAGE_SECURE_WRITER,
                    l,
                ),
            ]);
        }
        v
    }

    /// Whether the payloads of `topic` are protected.
    pub(crate) fn payload_protected(&self, topic: &str) -> bool {
        self.rule
            .topic_rule(topic)
            .is_ok_and(|r| r.data_protection_kind.is_protected())
    }

    /// A local endpoint is gone.
    pub(crate) fn remove_local_endpoint(&self, guid: &Guid) {
        let mut st = self.state();
        st.endpoints.remove(guid);
        st.decoders.remove(guid);
    }

    /// Whether the permissions of the remote endpoint's participant let it
    /// publish (`writer`) or subscribe to its topic.
    pub(crate) fn remote_allowed(&self, remote: &EndpointData, writer: bool) -> bool {
        let Ok(rule) = self.rule.topic_rule(&remote.topic_name) else {
            return false;
        };
        let (action, controlled) = if writer {
            (Action::Publish, rule.enable_write_access_control)
        } else {
            (Action::Subscribe, rule.enable_read_access_control)
        };
        let st = self.state();
        match st.peers.get(&remote.guid.prefix) {
            Some(p) => match &p.grant {
                Some(g) => {
                    !controlled
                        || g.allows(
                            self.domain_id,
                            action,
                            &remote.topic_name,
                            &remote.partitions,
                        )
                }
                None => false,
            },
            // An unauthenticated participant: only unprotected topics.
            None => {
                self.rule.allow_unauthenticated_participants
                    && !controlled
                    && !rule.metadata_protection_kind.is_protected()
                    && !rule.data_protection_kind.is_protected()
            }
        }
    }

    /// A local endpoint matched a remote one: sends it the local
    /// endpoint's key material.
    pub(crate) fn matched(&self, local: Guid, remote: Guid, local_is_writer: bool) {
        let mut g = self.state();
        self.matched_locked(&mut g, local, remote, local_is_writer);
    }

    fn matched_locked(&self, st: &mut State, local: Guid, remote: Guid, local_is_writer: bool) {
        let Some(e) = st.endpoints.get_mut(&local) else {
            return;
        };
        let mut kms = Vec::new();
        if let Some(enc) = &e.submessage {
            let km = if e.origin_authentication {
                enc.key_material().with_receiver_specific_key()
            } else {
                enc.key_material().clone()
            };
            e.sent.insert(remote, km.clone());
            kms.push(km);
            st.protected_remote.insert(remote, true);
        }
        if let Some(enc) = &e.payload {
            kms.push(enc.key_material().clone());
        }
        if kms.is_empty() || !st.peers.contains_key(&remote.prefix) {
            return;
        }
        let tokens = kms.iter().map(crypto_token).collect();
        let class = if local_is_writer {
            gm::DATAWRITER_CRYPTO_TOKENS
        } else {
            gm::DATAREADER_CRYPTO_TOKENS
        };
        self.volatile_send(st, remote.prefix, class, Some(remote), Some(local), tokens);
    }

    /// A local endpoint no longer matches a remote one.
    pub(crate) fn unmatched(&self, local: Guid, remote: Guid) {
        let mut st = self.state();
        if let Some(e) = st.endpoints.get_mut(&local) {
            e.sent.remove(&remote);
        }
    }

    /// Decodes the payload of a change of `writer`, if it is protected.
    pub(crate) fn decode_payload(&self, writer: &Guid, payload: &[u8]) -> Option<Vec<u8>> {
        let header = crypto::header_of(payload).ok()?;
        let st = self.state();
        st.decoders
            .get(writer)?
            .iter()
            .find(|d| d.matches(&header))?
            .decode_payload(payload)
            .ok()
    }

    /// The datagrams due, and the peers that became ready.
    pub(crate) fn poll(&self, now: Instant) -> (Vec<Outgoing>, Vec<PeerEvent>) {
        let mut g = self.state();
        let st = &mut *g;
        let mut out = std::mem::take(&mut st.out);
        for peer in st.peers.values_mut() {
            if let Some((m, at, wait)) = &mut peer.last_sent {
                if *at <= now && peer.authenticated.is_none() {
                    out.extend(stateless_datagram(self.prefix, &peer.data, m));
                    *wait = (*wait * 2).min(MAX_RESEND);
                    *at = now + *wait;
                }
            }
            if let Some(w) = peer.volatile_writer.as_mut() {
                out.extend(w.poll(now));
                w.remove_acknowledged();
            }
            if let Some(r) = peer.volatile_reader.as_mut() {
                out.extend(r.poll(now));
            }
        }
        (out, std::mem::take(&mut st.events))
    }

    /// When [`Self::poll`] next has something to do.
    pub(crate) fn next_deadline(&self) -> Option<Instant> {
        let st = self.state();
        let mut next: Option<Instant> = None;
        let mut take = |t: Option<Instant>| {
            if let Some(t) = t {
                next = Some(next.map_or(t, |n| n.min(t)));
            }
        };
        if !st.out.is_empty() || !st.events.is_empty() {
            take(Some(Instant::now()));
        }
        for p in st.peers.values() {
            if p.authenticated.is_none() {
                take(p.last_sent.as_ref().map(|(_, at, _)| *at));
            }
            take(p.volatile_writer.as_ref().and_then(|w| w.next_deadline()));
            take(p.volatile_reader.as_ref().and_then(|r| r.next_deadline()));
        }
        next
    }

    /// Protects a message before it is sent: its submessages as their
    /// endpoints ask, then the whole message unless it is a bootstrapping
    /// one. `None` if it cannot be sent (no key for its destination).
    pub(crate) fn encode<'a>(&self, bytes: &'a [u8]) -> Option<Cow<'a, [u8]>> {
        if bytes.len() < RTPS_HEADER || &bytes[..4] != b"RTPS" {
            return Some(Cow::Borrowed(bytes));
        }
        let st = self.state();
        let mut out = bytes[..RTPS_HEADER].to_vec();
        let mut rest = &bytes[RTPS_HEADER..];
        let mut dest = GuidPrefix::UNKNOWN;
        let mut changed = false;
        let mut bootstrap = true;
        while !rest.is_empty() {
            let Ok((s, r)) = read_submessage(rest) else {
                break;
            };
            rest = r;
            if s.id == sub::INFO_DST && s.body.len() >= 12 {
                dest = GuidPrefix(s.body[..12].try_into().expect("12"));
            }
            let Some((reader, writer, from_writer)) = endpoints_of(s.id, s.body) else {
                out.extend_from_slice(s.whole);
                continue;
            };
            if !is_bootstrap(reader, writer) {
                bootstrap = false;
            }
            if is_volatile(reader, writer) {
                let (enc, _) = st.peers.get(&dest).and_then(|p| p.kx.as_ref())?;
                out.extend_from_slice(&enc.encode_submessage(s.whole, &[]));
                changed = true;
                continue;
            }
            let local = Guid::new(self.prefix, if from_writer { writer } else { reader });
            match st
                .endpoints
                .get(&local)
                .and_then(|e| Some((e, e.submessage.as_ref()?)))
            {
                Some((e, enc)) => {
                    let receivers: Vec<&KeyMaterial> = e
                        .sent
                        .iter()
                        .filter(|(g, _)| dest == GuidPrefix::UNKNOWN || g.prefix == dest)
                        .map(|(_, k)| k)
                        .collect();
                    out.extend_from_slice(&enc.encode_submessage(s.whole, &receivers));
                    changed = true;
                }
                None => out.extend_from_slice(s.whole),
            }
        }
        if bootstrap {
            return Some(if changed {
                Cow::Owned(out)
            } else {
                Cow::Borrowed(bytes)
            });
        }
        let Some(enc) = &self.participant else {
            return Some(if changed {
                Cow::Owned(out)
            } else {
                Cow::Borrowed(bytes)
            });
        };
        let receivers: Vec<&KeyMaterial> = st
            .peers
            .iter()
            .filter(|(p, _)| dest == GuidPrefix::UNKNOWN || **p == dest)
            .filter_map(|(_, p)| p.participant_sent.as_ref())
            .collect();
        enc.encode_message(&out, &receivers).ok().map(Cow::Owned)
    }

    /// Reverses [`Self::encode`] on a message received: `None` if it must
    /// be dropped. Submessages that should have come protected and did
    /// not are left out.
    pub(crate) fn decode<'a>(&self, bytes: &'a [u8]) -> Option<Cow<'a, [u8]>> {
        if bytes.len() < RTPS_HEADER || &bytes[..4] != b"RTPS" {
            return None;
        }
        let src = GuidPrefix(bytes[8..20].try_into().expect("12"));
        let st = self.state();
        let first = read_submessage(&bytes[RTPS_HEADER..]).ok()?.0;
        let plain: Cow<'a, [u8]> = if first.id == submessage_id::SRTPS_PREFIX {
            let header = crypto::header_of(first.body).ok()?;
            let dec = st.peers.get(&src)?.participant.as_ref()?;
            if !dec.matches(&header) {
                return None;
            }
            Cow::Owned(dec.decode_message(bytes).ok()?)
        } else {
            Cow::Borrowed(bytes)
        };
        let rtps_protected = matches!(plain, Cow::Owned(_));
        let peer = st.peers.get(&src);
        let mut out = plain[..RTPS_HEADER].to_vec();
        let mut rest = &plain[RTPS_HEADER..];
        let mut changed = rtps_protected;
        while !rest.is_empty() {
            let Ok((s, r)) = read_submessage(rest) else {
                break;
            };
            if s.id == submessage_id::SEC_PREFIX {
                let header = crypto::header_of(s.body).ok()?;
                let kx = peer
                    .and_then(|p| p.kx.as_ref())
                    .map(|(_, d)| d)
                    .filter(|d| d.matches(&header));
                let dec = kx.or_else(|| {
                    st.decoders
                        .iter()
                        .filter(|(g, _)| g.prefix == src)
                        .flat_map(|(_, ds)| ds.iter())
                        .find(|d| d.matches(&header))
                });
                changed = true;
                match dec.and_then(|d| d.decode_submessage(rest).ok()) {
                    Some(d) => {
                        out.extend_from_slice(&d.plain);
                        rest = &rest[d.rest_offset..];
                    }
                    None => {
                        // Skip to after the SEC_POSTFIX.
                        let mut at = r;
                        loop {
                            let Ok((s2, r2)) = read_submessage(at) else {
                                return None;
                            };
                            at = r2;
                            if s2.id == submessage_id::SEC_POSTFIX {
                                break;
                            }
                        }
                        rest = at;
                    }
                }
                continue;
            }
            rest = r;
            let Some((reader, writer, from_writer)) = endpoints_of(s.id, s.body) else {
                out.extend_from_slice(s.whole);
                continue;
            };
            let must = if is_volatile(reader, writer) {
                true
            } else if is_bootstrap(reader, writer) {
                false
            } else if self.participant.is_some() && !rtps_protected && src != self.prefix {
                true
            } else if from_writer {
                st.protected_remote.contains_key(&Guid::new(src, writer))
                    || st
                        .endpoints
                        .get(&Guid::new(self.prefix, reader))
                        .is_some_and(|e| e.submessage.is_some())
            } else {
                st.endpoints
                    .get(&Guid::new(self.prefix, writer))
                    .is_some_and(|e| e.submessage.is_some())
            };
            if must && src != self.prefix {
                changed = true;
                continue;
            }
            out.extend_from_slice(s.whole);
        }
        Some(if changed { Cow::Owned(out) } else { plain })
    }
}

/// The datagram of a stateless message: to the peer's metatraffic
/// unicast locators, or its multicast ones.
fn stateless_datagram(
    local: GuidPrefix,
    peer: &ParticipantData,
    m: &GenericMessage,
) -> Option<Outgoing> {
    let mut msg = Message::new(Header::new(VENDOR_ID, local));
    msg.submessages
        .push(Submessage::InfoDestination(InfoDestination {
            guid_prefix: peer.guid_prefix,
        }));
    msg.submessages.push(Submessage::Data(Data {
        reader_id: STATELESS_READER,
        writer_id: STATELESS_WRITER,
        writer_sn: SequenceNumber(m.message_identity.sequence_number),
        inline_qos: None,
        serialized_payload: Some(SerializedPayload {
            kind: PayloadKind::Data,
            bytes: m.to_payload(),
        }),
        non_standard_payload: false,
    }));
    let locators: Vec<Locator> = if peer.metatraffic_unicast_locators.is_empty() {
        peer.metatraffic_multicast_locators.clone()
    } else {
        peer.metatraffic_unicast_locators.clone()
    };
    Some(Outgoing {
        bytes: msg.encode(Endianness::NATIVE).ok()?,
        locators,
    })
}

/// Reports a security event. There is no logging facility: it goes to
/// standard error when `RUSTORB_DDS_SECURITY_LOG` is set.
fn log(s: &str) {
    if std::env::var_os("RUSTORB_DDS_SECURITY_LOG").is_some() {
        eprintln!("rustorb-dds security: {s}");
    }
}
