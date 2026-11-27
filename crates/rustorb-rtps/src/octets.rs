//! [`Octets`]: a shared, immutable run of octets.
//!
//! A serialized payload goes from the application to the history of a
//! writer, into DATA_FRAGs, to local readers, and from a received datagram
//! to a reader. Kept as `Octets`, each of those steps shares the octets
//! instead of copying them (#112): a received DATA's payload is a slice of
//! the datagram, a DATA_FRAG's fragments are slices of the change.

use std::fmt;
use std::ops::{Deref, Range};
use std::sync::Arc;

/// A slice of a shared buffer. Cloning it, or taking a slice of it with
/// [`Octets::slice`], copies nothing.
#[derive(Clone)]
pub struct Octets {
    buf: Arc<Vec<u8>>,
    start: usize,
    end: usize,
}

impl Octets {
    /// No octets.
    pub fn new() -> Self {
        Octets::from(Vec::new())
    }

    /// The octets of `range` (relative to this slice), sharing the buffer.
    ///
    /// # Panics
    ///
    /// When `range` is not within the slice.
    pub fn slice(&self, range: Range<usize>) -> Octets {
        assert!(
            range.start <= range.end && range.end <= self.len(),
            "slice {range:?} out of {} octets",
            self.len()
        );
        Octets {
            buf: self.buf.clone(),
            start: self.start + range.start,
            end: self.start + range.end,
        }
    }

    /// `sub`, which must lie within these octets, as a slice of them;
    /// `None` when it does not.
    pub fn slice_ref(&self, sub: &[u8]) -> Option<Octets> {
        let base = self.as_ptr() as usize;
        let at = sub.as_ptr() as usize;
        if at < base || at + sub.len() > base + self.len() {
            return None;
        }
        let from = at - base;
        Some(self.slice(from..from + sub.len()))
    }

    /// The octets as a vector of their own: without a copy when nothing
    /// else shares them and they are the whole buffer.
    pub fn into_vec(self) -> Vec<u8> {
        if self.start == 0 && self.end == self.buf.len() {
            match Arc::try_unwrap(self.buf) {
                Ok(v) => v,
                Err(buf) => buf.to_vec(),
            }
        } else {
            self.to_vec()
        }
    }
}

impl Default for Octets {
    fn default() -> Self {
        Octets::new()
    }
}

impl Deref for Octets {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.buf[self.start..self.end]
    }
}

impl AsRef<[u8]> for Octets {
    fn as_ref(&self) -> &[u8] {
        self
    }
}

impl From<Vec<u8>> for Octets {
    fn from(v: Vec<u8>) -> Self {
        let end = v.len();
        Octets {
            buf: Arc::new(v),
            start: 0,
            end,
        }
    }
}

impl From<&[u8]> for Octets {
    fn from(s: &[u8]) -> Self {
        Octets::from(s.to_vec())
    }
}

impl<const N: usize> From<[u8; N]> for Octets {
    fn from(a: [u8; N]) -> Self {
        Octets::from(a.to_vec())
    }
}

impl FromIterator<u8> for Octets {
    fn from_iter<I: IntoIterator<Item = u8>>(iter: I) -> Self {
        Octets::from(iter.into_iter().collect::<Vec<u8>>())
    }
}

impl From<Octets> for Vec<u8> {
    fn from(o: Octets) -> Self {
        o.into_vec()
    }
}

impl fmt::Debug for Octets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl PartialEq for Octets {
    fn eq(&self, other: &Self) -> bool {
        **self == **other
    }
}

impl Eq for Octets {}

impl PartialEq<[u8]> for Octets {
    fn eq(&self, other: &[u8]) -> bool {
        **self == *other
    }
}

impl PartialEq<Vec<u8>> for Octets {
    fn eq(&self, other: &Vec<u8>) -> bool {
        **self == **other
    }
}

impl<const N: usize> PartialEq<[u8; N]> for Octets {
    fn eq(&self, other: &[u8; N]) -> bool {
        **self == *other
    }
}

impl std::hash::Hash for Octets {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (**self).hash(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_share_the_buffer() {
        let o = Octets::from(vec![1, 2, 3, 4, 5]);
        let s = o.slice(1..4);
        assert_eq!(&*s, &[2, 3, 4]);
        assert_eq!(s.as_ptr(), o[1..].as_ptr());
        let t = s.slice(1..2);
        assert_eq!(&*t, &[3]);
        assert_eq!(o.slice_ref(&o[2..5]).unwrap(), vec![3, 4, 5]);
        assert!(o.slice_ref(&[9, 9]).is_none());
        assert!(s.slice_ref(&o[0..2]).is_none());
    }

    #[test]
    fn into_vec_copies_only_when_shared_or_partial() {
        let v = vec![7u8; 16];
        let p = v.as_ptr();
        let o = Octets::from(v);
        assert_eq!(o.clone().into_vec(), vec![7u8; 16]);
        let back = o.into_vec();
        assert_eq!(back.as_ptr(), p);
        let o = Octets::from(vec![1, 2, 3]);
        assert_eq!(o.slice(1..3).into_vec(), vec![2, 3]);
    }
}
