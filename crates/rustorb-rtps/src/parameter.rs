//! Parameter lists (DDSI-RTPS 2.5, 8.3.5.9 and 9.4.2.11).
//!
//! Only the framing is handled here: each parameter's value is kept as raw
//! octets. Reading the values (PL_CDR) belongs to discovery (SPDP/SEDP).

use crate::error::{DecodeError, EncodeError};
use crate::wire::{Endianness, Reader, Writer};

/// A parameter id (DDSI-RTPS 2.5, 9.4.2.11.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ParameterId(pub u16);

impl ParameterId {
    /// `PID_PAD`: padding, skipped when reading.
    pub const PAD: ParameterId = ParameterId(0x0000);
    /// `PID_SENTINEL`: ends the list.
    pub const SENTINEL: ParameterId = ParameterId(0x0001);

    /// Whether the id is in the vendor-specific range (bit `0x8000`).
    pub const fn is_vendor_specific(self) -> bool {
        self.0 & 0x8000 != 0
    }

    /// Whether a receiver that does not know the id must treat it as an
    /// error rather than skip it (bit `0x4000`, Table 9.6).
    pub const fn must_understand(self) -> bool {
        self.0 & 0x4000 != 0
    }
}

/// One parameter: an id and its value.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Parameter {
    /// The parameter id.
    pub id: ParameterId,
    /// The value octets, including any trailing padding the sender put in
    /// the length.
    pub value: Vec<u8>,
}

impl Parameter {
    /// A parameter with the given id and value.
    pub fn new(id: ParameterId, value: impl Into<Vec<u8>>) -> Self {
        Parameter {
            id,
            value: value.into(),
        }
    }
}

/// A list of parameters.
///
/// `PID_PAD` entries are dropped when reading and never written. The
/// sentinel is not stored: it is written by [`ParameterList::encode`] when
/// asked for.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct ParameterList {
    /// The parameters in wire order. An id may appear more than once.
    pub parameters: Vec<Parameter>,
}

impl ParameterList {
    /// An empty list.
    pub fn new() -> Self {
        ParameterList::default()
    }

    /// The first parameter with the given id.
    pub fn get(&self, id: ParameterId) -> Option<&Parameter> {
        self.parameters.iter().find(|p| p.id == id)
    }

    /// All parameters with the given id, in order.
    pub fn get_all(&self, id: ParameterId) -> impl Iterator<Item = &Parameter> {
        self.parameters.iter().filter(move |p| p.id == id)
    }

    /// Appends a parameter.
    pub fn push(&mut self, parameter: Parameter) {
        self.parameters.push(parameter);
    }

    /// Reads a parameter list from the start of `bytes`.
    ///
    /// With `require_sentinel`, the list must end with `PID_SENTINEL`
    /// (every parameter list except the one in a HeaderExtension,
    /// 9.4.2.11.1); without it, the list may also end at the end of
    /// `bytes`. Returns the list and the number of octets read, including
    /// the sentinel.
    pub fn decode(
        bytes: &[u8],
        endianness: Endianness,
        require_sentinel: bool,
    ) -> Result<(ParameterList, usize), DecodeError> {
        let mut r = Reader::new(bytes, endianness);
        let list = Self::read(&mut r, require_sentinel)?;
        Ok((list, r.position()))
    }

    pub(crate) fn read(
        r: &mut Reader<'_>,
        require_sentinel: bool,
    ) -> Result<ParameterList, DecodeError> {
        let mut parameters = Vec::new();
        loop {
            if r.is_empty() && !require_sentinel {
                break;
            }
            let id = ParameterId(
                r.u16()
                    .map_err(|_| DecodeError::InvalidParameterList("missing PID_SENTINEL"))?,
            );
            let length = r
                .u16()
                .map_err(|_| DecodeError::InvalidParameterList("truncated parameter header"))?;
            if id == ParameterId::SENTINEL {
                // "its length is ignored" (9.4.2.11.1).
                break;
            }
            let value = r
                .bytes(length as usize)
                .map_err(|_| DecodeError::InvalidParameterList("parameter goes past the end"))?;
            if id != ParameterId::PAD {
                parameters.push(Parameter::new(id, value));
            }
        }
        Ok(ParameterList { parameters })
    }

    /// Writes the list to the end of `out`.
    ///
    /// Each value is padded with zeros to a multiple of four octets, so
    /// that every parameter starts on a 4-octet boundary (9.4.2.11.1).
    /// With `sentinel`, `PID_SENTINEL` is written at the end.
    pub fn encode(
        &self,
        out: &mut Vec<u8>,
        endianness: Endianness,
        sentinel: bool,
    ) -> Result<(), EncodeError> {
        self.write(&mut Writer::new(out, endianness), sentinel)
    }

    pub(crate) fn write(&self, w: &mut Writer<'_>, sentinel: bool) -> Result<(), EncodeError> {
        for p in &self.parameters {
            let padded = p.value.len().next_multiple_of(4);
            let length = u16::try_from(padded).map_err(|_| EncodeError::ParameterTooLong)?;
            w.u16(p.id.0);
            w.u16(length);
            w.bytes(&p.value);
            w.zeros(padded - p.value.len());
        }
        if sentinel {
            w.u16(ParameterId::SENTINEL.0);
            w.u16(0);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_both_endians() {
        let mut list = ParameterList::new();
        list.push(Parameter::new(ParameterId(0x0050), [1, 2, 3, 4]));
        list.push(Parameter::new(ParameterId(0x8001), [9; 8]));
        for e in [Endianness::Big, Endianness::Little] {
            let mut out = Vec::new();
            list.encode(&mut out, e, true).unwrap();
            assert_eq!(out.len(), 4 + 4 + 4 + 8 + 4);
            let (back, used) = ParameterList::decode(&out, e, true).unwrap();
            assert_eq!(back, list);
            assert_eq!(used, out.len());
        }
    }

    #[test]
    fn big_endian_layout() {
        let mut list = ParameterList::new();
        list.push(Parameter::new(ParameterId(0x0015), [2, 5]));
        let mut out = Vec::new();
        list.encode(&mut out, Endianness::Big, true).unwrap();
        assert_eq!(
            out,
            [0x00, 0x15, 0x00, 0x04, 2, 5, 0, 0, 0x00, 0x01, 0x00, 0x00]
        );
    }

    #[test]
    fn pad_is_skipped_and_sentinel_length_ignored() {
        let bytes = [
            0x00, 0x00, 0x00, 0x04, 0xaa, 0xbb, 0xcc, 0xdd, // PID_PAD
            0x00, 0x30, 0x00, 0x04, 1, 2, 3, 4, // 0x0030
            0x00, 0x01, 0x12, 0x34, // sentinel with a junk length
            0xff, 0xff, // after the list
        ];
        let (list, used) = ParameterList::decode(&bytes, Endianness::Big, true).unwrap();
        assert_eq!(used, 20);
        assert_eq!(
            list.parameters,
            [Parameter::new(ParameterId(0x30), [1, 2, 3, 4])]
        );
    }

    #[test]
    fn missing_sentinel() {
        let bytes = [0x30, 0x00, 0x04, 0x00, 1, 2, 3, 4];
        assert!(ParameterList::decode(&bytes, Endianness::Little, true).is_err());
        let (list, used) = ParameterList::decode(&bytes, Endianness::Little, false).unwrap();
        assert_eq!(used, 8);
        assert_eq!(list.parameters.len(), 1);
    }

    #[test]
    fn value_past_end() {
        let bytes = [0x00, 0x30, 0x00, 0x08, 1, 2, 3, 4];
        assert_eq!(
            ParameterList::decode(&bytes, Endianness::Big, false),
            Err(DecodeError::InvalidParameterList(
                "parameter goes past the end"
            ))
        );
    }

    #[test]
    fn id_bits() {
        assert!(ParameterId(0x8000).is_vendor_specific());
        assert!(!ParameterId(0x7fff).is_vendor_specific());
        assert!(ParameterId(0x4001).must_understand());
        assert!(!ParameterId(0x0001).must_understand());
    }
}
