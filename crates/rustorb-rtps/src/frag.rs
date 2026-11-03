//! Large data (DDSI-RTPS 2.5, 8.4.14.1): splitting a change into
//! DATA_FRAG submessages, and putting the fragments back together.
//!
//! A writer sends a change as DATA_FRAG only when its DATA would not fit
//! in a datagram ([`Fragmentation::needed`]). All its fragments have the
//! same size, [`Fragmentation::fragment_size`], whichever reader they go
//! to, and as many as fit go in one submessage. The in-line QoS goes with
//! the submessage that holds fragment 1 (8.3.8.3).
//!
//! A reader collects fragments in a [`Reassembler`], one for each writer,
//! and gets the change once every fragment is in. It holds at most
//! [`ReassemblyLimits::max_samples`] incomplete samples of at most
//! [`ReassemblyLimits::max_sample_size`] octets each.

use std::collections::{BTreeMap, BTreeSet};

use crate::parameter::ParameterList;
use crate::reliable::Change;
use crate::submessage::{DataFrag, PayloadKind, SerializedPayload};
use crate::types::{EntityId, FragmentNumber, FragmentNumberSet, Guid, SequenceNumber, Time};

/// The largest UDP payload over IPv4: 65535 octets less the IP and UDP
/// headers.
pub const MAX_UDP_PAYLOAD: usize = 65_507;

/// The RTPS header, INFO_DST and INFO_TS that may come before a DATA or
/// DATA_FRAG in a message.
pub(crate) const MESSAGE_OVERHEAD: usize = 20 + 16 + 12;

/// The submessage header and fixed fields of a DATA_FRAG.
const DATA_FRAG_HEADER: usize = 4 + 4 + 28;

/// The encoded length of an in-line QoS list, with its sentinel.
pub(crate) fn inline_qos_len(q: Option<&ParameterList>) -> usize {
    q.map_or(0, |q| {
        q.parameters
            .iter()
            .map(|p| 4 + p.value.len().next_multiple_of(4))
            .sum::<usize>()
            + 4
    })
}

/// The encoded length of the DATA submessage that carries `c`.
pub(crate) fn data_len(c: &Change) -> usize {
    24 + inline_qos_len(c.inline_qos.as_ref())
        + c.payload
            .as_ref()
            .map_or(0, |p| p.bytes.len().next_multiple_of(4))
}

/// How a writer fragments large changes (8.4.14.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fragmentation {
    /// The largest message the transport carries. A change whose DATA
    /// would not fit in a message of this size goes as DATA_FRAG.
    pub max_datagram_size: usize,
    /// The size of each fragment, the same for every reader. A
    /// DATA_FRAG holds as many fragments as fit in the writer's
    /// `max_message_size` (at least one).
    pub fragment_size: u16,
}

impl Default for Fragmentation {
    /// UDP's limit, and 4 KiB fragments.
    fn default() -> Self {
        Fragmentation {
            max_datagram_size: MAX_UDP_PAYLOAD,
            fragment_size: 4096,
        }
    }
}

impl Fragmentation {
    /// Whether `c` has to be fragmented: data is fragmented only when it
    /// does not fit otherwise (8.4.14.1).
    pub fn needed(&self, c: &Change) -> bool {
        c.payload.is_some() && MESSAGE_OVERHEAD + data_len(c) > self.max_datagram_size
    }

    /// How many fragments `c` has.
    pub fn count(&self, c: &Change) -> u32 {
        let size = c.payload.as_ref().map_or(0, |p| p.bytes.len());
        size.div_ceil(self.fragment_size.max(1) as usize) as u32
    }

    /// The DATA_FRAG submessages for the fragments `numbers` of `c`
    /// (sorted; all of them when `None`), with as many fragments in each
    /// as fit in a message of `max_message_size` octets.
    pub(crate) fn data_frags(
        &self,
        reader_id: EntityId,
        c: &Change,
        numbers: Option<&BTreeSet<u32>>,
        max_message_size: usize,
    ) -> Vec<DataFrag> {
        let Some(payload) = &c.payload else {
            return Vec::new();
        };
        let size = self.fragment_size.max(1) as usize;
        let total = self.count(c);
        let room = max_message_size
            .min(self.max_datagram_size)
            .saturating_sub(MESSAGE_OVERHEAD + DATA_FRAG_HEADER)
            .saturating_sub(inline_qos_len(c.inline_qos.as_ref()));
        let per = (room / size).clamp(1, u16::MAX as usize) as u32;
        let runs = match numbers {
            None => vec![(1, total)],
            Some(set) => runs(set.iter().copied().filter(|n| (1..=total).contains(n))),
        };
        let mut out = Vec::new();
        for (first, last) in runs {
            let mut n = first;
            while n <= last {
                let k = per.min(last - n + 1);
                let start = (n - 1) as usize * size;
                let end = ((n - 1 + k) as usize * size).min(payload.bytes.len());
                out.push(DataFrag {
                    key_flag: payload.kind == PayloadKind::Key,
                    non_standard_payload: false,
                    reader_id,
                    writer_id: c.writer.entity_id,
                    writer_sn: c.sequence_number,
                    fragment_starting_num: FragmentNumber(n),
                    fragments_in_submessage: k as u16,
                    fragment_size: self.fragment_size,
                    sample_size: payload.bytes.len() as u32,
                    inline_qos: if n == 1 { c.inline_qos.clone() } else { None },
                    serialized_payload: payload.bytes[start..end].to_vec(),
                });
                n += k;
            }
        }
        out
    }
}

/// Runs of consecutive numbers, as `(first, last)`.
fn runs(numbers: impl Iterator<Item = u32>) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for n in numbers {
        match out.last_mut() {
            Some((_, last)) if *last + 1 == n => *last = n,
            _ => out.push((n, n)),
        }
    }
    out
}

/// Limits on what a reader holds while putting samples together, for
/// each writer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReassemblyLimits {
    /// The largest sample accepted: fragments of larger ones are dropped.
    pub max_sample_size: u32,
    /// How many incomplete samples are kept.
    pub max_samples: usize,
}

impl Default for ReassemblyLimits {
    /// 64 MiB samples, 8 at a time.
    fn default() -> Self {
        ReassemblyLimits {
            max_sample_size: 64 * 1024 * 1024,
            max_samples: 8,
        }
    }
}

/// Which incomplete samples a [`Reassembler`] keeps when it has too many.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keep {
    /// The lowest sequence numbers, which a reliable reader delivers
    /// first. Fragments of a higher one are dropped, to be repaired later.
    Lowest,
    /// The highest sequence numbers, the newest, for a best-effort
    /// reader. The oldest incomplete sample is dropped.
    Highest,
}

#[derive(Debug)]
struct Partial {
    sample_size: u32,
    fragment_size: u16,
    kind: PayloadKind,
    data: Vec<u8>,
    received: Vec<bool>,
    missing: u32,
    inline_qos: Option<ParameterList>,
    timestamp: Option<Time>,
}

impl Partial {
    fn new(d: &DataFrag) -> Self {
        let total = d.sample_size.div_ceil(d.fragment_size as u32);
        Partial {
            sample_size: d.sample_size,
            fragment_size: d.fragment_size,
            kind: if d.key_flag {
                PayloadKind::Key
            } else {
                PayloadKind::Data
            },
            data: vec![0; d.sample_size as usize],
            received: vec![false; total as usize],
            missing: total,
            inline_qos: None,
            timestamp: None,
        }
    }

    fn fits(&self, d: &DataFrag) -> bool {
        self.sample_size == d.sample_size
            && self.fragment_size == d.fragment_size
            && (self.kind == PayloadKind::Key) == d.key_flag
    }

    /// Copies in the whole fragments that `d` holds.
    fn insert(&mut self, d: &DataFrag, timestamp: Option<Time>) {
        let size = self.fragment_size as usize;
        let start = d.fragment_starting_num.0;
        for k in 0..d.fragments_in_submessage as u32 {
            let n = start + k;
            let Some(have) = self.received.get_mut(n as usize - 1) else {
                break;
            };
            let offset = (n as usize - 1) * size;
            let len = size.min(self.sample_size as usize - offset);
            let from = k as usize * size;
            let Some(bytes) = d.serialized_payload.get(from..from + len) else {
                break;
            };
            if !*have {
                self.data[offset..offset + len].copy_from_slice(bytes);
                *have = true;
                self.missing -= 1;
            }
        }
        // The in-line QoS comes with fragment 1, and may come with others.
        if d.inline_qos.is_some() && (start == 1 || self.inline_qos.is_none()) {
            self.inline_qos = d.inline_qos.clone();
        }
        if self.timestamp.is_none() {
            self.timestamp = timestamp;
        }
    }

    /// The missing fragments up to `last`, from the first missing one,
    /// at most 256 of them (8.3.5.7).
    fn missing_up_to(&self, last: u32) -> Option<FragmentNumberSet> {
        let last = last.min(self.received.len() as u32);
        let first = (1..=last).find(|n| !self.received[*n as usize - 1])?;
        let end = last.min(first + 255);
        let mut set = FragmentNumberSet::new(FragmentNumber(first), end - first + 1)
            .expect("first >= 1, at most 256 bits");
        for n in first..=end {
            if !self.received[n as usize - 1] {
                set.insert(FragmentNumber(n));
            }
        }
        Some(set)
    }
}

/// The incomplete samples of one writer.
#[derive(Debug)]
pub struct Reassembler {
    limits: ReassemblyLimits,
    keep: Keep,
    partial: BTreeMap<SequenceNumber, Partial>,
}

impl Reassembler {
    /// An empty reassembler.
    pub fn new(limits: ReassemblyLimits, keep: Keep) -> Self {
        Reassembler {
            limits,
            keep,
            partial: BTreeMap::new(),
        }
    }

    /// Adds the fragments in `d` (from `writer`) and returns the change
    /// once all of its fragments are in. Fragments of a sample larger
    /// than the limit, or that disagree with earlier fragments of the
    /// same sample on its size or fragment size, are dropped.
    pub fn add(&mut self, writer: Guid, d: &DataFrag, timestamp: Option<Time>) -> Option<Change> {
        let sn = d.writer_sn;
        if d.sample_size > self.limits.max_sample_size || d.fragment_size == 0 {
            return None;
        }
        if !self.partial.contains_key(&sn) {
            if self.limits.max_samples == 0 {
                return None;
            }
            if self.partial.len() >= self.limits.max_samples {
                let evict = match self.keep {
                    Keep::Lowest => *self.partial.keys().next_back().expect("not empty"),
                    Keep::Highest => *self.partial.keys().next().expect("not empty"),
                };
                let keep_new = match self.keep {
                    Keep::Lowest => sn < evict,
                    Keep::Highest => sn > evict,
                };
                if !keep_new {
                    return None;
                }
                self.partial.remove(&evict);
            }
            self.partial.insert(sn, Partial::new(d));
        }
        let p = self.partial.get_mut(&sn).expect("inserted above");
        if !p.fits(d) {
            return None;
        }
        p.insert(d, timestamp);
        if p.missing > 0 {
            return None;
        }
        let p = self.partial.remove(&sn).expect("looked at above");
        Some(Change {
            writer,
            sequence_number: sn,
            inline_qos: p.inline_qos,
            payload: Some(SerializedPayload {
                kind: p.kind,
                bytes: p.data,
            }),
            timestamp: p.timestamp,
        })
    }

    /// Whether some fragments of `sn` are in, but not all.
    pub fn contains(&self, sn: SequenceNumber) -> bool {
        self.partial.contains_key(&sn)
    }

    /// Drops the incomplete sample `sn`.
    pub fn remove(&mut self, sn: SequenceNumber) {
        self.partial.remove(&sn);
    }

    /// Keeps only the incomplete samples for which `f` is true.
    pub fn retain(&mut self, mut f: impl FnMut(SequenceNumber) -> bool) {
        self.partial.retain(|sn, _| f(*sn));
    }

    /// The incomplete samples, in order.
    pub fn incomplete(&self) -> impl Iterator<Item = SequenceNumber> + '_ {
        self.partial.keys().copied()
    }

    /// Whether no sample is incomplete.
    pub fn is_empty(&self) -> bool {
        self.partial.is_empty()
    }

    /// The fragments of `sn` still missing, up to `last` (all when
    /// `None`), for a NACK_FRAG: at most 256 numbers from the first
    /// missing one. `None` when nothing of `sn` is in, or nothing is
    /// missing up to `last`.
    pub fn missing(
        &self,
        sn: SequenceNumber,
        last: Option<FragmentNumber>,
    ) -> Option<FragmentNumberSet> {
        let p = self.partial.get(&sn)?;
        p.missing_up_to(last.map_or(u32::MAX, |l| l.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{GuidPrefix, entity_kind};

    fn writer() -> Guid {
        Guid::new(
            GuidPrefix([1; 12]),
            EntityId::user([0, 0, 1], entity_kind::USER_WRITER_NO_KEY),
        )
    }

    fn change(sn: i64, size: usize) -> Change {
        let mut q = ParameterList::new();
        q.push(crate::parameter::Parameter::new(
            crate::parameter::ParameterId(0x0070),
            [7; 16],
        ));
        Change {
            writer: writer(),
            sequence_number: SequenceNumber(sn),
            inline_qos: Some(q),
            payload: Some(SerializedPayload {
                kind: PayloadKind::Data,
                bytes: (0..size).map(|i| (i % 251) as u8).collect(),
            }),
            timestamp: None,
        }
    }

    const F: Fragmentation = Fragmentation {
        max_datagram_size: 1000,
        fragment_size: 100,
    };

    #[test]
    fn only_what_does_not_fit_is_fragmented() {
        assert!(!F.needed(&change(1, 800)));
        assert!(F.needed(&change(1, 1000)));
        let mut c = change(1, 5000);
        c.payload = None;
        assert!(!F.needed(&c));
    }

    #[test]
    fn split_and_put_back_in_any_order() {
        let c = change(3, 1050);
        assert_eq!(F.count(&c), 11);
        // 4 fragments of 100 fit in 500 octets with the headers and QoS.
        let frags = F.data_frags(EntityId::UNKNOWN, &c, None, 500);
        let starts: Vec<u32> = frags.iter().map(|d| d.fragment_starting_num.0).collect();
        assert_eq!(starts, [1, 4, 7, 10]);
        assert!(frags[0].inline_qos.is_some());
        assert!(frags[1..].iter().all(|d| d.inline_qos.is_none()));
        assert_eq!(frags[3].serialized_payload.len(), 150);
        let mut r = Reassembler::new(ReassemblyLimits::default(), Keep::Lowest);
        for d in [&frags[2], &frags[0], &frags[3]] {
            assert_eq!(r.add(writer(), d, None), None);
        }
        assert!(r.contains(SequenceNumber(3)));
        let missing = r.missing(SequenceNumber(3), None).unwrap();
        assert_eq!(missing.iter().map(|n| n.0).collect::<Vec<_>>(), [4, 5, 6]);
        assert_eq!(r.add(writer(), &frags[1], None), Some(c));
        assert!(r.is_empty());
    }

    #[test]
    fn repairs_send_the_fragments_asked_for() {
        let c = change(1, 1050);
        let asked: BTreeSet<u32> = [2, 3, 5, 11, 12].into_iter().collect();
        let frags = F.data_frags(EntityId::UNKNOWN, &c, Some(&asked), 10_000);
        let got: Vec<(u32, u16)> = frags
            .iter()
            .map(|d| (d.fragment_starting_num.0, d.fragments_in_submessage))
            .collect();
        // 12 is past the end.
        assert_eq!(got, [(2, 2), (5, 1), (11, 1)]);
    }

    #[test]
    fn limits() {
        let limits = ReassemblyLimits {
            max_sample_size: 2000,
            max_samples: 2,
        };
        let first =
            |sn, size| F.data_frags(EntityId::UNKNOWN, &change(sn, size), None, 300)[0].clone();
        // Too large.
        let mut r = Reassembler::new(limits, Keep::Lowest);
        assert_eq!(r.add(writer(), &first(1, 3000), None), None);
        assert!(r.is_empty());
        // Reliable: the lowest are kept.
        for sn in [5, 3, 7, 1] {
            r.add(writer(), &first(sn, 1500), None);
        }
        assert_eq!(r.incomplete().map(|s| s.0).collect::<Vec<_>>(), [1, 3]);
        // Best effort: the newest are kept.
        let mut r = Reassembler::new(limits, Keep::Highest);
        for sn in [5, 3, 7, 1] {
            r.add(writer(), &first(sn, 1500), None);
        }
        assert_eq!(r.incomplete().map(|s| s.0).collect::<Vec<_>>(), [5, 7]);
        // A fragment that disagrees on the sample size is dropped.
        let mut other = first(7, 1600);
        other.fragment_starting_num = FragmentNumber(2);
        r.add(writer(), &other, None);
        assert_eq!(
            r.missing(SequenceNumber(7), None).unwrap().base(),
            FragmentNumber(2)
        );
    }

    #[test]
    fn missing_is_capped_at_256_and_at_last() {
        let f = Fragmentation {
            max_datagram_size: 1000,
            fragment_size: 10,
        };
        let c = change(1, 4000);
        let frags = f.data_frags(EntityId::UNKNOWN, &c, None, 100);
        let mut r = Reassembler::new(ReassemblyLimits::default(), Keep::Lowest);
        r.add(writer(), &frags[0], None);
        let m = r.missing(SequenceNumber(1), None).unwrap();
        assert_eq!(
            m.base().0 as usize,
            1 + frags[0].fragments_in_submessage as usize
        );
        assert_eq!(m.num_bits(), 256);
        let m = r
            .missing(SequenceNumber(1), Some(FragmentNumber(9)))
            .unwrap();
        assert_eq!(m.iter().last(), Some(FragmentNumber(9)));
        assert_eq!(r.missing(SequenceNumber(1), Some(FragmentNumber(1))), None);
        assert_eq!(r.missing(SequenceNumber(2), None), None);
    }
}
