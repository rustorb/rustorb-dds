//! Byte order and the low-level cursor used by the message codec.

use crate::error::DecodeError;

/// The byte order of a submessage or of a CDR stream.
///
/// In a submessage, the EndiannessFlag (the least significant bit of the
/// flags) selects it: 0 is big-endian and 1 is little-endian
/// (DDSI-RTPS 2.5, 9.4.5.1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Endianness {
    /// Big-endian (EndiannessFlag = 0).
    Big,
    /// Little-endian (EndiannessFlag = 1).
    Little,
}

impl Endianness {
    /// The byte order of the machine this code runs on.
    pub const NATIVE: Endianness = if cfg!(target_endian = "big") {
        Endianness::Big
    } else {
        Endianness::Little
    };

    /// Reads the EndiannessFlag out of submessage flags.
    pub fn from_flags(flags: u8) -> Endianness {
        if flags & 0x01 != 0 {
            Endianness::Little
        } else {
            Endianness::Big
        }
    }

    /// The EndiannessFlag bit for this byte order.
    pub fn flag(self) -> u8 {
        match self {
            Endianness::Big => 0x00,
            Endianness::Little => 0x01,
        }
    }
}

/// A read cursor over a byte slice with a fixed byte order.
#[derive(Debug)]
pub(crate) struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
    endianness: Endianness,
}

impl<'a> Reader<'a> {
    pub(crate) fn new(buf: &'a [u8], endianness: Endianness) -> Self {
        Reader {
            buf,
            pos: 0,
            endianness,
        }
    }

    pub(crate) fn endianness(&self) -> Endianness {
        self.endianness
    }

    pub(crate) fn position(&self) -> usize {
        self.pos
    }

    pub(crate) fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub(crate) fn bytes(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        if n > self.remaining() {
            return Err(DecodeError::UnexpectedEnd);
        }
        let out = &self.buf[self.pos..self.pos + n];
        self.pos += n;
        Ok(out)
    }

    pub(crate) fn skip(&mut self, n: usize) -> Result<(), DecodeError> {
        self.bytes(n).map(|_| ())
    }

    pub(crate) fn rest(&mut self) -> &'a [u8] {
        let out = &self.buf[self.pos..];
        self.pos = self.buf.len();
        out
    }

    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let mut out = [0u8; N];
        out.copy_from_slice(self.bytes(N)?);
        Ok(out)
    }

    pub(crate) fn u16(&mut self) -> Result<u16, DecodeError> {
        let b = self.array::<2>()?;
        Ok(match self.endianness {
            Endianness::Big => u16::from_be_bytes(b),
            Endianness::Little => u16::from_le_bytes(b),
        })
    }

    pub(crate) fn u32(&mut self) -> Result<u32, DecodeError> {
        let b = self.array::<4>()?;
        Ok(match self.endianness {
            Endianness::Big => u32::from_be_bytes(b),
            Endianness::Little => u32::from_le_bytes(b),
        })
    }

    pub(crate) fn i32(&mut self) -> Result<i32, DecodeError> {
        self.u32().map(|v| v as i32)
    }
}

/// Writes integers in a fixed byte order to the end of a vector.
#[derive(Debug)]
pub(crate) struct Writer<'a> {
    buf: &'a mut Vec<u8>,
    endianness: Endianness,
}

impl<'a> Writer<'a> {
    pub(crate) fn new(buf: &'a mut Vec<u8>, endianness: Endianness) -> Self {
        Writer { buf, endianness }
    }

    pub(crate) fn endianness(&self) -> Endianness {
        self.endianness
    }

    pub(crate) fn len(&self) -> usize {
        self.buf.len()
    }

    pub(crate) fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    pub(crate) fn zeros(&mut self, n: usize) {
        self.buf.resize(self.buf.len() + n, 0);
    }

    /// Pads with zeros until the length is a multiple of 4.
    pub(crate) fn align4(&mut self) {
        let pad = (4 - self.buf.len() % 4) % 4;
        self.zeros(pad);
    }

    pub(crate) fn u16(&mut self, v: u16) {
        match self.endianness {
            Endianness::Big => self.bytes(&v.to_be_bytes()),
            Endianness::Little => self.bytes(&v.to_le_bytes()),
        }
    }

    /// Overwrites octets already written, starting at `at`.
    pub(crate) fn patch(&mut self, at: usize, b: &[u8]) {
        self.buf[at..at + b.len()].copy_from_slice(b);
    }

    pub(crate) fn u32(&mut self, v: u32) {
        match self.endianness {
            Endianness::Big => self.bytes(&v.to_be_bytes()),
            Endianness::Little => self.bytes(&v.to_le_bytes()),
        }
    }

    pub(crate) fn i32(&mut self, v: i32) {
        self.u32(v as u32);
    }
}
