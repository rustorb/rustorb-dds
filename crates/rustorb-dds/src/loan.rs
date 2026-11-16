//! Loaned samples (zero-copy): a writer fills a sample in shared memory
//! and readers of this host read it there, with no serialization and no
//! copy. See [`DataWriter::loan_sample`](crate::DataWriter::loan_sample)
//! and [`DataReader::take_loaned`](crate::DataReader::take_loaned).
// Without shared memory every loan is on the heap.
#![cfg_attr(not(feature = "shm"), allow(dead_code, unused_imports))]

use std::marker::PhantomData;
use std::ops::{Deref, DerefMut};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

/// A type whose samples can be loaned: one of a fixed size, whose value
/// in memory is its serialized form.
///
/// # Safety
///
/// The type must be plain data: `#[repr(C)]` (or a primitive, or an
/// array of such), with no pointers or references, valid when all its
/// octets are zero, and valid for any octets that a value of it holds,
/// since a reader of another process takes the octets of a writer's
/// sample as a value. Its layout must be its serialized form (after the
/// 4-octet encapsulation) in the writer's data representation and the
/// host's byte order: the members in order, each aligned as the
/// representation aligns it (to its size in XCDR1, to at most 4 in
/// XCDR2). A writer checks this on its first loaned sample, and copies
/// the samples it lends when it does not hold.
pub unsafe trait PlainType: Copy + Send + Sync + 'static {}

macro_rules! plain {
    ($($t:ty),*) => {
        $(
            // SAFETY: a primitive with every bit pattern valid.
            unsafe impl PlainType for $t {}
        )*
    };
}
plain!(u8, i8, u16, i16, u32, i32, u64, i64, f32, f64);

// SAFETY: an array of plain data is laid out as its elements, in order.
unsafe impl<T: PlainType, const N: usize> PlainType for [T; N] {}

/// A zero `T` on the heap (it may be too large for the stack).
pub(crate) fn zeroed_box<T: PlainType>() -> Box<T> {
    let layout = std::alloc::Layout::new::<T>();
    if layout.size() == 0 {
        // SAFETY: `PlainType` is valid as all zeros.
        return Box::new(unsafe { std::mem::zeroed() });
    }
    // SAFETY: a zeroed allocation of the layout of `T`, a valid `T`
    // (`PlainType`), owned by the box.
    unsafe {
        let p = std::alloc::alloc_zeroed(layout) as *mut T;
        if p.is_null() {
            std::alloc::handle_alloc_error(layout);
        }
        Box::from_raw(p)
    }
}

/// What a writer learnt of the layout of its type: the encapsulation of
/// a sample whose value in memory is its serialized form, and whether a
/// sample written showed that it is.
#[derive(Debug, Default)]
pub(crate) struct LoanCheck {
    /// The encapsulation header, if the serialized form of a zero sample
    /// is as long as the type plus the header.
    pub(crate) header: OnceLock<Option<[u8; 4]>>,
    /// Whether a sample showed that the layout is the serialized form
    /// (until then, loaned samples are serialized).
    verified: AtomicBool,
}

impl LoanCheck {
    pub(crate) fn verified(&self) -> bool {
        self.verified.load(Ordering::Relaxed)
    }

    pub(crate) fn set_verified(&self) {
        self.verified.store(true, Ordering::Relaxed);
    }
}

/// A sample loaned to a writer to fill: in shared memory when the writer
/// has a pool, else on the heap. It holds zeros, or (a slot used again)
/// an earlier sample of the writer; it is published
/// with [`DataWriter::write_loaned`](crate::DataWriter::write_loaned),
/// and returned unwritten when dropped.
pub struct WriteLoan<T: PlainType> {
    pub(crate) inner: WriteInner<T>,
}

pub(crate) enum WriteInner<T> {
    #[cfg(feature = "shm")]
    Shared(rustorb_rtps::shm::PoolSlot, PhantomData<T>),
    Heap(Box<T>),
}

impl<T: PlainType> WriteLoan<T> {
    pub(crate) fn heap() -> Self {
        WriteLoan {
            inner: WriteInner::Heap(zeroed_box()),
        }
    }

    /// The sample in a slot.
    #[cfg(feature = "shm")]
    pub(crate) fn value_of(slot: &rustorb_rtps::shm::PoolSlot) -> &T {
        // SAFETY: the slot holds a `T` after the header, aligned to 8
        // (`loan_sample` checks that `T` needs no more), held by the loan
        // alone; it holds zeros or an earlier `T`, valid either way.
        unsafe { &*(slot.bytes()[4..].as_ptr() as *const T) }
    }

    /// Whether the sample is in shared memory.
    pub fn is_shared(&self) -> bool {
        match &self.inner {
            #[cfg(feature = "shm")]
            WriteInner::Shared(..) => true,
            WriteInner::Heap(_) => false,
        }
    }
}

impl<T: PlainType> std::fmt::Debug for WriteLoan<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WriteLoan")
            .field("shared", &self.is_shared())
            .finish()
    }
}

impl<T: PlainType> Deref for WriteLoan<T> {
    type Target = T;
    fn deref(&self) -> &T {
        match &self.inner {
            #[cfg(feature = "shm")]
            WriteInner::Shared(slot, _) => Self::value_of(slot),
            WriteInner::Heap(b) => b,
        }
    }
}

impl<T: PlainType> DerefMut for WriteLoan<T> {
    fn deref_mut(&mut self) -> &mut T {
        match &mut self.inner {
            #[cfg(feature = "shm")]
            WriteInner::Shared(slot, _) => {
                // SAFETY: as in `value_of`.
                unsafe { &mut *(slot.bytes_mut()[4..].as_mut_ptr() as *mut T) }
            }
            WriteInner::Heap(b) => b,
        }
    }
}

/// A sample read without a copy: in the shared memory of the writer that
/// loaned it, held there until this is dropped, or a value of its own
/// (a sample that was serialized).
pub struct ReadLoan<T: PlainType> {
    inner: ReadInner<T>,
}

enum ReadInner<T> {
    #[cfg(feature = "shm")]
    Shared(rustorb_rtps::shm::PoolLoan, PhantomData<T>),
    /// Boxed: a `ReadLoan` is moved around, and `T` may be large.
    Owned(Box<T>),
}

impl<T: PlainType> ReadLoan<T> {
    pub(crate) fn owned(v: T) -> Self {
        ReadLoan {
            inner: ReadInner::Owned(Box::new(v)),
        }
    }

    /// The sample in a pool, if its octets are a `T` there: as long as a
    /// `T` plus the header, in the host's byte order, and aligned.
    #[cfg(feature = "shm")]
    pub(crate) fn shared(l: &rustorb_rtps::shm::PoolLoan) -> Option<Self> {
        let b = l.bytes();
        let native = u8::from(cfg!(target_endian = "little"));
        let fits = b.len() == 4 + std::mem::size_of::<T>()
            && b[0] == 0
            && b[1] & 1 == native
            && (b[4..].as_ptr() as usize).is_multiple_of(std::mem::align_of::<T>());
        fits.then_some(ReadLoan {
            inner: ReadInner::Shared(l.clone(), PhantomData),
        })
    }

    /// Whether the sample is read in place, in shared memory.
    pub fn is_shared(&self) -> bool {
        match &self.inner {
            #[cfg(feature = "shm")]
            ReadInner::Shared(..) => true,
            ReadInner::Owned(_) => false,
        }
    }
}

impl<T: PlainType> Deref for ReadLoan<T> {
    type Target = T;
    fn deref(&self) -> &T {
        match &self.inner {
            #[cfg(feature = "shm")]
            ReadInner::Shared(l, _) => {
                // SAFETY: checked in `shared`; the writer published the
                // slot and does not write it while it is held.
                unsafe { &*(l.bytes()[4..].as_ptr() as *const T) }
            }
            ReadInner::Owned(v) => v,
        }
    }
}

impl<T: PlainType + std::fmt::Debug> std::fmt::Debug for ReadLoan<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        (**self).fmt(f)
    }
}
