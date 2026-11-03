//! The QoS policies that endpoint discovery reads and matches on.
//!
//! The policies and their default values come from the DDS 1.4
//! specification (2.2.3 and the IDL in 2.3.3). How they are written in a
//! parameter list comes from DDSI-RTPS 2.5, 9.6.3 (Tables 9.18 and 9.19).
//!
//! Only RELIABILITY, DURABILITY and PARTITION are modelled for now
//! (rustorb-dds#6). Other policies sent by remote endpoints are skipped.

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

/// Whether an offered (writer) and a requested (reader) policy set are
/// compatible: "offered kind >= requested kind" for RELIABILITY
/// (BEST_EFFORT < RELIABLE) and DURABILITY (VOLATILE < TRANSIENT_LOCAL <
/// TRANSIENT < PERSISTENT) (DDS 1.4, 2.2.3).
pub fn compatible(
    offered: (ReliabilityKind, DurabilityKind),
    requested: (ReliabilityKind, DurabilityKind),
) -> bool {
    offered.0 >= requested.0 && offered.1 >= requested.1
}

/// Whether two PARTITION lists share a partition (DDS 1.4, 2.2.3.13). An
/// empty list means the default partition, `""`.
///
/// Names are compared as plain strings: the fnmatch wildcards of the DDS
/// specification are not supported yet.
pub fn partitions_match(a: &[String], b: &[String]) -> bool {
    let default = [String::new()];
    let a = if a.is_empty() { &default[..] } else { a };
    let b = if b.is_empty() { &default[..] } else { b };
    a.iter().any(|x| b.contains(x))
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
    fn matching_rules() {
        use DurabilityKind::*;
        use ReliabilityKind::*;
        assert!(compatible((Reliable, Volatile), (BestEffort, Volatile)));
        assert!(compatible((Reliable, TransientLocal), (Reliable, Volatile)));
        assert!(!compatible((BestEffort, Volatile), (Reliable, Volatile)));
        assert!(!compatible(
            (Reliable, Volatile),
            (Reliable, TransientLocal)
        ));

        let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(partitions_match(&[], &[]));
        assert!(partitions_match(&[], &s(&[""])));
        assert!(!partitions_match(&[], &s(&["a"])));
        assert!(partitions_match(&s(&["a", "b"]), &s(&["b"])));
    }
}
