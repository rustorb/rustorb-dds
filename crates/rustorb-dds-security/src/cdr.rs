//! Big-endian CDR, the little of it the plugins' types need.

use crate::Error;

/// Writes big-endian CDR.
#[derive(Debug, Default)]
pub struct CdrWriter {
    out: Vec<u8>,
}

impl CdrWriter {
    /// An empty stream.
    pub fn new() -> Self {
        Self::default()
    }

    fn align(&mut self, n: usize) {
        while self.out.len() % n != 0 {
            self.out.push(0);
        }
    }

    /// Octets as they are.
    pub fn octets(&mut self, b: &[u8]) {
        self.out.extend_from_slice(b);
    }

    /// An unsigned long.
    pub fn u32(&mut self, v: u32) {
        self.align(4);
        self.out.extend_from_slice(&v.to_be_bytes());
    }

    /// A long long.
    pub fn i64(&mut self, v: i64) {
        self.align(8);
        self.out.extend_from_slice(&v.to_be_bytes());
    }

    /// A sequence of octets.
    pub fn sequence(&mut self, b: &[u8]) {
        self.u32(b.len() as u32);
        self.octets(b);
    }

    /// A string (with its terminating NUL).
    pub fn string(&mut self, s: &str) {
        self.u32(s.len() as u32 + 1);
        self.octets(s.as_bytes());
        self.out.push(0);
    }

    /// The octets written.
    pub fn into_inner(self) -> Vec<u8> {
        self.out
    }
}

/// Reads big-endian (or little-endian) CDR.
#[derive(Debug)]
pub struct CdrReader<'a> {
    b: &'a [u8],
    at: usize,
    little: bool,
}

impl<'a> CdrReader<'a> {
    /// Big-endian CDR in `b`.
    pub fn new(b: &'a [u8]) -> Self {
        CdrReader {
            b,
            at: 0,
            little: false,
        }
    }

    /// CDR in `b`, little-endian if `little`.
    pub fn with_endian(b: &'a [u8], little: bool) -> Self {
        CdrReader { b, at: 0, little }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let s = self
            .b
            .get(self.at..self.at + n)
            .ok_or_else(|| Error::new("the CDR is cut short"))?;
        self.at += n;
        Ok(s)
    }

    fn align(&mut self, n: usize) {
        self.at = self.at.div_ceil(n) * n;
    }

    /// Octets.
    pub fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        Ok(self.take(N)?.try_into().expect("N"))
    }

    /// An unsigned long.
    pub fn u32(&mut self) -> Result<u32, Error> {
        self.align(4);
        let a = self.array()?;
        Ok(if self.little {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        })
    }

    /// A long long.
    pub fn i64(&mut self) -> Result<i64, Error> {
        self.align(8);
        let a = self.array()?;
        Ok(if self.little {
            i64::from_le_bytes(a)
        } else {
            i64::from_be_bytes(a)
        })
    }

    /// A sequence of octets of at most `max` (0: unbounded).
    pub fn sequence(&mut self, max: usize) -> Result<Vec<u8>, Error> {
        let n = self.u32()? as usize;
        if max != 0 && n > max {
            return Err(Error::new("a sequence is longer than its bound"));
        }
        Ok(self.take(n)?.to_vec())
    }

    /// A string.
    pub fn string(&mut self) -> Result<String, Error> {
        let n = self.u32()? as usize;
        let s = self.take(n)?;
        let s = s.strip_suffix(&[0]).unwrap_or(s);
        String::from_utf8(s.to_vec()).map_err(|_| Error::new("a string is not UTF-8"))
    }

    /// Whether everything was read.
    pub fn is_empty(&self) -> bool {
        self.at >= self.b.len()
    }
}
