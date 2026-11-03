//! RTPS messages: the header followed by submessages
//! (DDSI-RTPS 2.5, 8.3.3, 8.3.4.1, 9.4.1, 9.4.4 and 9.4.5.1).

use crate::error::{DecodeError, EncodeError};
use crate::submessage::{Submessage, submessage_id};
use crate::types::{GuidPrefix, ProtocolVersion, VendorId};
use crate::wire::{Endianness, Reader, Writer};

/// `PROTOCOL_RTPS`.
pub const PROTOCOL_RTPS: [u8; 4] = *b"RTPS";

/// The fixed 20-octet header at the start of every message (9.4.4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Header {
    /// The protocol version of the sender.
    pub version: ProtocolVersion,
    /// The vendor of the sender.
    pub vendor_id: VendorId,
    /// The participant of the sender; the default prefix for the GUIDs in
    /// the message.
    pub guid_prefix: GuidPrefix,
}

impl Header {
    /// The size of the header on the wire.
    pub const SIZE: usize = 20;

    /// A header for this crate's protocol version (2.5).
    pub fn new(vendor_id: VendorId, guid_prefix: GuidPrefix) -> Self {
        Header {
            version: ProtocolVersion::CURRENT,
            vendor_id,
            guid_prefix,
        }
    }

    /// Reads a header from the start of `bytes`.
    ///
    /// Fails if the input is shorter than 20 octets, does not start with
    /// `RTPS`, or has a major version above 2 (8.3.6.3).
    pub fn decode(bytes: &[u8]) -> Result<Header, DecodeError> {
        // The header has no multi-octet integers, so the byte order does
        // not matter.
        let mut r = Reader::new(bytes, Endianness::Big);
        let protocol: [u8; 4] = r.array()?;
        if protocol != PROTOCOL_RTPS {
            return Err(DecodeError::NotRtps(protocol));
        }
        let version = ProtocolVersion::read(&mut r)?;
        if version.major > ProtocolVersion::CURRENT.major {
            return Err(DecodeError::UnsupportedVersion(version));
        }
        Ok(Header {
            version,
            vendor_id: VendorId::read(&mut r)?,
            guid_prefix: GuidPrefix::read(&mut r)?,
        })
    }

    fn write(&self, w: &mut Writer<'_>) {
        w.bytes(&PROTOCOL_RTPS);
        self.version.write(w);
        self.vendor_id.write(w);
        self.guid_prefix.write(w);
    }
}

/// A whole RTPS message, as carried in one UDP datagram (9.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    /// The header.
    pub header: Header,
    /// The submessages, in order.
    pub submessages: Vec<Submessage>,
}

impl Message {
    /// A message with no submessages yet.
    pub fn new(header: Header) -> Self {
        Message {
            header,
            submessages: Vec::new(),
        }
    }

    /// Reads a whole message and fails on the first problem.
    ///
    /// The receive rules of 8.3.4.1 say that the submessages before an
    /// invalid one are still valid. To act on those, use
    /// [`Message::parse`] instead.
    pub fn decode(bytes: &[u8]) -> Result<Message, DecodeError> {
        let (header, submessages) = Message::parse(bytes)?;
        Ok(Message {
            header,
            submessages: submessages.collect::<Result<_, _>>()?,
        })
    }

    /// Reads the header and returns an iterator over the submessages.
    ///
    /// The iterator follows 8.3.4.1: unknown submessage ids and unknown
    /// flags are skipped (unknown ids come out as
    /// [`Submessage::Unknown`]), and after the first error it yields that
    /// error and then stops, as the rest of the message is invalid.
    pub fn parse(bytes: &[u8]) -> Result<(Header, Submessages<'_>), DecodeError> {
        let header = Header::decode(bytes)?;
        Ok((
            header,
            Submessages {
                bytes,
                pos: Header::SIZE,
                index: 0,
                done: false,
            },
        ))
    }

    /// Writes the message with every submessage in byte order
    /// `endianness`.
    ///
    /// Each submessage but the last is padded with zeros so that the next
    /// one starts on a 4-octet boundary (9.4.1). The last one may be
    /// longer than 65535 octets; its `octetsToNextHeader` is then 0
    /// (9.4.5.1.3).
    pub fn encode(&self, endianness: Endianness) -> Result<Vec<u8>, EncodeError> {
        let mut out = Vec::new();
        self.encode_into(&mut out, endianness)?;
        Ok(out)
    }

    /// Like [`Message::encode`], appending to `out`.
    pub fn encode_into(
        &self,
        out: &mut Vec<u8>,
        endianness: Endianness,
    ) -> Result<(), EncodeError> {
        let mut w = Writer::new(out, endianness);
        self.header.write(&mut w);
        let last = self.submessages.len().saturating_sub(1);
        for (index, sub) in self.submessages.iter().enumerate() {
            let id = sub.id();
            if id == submessage_id::RTPS_HE && index != 0 {
                return Err(EncodeError::MisplacedHeaderExtension { index });
            }
            let header_at = w.len();
            w.zeros(4);
            let flags = sub.write_body(&mut w)?;
            if index != last {
                w.align4();
            }
            let body_len = w.len() - header_at - 4;
            let length = if body_len == 0 {
                let zero_means_empty = matches!(id, submessage_id::PAD | submessage_id::INFO_TS);
                if !zero_means_empty && index != last {
                    return Err(EncodeError::EmptySubmessage { index });
                }
                0
            } else {
                match u16::try_from(body_len) {
                    Ok(n) => n,
                    Err(_) if index == last && id != submessage_id::PAD => 0,
                    Err(_) => return Err(EncodeError::SubmessageTooLong { index }),
                }
            };
            // The length is in the byte order of the submessage, which for
            // an unknown submessage is the one it was received in.
            let len_bytes = match Endianness::from_flags(flags) {
                Endianness::Big => length.to_be_bytes(),
                Endianness::Little => length.to_le_bytes(),
            };
            w.patch(header_at, &[id, flags, len_bytes[0], len_bytes[1]]);
        }
        Ok(())
    }
}

/// Iterator over the submessages of a message; see [`Message::parse`].
#[derive(Debug)]
pub struct Submessages<'a> {
    bytes: &'a [u8],
    pos: usize,
    index: usize,
    done: bool,
}

impl Submessages<'_> {
    fn next_inner(&mut self) -> Result<Submessage, DecodeError> {
        let offset = self.pos;
        let rest = &self.bytes[offset..];
        // 8.3.4.1 rule 1.
        if rest.len() < 4 {
            return Err(DecodeError::BadSubmessageLength { offset });
        }
        let id = rest[0];
        let flags = rest[1];
        let raw = [rest[2], rest[3]];
        let length = match Endianness::from_flags(flags) {
            Endianness::Big => u16::from_be_bytes(raw),
            Endianness::Little => u16::from_le_bytes(raw),
        } as usize;
        let after_header = &rest[4..];
        // 9.4.5.1.3: 0 means "to the end of the message", except for PAD
        // and INFO_TS where it means an empty body.
        let body = if length == 0 {
            if matches!(id, submessage_id::PAD | submessage_id::INFO_TS) {
                &after_header[..0]
            } else {
                after_header
            }
        } else if length <= after_header.len() {
            &after_header[..length]
        } else {
            // 8.3.4.1 rule 2.
            return Err(DecodeError::BadSubmessageLength { offset });
        };
        self.pos = offset + 4 + body.len();
        // 8.3.7.3: a HeaderExtension must come right after the Header.
        if id == submessage_id::RTPS_HE && self.index != 0 {
            return Err(DecodeError::InvalidSubmessage {
                id,
                reason: "HeaderExtension does not follow the Header",
            });
        }
        self.index += 1;
        Submessage::decode_body(id, flags, body)
    }
}

impl Iterator for Submessages<'_> {
    type Item = Result<Submessage, DecodeError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done || self.pos >= self.bytes.len() {
            return None;
        }
        let item = self.next_inner();
        if item.is_err() {
            self.done = true;
        }
        Some(item)
    }
}

impl std::iter::FusedIterator for Submessages<'_> {}
