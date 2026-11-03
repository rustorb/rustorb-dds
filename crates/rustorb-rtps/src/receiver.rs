//! The RTPS message receiver (DDSI-RTPS 2.5, 8.3.4): walks the
//! submessages of a message and keeps the receiver state that tells
//! entity submessages where they come from and who they are for.

use crate::message::{Header, Message};
use crate::submessage::Submessage;
use crate::types::{GuidPrefix, Time};

/// An entity submessage with the receiver state that applies to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Received {
    /// `sourceGuidPrefix`: the participant the submessage comes from.
    pub source_prefix: GuidPrefix,
    /// `timestamp` set by the last INFO_TS, if any.
    pub timestamp: Option<Time>,
    /// The submessage: one of DATA, DATA_FRAG, HEARTBEAT, HEARTBEAT_FRAG,
    /// GAP, ACKNACK, NACK_FRAG or an unknown one.
    pub submessage: Submessage,
}

/// Reads a message for the participant `local` and returns its entity
/// submessages.
///
/// The interpreter submessages update the state as in 8.3.4.1: INFO_SRC
/// sets the source, INFO_DST sets the destination and INFO_TS the time.
/// Entity submessages whose destination is neither `local` nor unknown
/// are left out. Reading stops at the first invalid submessage, and the
/// submessages before it are kept (8.3.4.1 rule 3 and 8.3.4.2). `None`
/// means the header itself is invalid.
pub fn receive(bytes: &[u8], local: GuidPrefix) -> Option<(Header, Vec<Received>)> {
    let (header, submessages) = Message::parse(bytes).ok()?;
    let mut source = header.guid_prefix;
    let mut dest_ok = true;
    let mut timestamp = None;
    let mut out = Vec::new();
    for sub in submessages {
        let Ok(sub) = sub else { break };
        match sub {
            Submessage::InfoSource(s) => {
                source = s.guid_prefix;
                // INFO_SRC also clears the time (8.3.8.10 "Change in
                // state of Receiver").
                timestamp = None;
            }
            Submessage::InfoDestination(d) => {
                dest_ok = d.guid_prefix == GuidPrefix::UNKNOWN || d.guid_prefix == local;
            }
            Submessage::InfoTimestamp(t) => timestamp = t.timestamp,
            Submessage::InfoReply(_)
            | Submessage::InfoReplyIp4(_)
            | Submessage::Pad(_)
            | Submessage::HeaderExtension(_) => {}
            other => {
                if dest_ok {
                    out.push(Received {
                        source_prefix: source,
                        timestamp,
                        submessage: other,
                    });
                }
            }
        }
    }
    Some((header, out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::submessage::{Gap, InfoDestination, InfoSource, InfoTimestamp};
    use crate::types::{EntityId, ProtocolVersion, SequenceNumber, SequenceNumberSet, VendorId};
    use crate::wire::Endianness;

    fn gap(n: i64) -> Submessage {
        Submessage::Gap(Gap {
            reader_id: EntityId::UNKNOWN,
            writer_id: EntityId::UNKNOWN,
            gap_start: SequenceNumber(n),
            gap_list: SequenceNumberSet::new(SequenceNumber(n + 1), 0).unwrap(),
            group_info: None,
            filtered_count: None,
        })
    }

    #[test]
    fn state() {
        let local = GuidPrefix([1; 12]);
        let mut m = Message::new(Header::new(VendorId([1, 1]), GuidPrefix([2; 12])));
        let ts = Time {
            seconds: 5,
            fraction: 0,
        };
        m.submessages.push(gap(1));
        m.submessages.push(Submessage::InfoTimestamp(InfoTimestamp {
            timestamp: Some(ts),
        }));
        m.submessages
            .push(Submessage::InfoDestination(InfoDestination {
                guid_prefix: GuidPrefix([3; 12]),
            }));
        m.submessages.push(gap(2));
        m.submessages
            .push(Submessage::InfoDestination(InfoDestination {
                guid_prefix: local,
            }));
        m.submessages.push(gap(3));
        m.submessages.push(Submessage::InfoSource(InfoSource {
            protocol_version: ProtocolVersion::CURRENT,
            vendor_id: VendorId([1, 1]),
            guid_prefix: GuidPrefix([4; 12]),
        }));
        m.submessages.push(gap(4));
        let bytes = m.encode(Endianness::Little).unwrap();
        let (h, r) = receive(&bytes, local).unwrap();
        assert_eq!(h.guid_prefix, GuidPrefix([2; 12]));
        assert_eq!(r.len(), 3);
        assert_eq!(r[0].submessage, gap(1));
        assert_eq!(r[0].timestamp, None);
        assert_eq!(r[1].submessage, gap(3));
        assert_eq!(r[1].timestamp, Some(ts));
        assert_eq!(r[1].source_prefix, GuidPrefix([2; 12]));
        assert_eq!(r[2].submessage, gap(4));
        assert_eq!(r[2].source_prefix, GuidPrefix([4; 12]));
        assert_eq!(r[2].timestamp, None);
        assert!(receive(b"RTPX", local).is_none());
    }
}
