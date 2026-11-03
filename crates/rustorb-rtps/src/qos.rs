//! The QoS policies that endpoint discovery reads and matches on.
//!
//! The policies and their default values come from the DDS 1.4
//! specification (2.2.3 and the IDL in 2.3.3). How they are written in a
//! parameter list comes from DDSI-RTPS 2.5, 9.6.3 (Tables 9.18 and 9.19).
//!
//! RELIABILITY, DURABILITY, DEADLINE, LIVELINESS and PARTITION are
//! modelled (rustorb-dds#6, rustorb-dds#19). Other policies sent by remote
//! endpoints are skipped.

use crate::error::DecodeError;
use crate::parameter::Parameter;
use crate::pl::{PlBuilder, PlValues, pid};
use crate::types::Duration;

/// `ReliabilityQosPolicyKind`. On the wire it is `ReliabilityKind_t`:
/// `BEST_EFFORT = 1`, `RELIABLE = 2` (DDSI-RTPS 2.5, 9.3.2 and Table 9.18
/// note 3), not the DDS enum ordinal.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ReliabilityKind {
    /// `BEST_EFFORT_RELIABILITY_QOS`.
    BestEffort,
    /// `RELIABLE_RELIABILITY_QOS`.
    Reliable,
}

/// `ReliabilityQosPolicy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Reliability {
    /// The kind.
    pub kind: ReliabilityKind,
    /// How long `write` may block when there is no room.
    pub max_blocking_time: Duration,
}

impl Reliability {
    /// The default `max_blocking_time`: 100 ms (DDS 1.4, 2.2.3).
    pub fn default_max_blocking_time() -> Duration {
        Duration::from_std(std::time::Duration::from_millis(100))
    }

    /// The default for DataWriters: RELIABLE (DDS 1.4, 2.2.3).
    pub fn writer_default() -> Self {
        Reliability {
            kind: ReliabilityKind::Reliable,
            max_blocking_time: Self::default_max_blocking_time(),
        }
    }

    /// The default for DataReaders and Topics: BEST_EFFORT (DDS 1.4,
    /// 2.2.3).
    pub fn reader_default() -> Self {
        Reliability {
            kind: ReliabilityKind::BestEffort,
            max_blocking_time: Self::default_max_blocking_time(),
        }
    }

    /// Reads `PID_RELIABILITY`: `{ long kind; Duration_t max_blocking_time; }`.
    ///
    /// Some senders write only the kind; the blocking time then takes its
    /// default. An unknown kind is an error.
    pub fn from_parameter(v: &PlValues, p: &Parameter) -> Result<Self, DecodeError> {
        let bad = DecodeError::InvalidParameterValue(p.id);
        let kind = match v.u32(p)? {
            1 => ReliabilityKind::BestEffort,
            2 => ReliabilityKind::Reliable,
            _ => return Err(bad),
        };
        let max_blocking_time = if p.value.len() >= 12 {
            let rest = Parameter::new(p.id, p.value[4..12].to_vec());
            v.duration(&rest)?
        } else {
            Self::default_max_blocking_time()
        };
        Ok(Reliability {
            kind,
            max_blocking_time,
        })
    }

    /// Writes `PID_RELIABILITY`.
    pub fn to_parameter(&self, b: &PlBuilder) -> Parameter {
        let kind: u32 = match self.kind {
            ReliabilityKind::BestEffort => 1,
            ReliabilityKind::Reliable => 2,
        };
        let mut value = b.u32(pid::RELIABILITY, kind).value;
        value.extend(b.duration(pid::RELIABILITY, self.max_blocking_time).value);
        Parameter::new(pid::RELIABILITY, value)
    }
}

/// `DurabilityQosPolicyKind`, in the order of the DDS enum, which is also
/// the order used to compare an offered and a requested kind (DDS 1.4,
/// 2.2.3.4).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DurabilityKind {
    /// `VOLATILE_DURABILITY_QOS`, the default.
    #[default]
    Volatile,
    /// `TRANSIENT_LOCAL_DURABILITY_QOS`.
    TransientLocal,
    /// `TRANSIENT_DURABILITY_QOS`.
    Transient,
    /// `PERSISTENT_DURABILITY_QOS`.
    Persistent,
}

impl DurabilityKind {
    /// Reads `PID_DURABILITY`: `{ DurabilityQosPolicyKind kind; }`, the
    /// enum being a 32-bit ordinal.
    pub fn from_parameter(v: &PlValues, p: &Parameter) -> Result<Self, DecodeError> {
        Ok(match v.u32(p)? {
            0 => DurabilityKind::Volatile,
            1 => DurabilityKind::TransientLocal,
            2 => DurabilityKind::Transient,
            3 => DurabilityKind::Persistent,
            _ => return Err(DecodeError::InvalidParameterValue(p.id)),
        })
    }

    /// Writes `PID_DURABILITY`.
    pub fn to_parameter(self, b: &PlBuilder) -> Parameter {
        b.u32(pid::DURABILITY, self as u32)
    }
}

/// `LivelinessQosPolicyKind`, in the order of the DDS enum, which is also
/// the order used to compare an offered and a requested kind (DDS 1.4,
/// 2.2.3.11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum LivelinessKind {
    /// `AUTOMATIC_LIVELINESS_QOS`, the default: the service asserts the
    /// liveliness of the writers of a participant that is running.
    #[default]
    Automatic,
    /// `MANUAL_BY_PARTICIPANT_LIVELINESS_QOS`: the application asserts
    /// it for all the writers of a participant.
    ManualByParticipant,
    /// `MANUAL_BY_TOPIC_LIVELINESS_QOS`: the application asserts it for
    /// each writer.
    ManualByTopic,
}

/// `LivelinessQosPolicy` (DDS 1.4, 2.2.3.11).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Liveliness {
    /// The kind.
    pub kind: LivelinessKind,
    /// How long an entity is alive without asserting it.
    pub lease_duration: Duration,
}

impl Default for Liveliness {
    /// AUTOMATIC, with an infinite lease (DDS 1.4, 2.2.3).
    fn default() -> Self {
        Liveliness {
            kind: LivelinessKind::Automatic,
            lease_duration: Duration::INFINITE,
        }
    }
}

impl Liveliness {
    /// Reads `PID_LIVELINESS`: `{ LivelinessQosPolicyKind kind;
    /// Duration_t lease_duration; }`, the enum being a 32-bit ordinal.
    pub fn from_parameter(v: &PlValues, p: &Parameter) -> Result<Self, DecodeError> {
        let kind = match v.u32(p)? {
            0 => LivelinessKind::Automatic,
            1 => LivelinessKind::ManualByParticipant,
            2 => LivelinessKind::ManualByTopic,
            _ => return Err(DecodeError::InvalidParameterValue(p.id)),
        };
        let lease_duration = if p.value.len() >= 12 {
            v.duration(&Parameter::new(p.id, p.value[4..12].to_vec()))?
        } else {
            Duration::INFINITE
        };
        Ok(Liveliness {
            kind,
            lease_duration,
        })
    }

    /// Writes `PID_LIVELINESS`.
    pub fn to_parameter(&self, b: &PlBuilder) -> Parameter {
        let mut value = b.u32(pid::LIVELINESS, self.kind as u32).value;
        value.extend(b.duration(pid::LIVELINESS, self.lease_duration).value);
        Parameter::new(pid::LIVELINESS, value)
    }
}

/// `QosPolicyId_t` (DDS 1.4, 2.3.3): which policy made a writer and a
/// reader incompatible.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct QosPolicyId(pub u32);

impl QosPolicyId {
    /// `DURABILITY_QOS_POLICY_ID`.
    pub const DURABILITY: QosPolicyId = QosPolicyId(2);
    /// `DEADLINE_QOS_POLICY_ID`.
    pub const DEADLINE: QosPolicyId = QosPolicyId(4);
    /// `LIVELINESS_QOS_POLICY_ID`.
    pub const LIVELINESS: QosPolicyId = QosPolicyId(8);
    /// `RELIABILITY_QOS_POLICY_ID`.
    pub const RELIABILITY: QosPolicyId = QosPolicyId(11);
}

/// The policies an offering writer and a requesting reader set.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RxO {
    /// RELIABILITY kind.
    pub reliability: ReliabilityKind,
    /// DURABILITY kind.
    pub durability: DurabilityKind,
    /// DEADLINE period.
    pub deadline: Duration,
    /// LIVELINESS.
    pub liveliness: Liveliness,
}

/// The policies for which what a writer offers does not satisfy what a
/// reader requests (the "requested/offered" rules of DDS 1.4, 2.2.3):
/// RELIABILITY and DURABILITY kinds, offered >= requested; DEADLINE,
/// offered period <= requested; LIVELINESS, offered kind >= requested and
/// offered lease <= requested. Empty when they are compatible.
pub fn incompatible_policies(offered: &RxO, requested: &RxO) -> Vec<QosPolicyId> {
    let mut out = Vec::new();
    if offered.durability < requested.durability {
        out.push(QosPolicyId::DURABILITY);
    }
    if offered.deadline > requested.deadline {
        out.push(QosPolicyId::DEADLINE);
    }
    if offered.liveliness.kind < requested.liveliness.kind
        || offered.liveliness.lease_duration > requested.liveliness.lease_duration
    {
        out.push(QosPolicyId::LIVELINESS);
    }
    if offered.reliability < requested.reliability {
        out.push(QosPolicyId::RELIABILITY);
    }
    out
}

/// Whether two PARTITION lists share a partition (DDS 1.4, 2.2.3.13). An
/// empty list means the default partition, `""`.
///
/// A name may hold the POSIX fnmatch wildcards `*`, `?` and `[...]`; it
/// then matches the names of the other side that fit it. Two names that
/// both hold wildcards never match each other, even when equal.
pub fn partitions_match(a: &[String], b: &[String]) -> bool {
    let default = [String::new()];
    let a = if a.is_empty() { &default[..] } else { a };
    let b = if b.is_empty() { &default[..] } else { b };
    a.iter()
        .any(|x| b.iter().any(|y| partition_names_match(x, y)))
}

fn partition_names_match(a: &str, b: &str) -> bool {
    match (has_wildcard(a), has_wildcard(b)) {
        (false, false) => a == b,
        (true, false) => fnmatch(a.as_bytes(), b.as_bytes()),
        (false, true) => fnmatch(b.as_bytes(), a.as_bytes()),
        (true, true) => false,
    }
}

fn has_wildcard(s: &str) -> bool {
    s.bytes().any(|c| matches!(c, b'*' | b'?' | b'['))
}

/// fnmatch without flags (POSIX.2 B.6): `*` matches any string, `?` any
/// character, `[...]` a set (with ranges and `!` for negation), `\`
/// quotes the next character. Works on octets.
fn fnmatch(pattern: &[u8], name: &[u8]) -> bool {
    match pattern.first() {
        None => name.is_empty(),
        Some(b'*') => {
            // Any number of characters, then the rest of the pattern.
            (0..=name.len()).any(|i| fnmatch(&pattern[1..], &name[i..]))
        }
        Some(b'?') => !name.is_empty() && fnmatch(&pattern[1..], &name[1..]),
        Some(b'[') => {
            let Some((&c, rest)) = name.split_first() else {
                return false;
            };
            match bracket(&pattern[1..], c) {
                Some((matched, after)) => matched && fnmatch(after, rest),
                // No closing bracket: `[` is an ordinary character.
                None => c == b'[' && fnmatch(&pattern[1..], rest),
            }
        }
        Some(b'\\') if pattern.len() > 1 => {
            name.first() == Some(&pattern[1]) && fnmatch(&pattern[2..], &name[1..])
        }
        Some(&p) => name.first() == Some(&p) && fnmatch(&pattern[1..], &name[1..]),
    }
}

/// Matches `c` against a bracket expression whose text starts at `p`
/// (after the `[`): whether it matched, and the pattern after the `]`.
/// `None` when there is no closing `]`.
fn bracket(p: &[u8], c: u8) -> Option<(bool, &[u8])> {
    let (negate, mut i) = match p.first() {
        Some(b'!') => (true, 1),
        _ => (false, 0),
    };
    let mut matched = false;
    let mut first = true;
    loop {
        let lo = *p.get(i)?;
        if lo == b']' && !first {
            return Some((matched != negate, &p[i + 1..]));
        }
        first = false;
        if p.get(i + 1) == Some(&b'-') && p.get(i + 2).is_some_and(|h| *h != b']') {
            let hi = p[i + 2];
            matched |= lo <= c && c <= hi;
            i += 3;
        } else {
            matched |= lo == c;
            i += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::Endianness;

    #[test]
    fn reliability_wire() {
        for e in [Endianness::Big, Endianness::Little] {
            let b = PlBuilder::new(e);
            let v = PlValues::new(e);
            for r in [Reliability::writer_default(), Reliability::reader_default()] {
                let p = r.to_parameter(&b);
                assert_eq!(p.id, pid::RELIABILITY);
                assert_eq!(p.value.len(), 12);
                assert_eq!(Reliability::from_parameter(&v, &p).unwrap(), r);
            }
        }
        let v = PlValues::new(Endianness::Little);
        // RELIABLE is 2 on the wire, not the DDS ordinal 1.
        let p = Parameter::new(pid::RELIABILITY, [2, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
        let r = Reliability::from_parameter(&v, &p).unwrap();
        assert_eq!(r.kind, ReliabilityKind::Reliable);
        assert_eq!(r.max_blocking_time, Duration::from_secs(1));
        // Kind only.
        let p = Parameter::new(pid::RELIABILITY, [1, 0, 0, 0]);
        assert_eq!(
            Reliability::from_parameter(&v, &p).unwrap(),
            Reliability::reader_default()
        );
        let p = Parameter::new(pid::RELIABILITY, [0, 0, 0, 0]);
        assert!(Reliability::from_parameter(&v, &p).is_err());
    }

    #[test]
    fn durability_wire() {
        let b = PlBuilder::new(Endianness::Big);
        let v = PlValues::new(Endianness::Big);
        for k in [
            DurabilityKind::Volatile,
            DurabilityKind::TransientLocal,
            DurabilityKind::Transient,
            DurabilityKind::Persistent,
        ] {
            assert_eq!(
                DurabilityKind::from_parameter(&v, &k.to_parameter(&b)).unwrap(),
                k
            );
        }
        assert_eq!(
            DurabilityKind::TransientLocal.to_parameter(&b).value,
            [0, 0, 0, 1]
        );
        let p = Parameter::new(pid::DURABILITY, [0, 0, 0, 4]);
        assert!(DurabilityKind::from_parameter(&v, &p).is_err());
    }

    #[test]
    fn liveliness_wire() {
        for e in [Endianness::Big, Endianness::Little] {
            let b = PlBuilder::new(e);
            let v = PlValues::new(e);
            for l in [
                Liveliness::default(),
                Liveliness {
                    kind: LivelinessKind::ManualByTopic,
                    lease_duration: Duration::from_secs(3),
                },
            ] {
                let p = l.to_parameter(&b);
                assert_eq!(p.value.len(), 12);
                assert_eq!(Liveliness::from_parameter(&v, &p).unwrap(), l);
            }
        }
        let v = PlValues::new(Endianness::Little);
        let p = Parameter::new(pid::LIVELINESS, [1, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(
            Liveliness::from_parameter(&v, &p).unwrap(),
            Liveliness {
                kind: LivelinessKind::ManualByParticipant,
                lease_duration: Duration::from_secs(2),
            }
        );
        let p = Parameter::new(pid::LIVELINESS, [3, 0, 0, 0]);
        assert!(Liveliness::from_parameter(&v, &p).is_err());
    }

    #[test]
    fn matching_rules() {
        use DurabilityKind::*;
        use ReliabilityKind::*;
        let rxo = |reliability, durability| RxO {
            reliability,
            durability,
            deadline: Duration::INFINITE,
            liveliness: Liveliness::default(),
        };
        let none: Vec<QosPolicyId> = Vec::new();
        assert_eq!(
            incompatible_policies(&rxo(Reliable, Volatile), &rxo(BestEffort, Volatile)),
            none
        );
        assert_eq!(
            incompatible_policies(&rxo(Reliable, TransientLocal), &rxo(Reliable, Volatile)),
            none
        );
        assert_eq!(
            incompatible_policies(&rxo(BestEffort, Volatile), &rxo(Reliable, TransientLocal)),
            [QosPolicyId::DURABILITY, QosPolicyId::RELIABILITY]
        );
        // DEADLINE: the writer must promise at least as often.
        let mut w = rxo(Reliable, Volatile);
        let mut r = rxo(Reliable, Volatile);
        r.deadline = Duration::from_secs(1);
        assert_eq!(incompatible_policies(&w, &r), [QosPolicyId::DEADLINE]);
        w.deadline = Duration::from_secs(1);
        assert_eq!(incompatible_policies(&w, &r), none);
        // LIVELINESS: kind and lease.
        r.liveliness = Liveliness {
            kind: LivelinessKind::ManualByParticipant,
            lease_duration: Duration::from_secs(5),
        };
        assert_eq!(incompatible_policies(&w, &r), [QosPolicyId::LIVELINESS]);
        w.liveliness = Liveliness {
            kind: LivelinessKind::ManualByTopic,
            lease_duration: Duration::from_secs(10),
        };
        assert_eq!(incompatible_policies(&w, &r), [QosPolicyId::LIVELINESS]);
        w.liveliness.lease_duration = Duration::from_secs(5);
        assert_eq!(incompatible_policies(&w, &r), none);
    }

    #[test]
    fn partitions() {
        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(partitions_match(&[], &[]));
        assert!(partitions_match(&[], &s(&[""])));
        assert!(!partitions_match(&[], &s(&["a"])));
        assert!(partitions_match(&s(&["a", "b"]), &s(&["b"])));
        // Wildcards on either side, not on both.
        assert!(partitions_match(&s(&["sensors/*"]), &s(&["sensors/front"])));
        assert!(partitions_match(&s(&["sensors/front"]), &s(&["sensors/*"])));
        assert!(!partitions_match(&s(&["sensors/*"]), &s(&["sensors/*"])));
        assert!(!partitions_match(&s(&["sensors/*"]), &s(&["robot/1"])));
        assert!(partitions_match(&s(&["*"]), &[]));
    }

    #[test]
    fn fnmatch_rules() {
        let m = |p: &str, n: &str| fnmatch(p.as_bytes(), n.as_bytes());
        assert!(m("*", ""));
        assert!(m("a*c", "abbbc"));
        assert!(!m("a*c", "abbbd"));
        assert!(m("a?c", "abc"));
        assert!(!m("a?c", "ac"));
        assert!(m("[abc]x", "bx"));
        assert!(!m("[abc]x", "dx"));
        assert!(m("[a-c]x", "bx"));
        assert!(m("[!a-c]x", "dx"));
        assert!(!m("[!a-c]x", "bx"));
        assert!(m("[]]", "]"));
        assert!(m("[a-]", "-"));
        assert!(m("\\*", "*"));
        assert!(!m("\\*", "a"));
        // No closing bracket: an ordinary `[`.
        assert!(m("[ab", "[ab"));
    }
}
