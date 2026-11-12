//! ParticipantGenericMessage (7.3.21), the type of the stateless and
//! volatile secure messages.

use crate::Error;
use crate::cdr::{CdrReader, CdrWriter};
use crate::token::{DataHolder, read_holders, write_holders};

/// The class of the authentication request (7.5.3.5).
pub const AUTH_REQUEST: &str = "dds.sec.auth_request";
/// The class of the handshake messages.
pub const AUTH: &str = "dds.sec.auth";
/// The class of the participant crypto tokens (7.5.4.5).
pub const PARTICIPANT_CRYPTO_TOKENS: &str = "dds.sec.participant_crypto_tokens";
/// The class of a writer's crypto tokens.
pub const DATAWRITER_CRYPTO_TOKENS: &str = "dds.sec.datawriter_crypto_tokens";
/// The class of a reader's crypto tokens.
pub const DATAREADER_CRYPTO_TOKENS: &str = "dds.sec.datareader_crypto_tokens";

/// A message identity: the writer's GUID and a sequence number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct MessageIdentity {
    /// source_guid.
    pub source_guid: [u8; 16],
    /// sequence_number.
    pub sequence_number: i64,
}

/// A ParticipantGenericMessage.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GenericMessage {
    /// message_identity.
    pub message_identity: MessageIdentity,
    /// related_message_identity.
    pub related_message_identity: MessageIdentity,
    /// destination_participant_guid.
    pub destination_participant_guid: [u8; 16],
    /// destination_endpoint_guid.
    pub destination_endpoint_guid: [u8; 16],
    /// source_endpoint_guid.
    pub source_endpoint_guid: [u8; 16],
    /// message_class_id.
    pub message_class_id: String,
    /// message_data.
    pub message_data: Vec<DataHolder>,
}

impl GenericMessage {
    /// The serialized payload: a CDR_BE encapsulation of the message.
    pub fn to_payload(&self) -> Vec<u8> {
        let mut w = CdrWriter::new();
        for id in [&self.message_identity, &self.related_message_identity] {
            w.octets(&id.source_guid);
            w.i64(id.sequence_number);
        }
        w.octets(&self.destination_participant_guid);
        w.octets(&self.destination_endpoint_guid);
        w.octets(&self.source_endpoint_guid);
        w.string(&self.message_class_id);
        write_holders(&mut w, &self.message_data);
        let mut out = vec![0, 0, 0, 0];
        out.extend_from_slice(&w.into_inner());
        out
    }

    /// Reads a serialized payload (CDR_BE or CDR_LE).
    pub fn from_payload(b: &[u8]) -> Result<Self, Error> {
        if b.len() < 4 {
            return Err(Error::new("the message is cut short"));
        }
        let little = match [b[0], b[1]] {
            [0, 0] => false,
            [0, 1] => true,
            e => return Err(Error::new(format!("encapsulation {e:02x?}"))),
        };
        let mut r = CdrReader::with_endian(&b[4..], little);
        let mut id = || -> Result<MessageIdentity, Error> {
            Ok(MessageIdentity {
                source_guid: r.array()?,
                sequence_number: r.i64()?,
            })
        };
        let message_identity = id()?;
        let related_message_identity = id()?;
        Ok(GenericMessage {
            message_identity,
            related_message_identity,
            destination_participant_guid: r.array()?,
            destination_endpoint_guid: r.array()?,
            source_endpoint_guid: r.array()?,
            message_class_id: r.string()?,
            message_data: read_holders(&mut r)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let m = GenericMessage {
            message_identity: MessageIdentity {
                source_guid: [1; 16],
                sequence_number: 7,
            },
            destination_participant_guid: [2; 16],
            message_class_id: AUTH.into(),
            message_data: vec![DataHolder::new("x").with_property("a", "b")],
            ..Default::default()
        };
        let p = m.to_payload();
        assert_eq!(&p[..4], &[0, 0, 0, 0]);
        assert_eq!(GenericMessage::from_payload(&p).unwrap(), m);
    }
}
