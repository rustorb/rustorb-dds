//! The RTPS messages user writers send.

use rustorb_rtps::pl::pid;
use rustorb_rtps::{
    Data, Endianness, EntityId, GuidPrefix, Header, InfoTimestamp, Message, Parameter,
    ParameterList, PayloadKind, SequenceNumber, SerializedPayload, Submessage, Time,
};

use crate::participant::VENDOR_ID;

/// INFO_TS and a DATA from `writer_id` to any reader (8.4.8.1.4:
/// `readerId = ENTITYID_UNKNOWN`), with the key hash in-line for keyed
/// types (9.6.4.8).
pub(crate) fn data_message(
    prefix: GuidPrefix,
    writer_id: EntityId,
    sn: SequenceNumber,
    timestamp: Time,
    payload: Vec<u8>,
    key_hash: Option<[u8; 16]>,
) -> Vec<u8> {
    let mut msg = Message::new(Header::new(VENDOR_ID, prefix));
    msg.submessages
        .push(Submessage::InfoTimestamp(InfoTimestamp {
            timestamp: Some(timestamp),
        }));
    let inline_qos = key_hash.map(|h| {
        let mut q = ParameterList::new();
        q.push(Parameter::new(pid::KEY_HASH, h));
        q
    });
    msg.submessages.push(Submessage::Data(Data {
        reader_id: EntityId::UNKNOWN,
        writer_id,
        writer_sn: sn,
        inline_qos,
        serialized_payload: Some(SerializedPayload {
            kind: PayloadKind::Data,
            bytes: payload,
        }),
        non_standard_payload: false,
    }));
    msg.encode(Endianness::NATIVE)
        .expect("a single DATA is encodable")
}
