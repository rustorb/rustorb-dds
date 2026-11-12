//! Stable `extern "C"` ABI of rustorb-dds.
//!
//! The C++ binding (`cpp/include/rustorb/dds`) and the Java binding
//! (through the FFM API) are built on this crate; the Python binding uses
//! PyO3 directly instead. The functions are declared, with the rules every
//! caller follows (handles, errors, strings, streams), in
//! `include/rustorb/dds.h`, whose names start with `rdds_`.
//!
//! - [`types`]: topic types, built the way DDS-XTypes 1.3 builds a
//!   `DynamicType` (7.5.2.2), member by member.
//! - [`stream`]: samples. The caller writes the values of a sample in the
//!   order of its type into an output stream, and reads a sample from an
//!   input stream in the same order; the library encodes and decodes them
//!   (XCDR1 or XCDR2, the key hash) knowing the type. Generated code
//!   therefore knows nothing of the encodings.
//! - [`entity`]: the DCPS entities, their QoS and statuses.
//! - [`condition`]: conditions and WaitSets.
//!
//! Failures return a DDS `ReturnCode_t` (DDS 1.4, 2.2.1.1) and leave a
//! message for the thread ([`rdds_last_error_message`]). A Rust panic is
//! caught at the boundary and returns `RETCODE_ERROR`.

// Every function takes raw pointers from C; what they must point to is
// documented once, in the header.
#![allow(clippy::missing_safety_doc)]
// The fields of the C structs are documented in the header too.
#![allow(missing_docs)]

pub mod condition;
pub mod entity;
pub mod rpc;
pub mod stream;
pub mod types;

use std::cell::RefCell;
use std::ffi::{CStr, CString, c_char};
use std::panic::{AssertUnwindSafe, catch_unwind};

/// `rdds_status`: a DDS `ReturnCode_t`.
pub type Status = i32;

/// `RETCODE_OK`.
pub const OK: Status = 0;
/// `RETCODE_ERROR`.
pub const ERROR: Status = 1;
/// `RETCODE_UNSUPPORTED`.
pub const UNSUPPORTED: Status = 2;
/// `RETCODE_BAD_PARAMETER`.
pub const BAD_PARAMETER: Status = 3;
/// `RETCODE_PRECONDITION_NOT_MET`.
pub const PRECONDITION_NOT_MET: Status = 4;
/// `RETCODE_OUT_OF_RESOURCES`.
pub const OUT_OF_RESOURCES: Status = 5;
/// `RETCODE_ALREADY_DELETED`.
pub const ALREADY_DELETED: Status = 9;
/// `RETCODE_TIMEOUT`.
pub const TIMEOUT: Status = 10;
/// `RETCODE_NO_DATA`.
pub const NO_DATA: Status = 11;

/// The version of the ABI in the header (`RDDS_ABI_VERSION`).
pub const ABI_VERSION: u32 = 1;

/// `rdds_abi_version`: the ABI version of the library, to compare with
/// `RDDS_ABI_VERSION` of the header the caller was built with.
#[unsafe(no_mangle)]
pub extern "C" fn rdds_abi_version() -> u32 {
    ABI_VERSION
}

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

/// `rdds_last_error_message`: the message of the last failure on this
/// thread, valid until the next failure on the thread.
#[unsafe(no_mangle)]
pub extern "C" fn rdds_last_error_message() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().as_ptr())
}

/// A failure: the code to return and the message to leave.
#[derive(Debug)]
pub(crate) struct Fail(pub Status, pub String);

pub(crate) type Res<T = ()> = std::result::Result<T, Fail>;

pub(crate) fn bad(msg: impl Into<String>) -> Fail {
    Fail(BAD_PARAMETER, msg.into())
}

impl From<rustorb_dds::Error> for Fail {
    fn from(e: rustorb_dds::Error) -> Fail {
        use rustorb_dds::Error as E;
        let code = match &e {
            E::Serialize(_) | E::Dynamic(_) => BAD_PARAMETER,
            E::AlreadyDeleted => ALREADY_DELETED,
            E::OutOfResources => OUT_OF_RESOURCES,
            E::Timeout => TIMEOUT,
            E::PreconditionNotMet => PRECONDITION_NOT_MET,
            E::Unsupported(_) => UNSUPPORTED,
            _ => ERROR,
        };
        Fail(code, e.to_string())
    }
}

impl From<rustorb_xtypes::Error> for Fail {
    fn from(e: rustorb_xtypes::Error) -> Fail {
        bad(e.to_string())
    }
}

fn set_error(msg: &str) {
    let c = CString::new(msg.replace('\0', " ")).unwrap_or_default();
    LAST_ERROR.with(|e| *e.borrow_mut() = c);
}

/// Runs `f`, turning its failure or panic into a status.
pub(crate) fn guard(f: impl FnOnce() -> Res) -> Status {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(Ok(())) => OK,
        Ok(Err(Fail(code, msg))) => {
            set_error(&msg);
            code
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".into());
            set_error(&format!("internal error: {msg}"));
            ERROR
        }
    }
}

/// Runs `f` for a value of a function that cannot fail, `default` after
/// a panic.
pub(crate) fn guard_value<T>(default: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(default)
}

/// The object a handle points to; a failure for NULL.
pub(crate) unsafe fn obj<'a, T>(p: *const T, what: &str) -> Res<&'a T> {
    // SAFETY: the caller passes NULL or a live handle (the header).
    unsafe { p.as_ref() }.ok_or_else(|| bad(format!("{what} is NULL")))
}

/// Stores `v` boxed in `*out`.
pub(crate) unsafe fn put<T>(out: *mut *mut T, v: T) -> Res {
    if out.is_null() {
        return Err(bad("the output pointer is NULL"));
    }
    // SAFETY: `out` is valid for a write (the header).
    unsafe { *out = Box::into_raw(Box::new(v)) };
    Ok(())
}

/// Writes `v` to `*out`.
pub(crate) unsafe fn set<T>(out: *mut T, v: T) -> Res {
    if out.is_null() {
        return Err(bad("the output pointer is NULL"));
    }
    // SAFETY: `out` is valid for a write (the header).
    unsafe { out.write(v) };
    Ok(())
}

/// A NUL-terminated UTF-8 string.
pub(crate) unsafe fn cstr<'a>(p: *const c_char, what: &str) -> Res<&'a str> {
    if p.is_null() {
        return Err(bad(format!("{what} is NULL")));
    }
    // SAFETY: a NUL-terminated string (the header).
    unsafe { CStr::from_ptr(p) }
        .to_str()
        .map_err(|_| bad(format!("{what} is not UTF-8")))
}

/// `n` items at `p` (NULL for none when `n` is 0).
pub(crate) unsafe fn slice<'a, T>(p: *const T, n: usize) -> Res<&'a [T]> {
    if n == 0 {
        return Ok(&[]);
    }
    if p.is_null() {
        return Err(bad("an array pointer is NULL"));
    }
    // SAFETY: `n` valid items (the header).
    Ok(unsafe { std::slice::from_raw_parts(p, n) })
}

/// Frees a handle made by [`put`].
pub(crate) unsafe fn release<T>(p: *mut T) {
    if !p.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            // SAFETY: a handle this library made, released once.
            drop(unsafe { Box::from_raw(p) })
        }));
    }
}
