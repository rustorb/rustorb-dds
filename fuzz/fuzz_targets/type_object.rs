//! Decodes arbitrary input as a TypeObject and as a TypeInformation, in
//! both byte orders.
//!
//! Run with `cargo +nightly fuzz run type_object` from the repository
//! root.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustorb_cdr::ByteOrder;
use rustorb_xtypes::type_object::{TypeInformation, TypeObject};
use rustorb_xtypes::xcdr2::{from_bytes, to_bytes};

fuzz_target!(|data: &[u8]| {
    for order in [ByteOrder::Little, ByteOrder::Big] {
        // Once decoded, encoding and decoding again must be stable. The
        // octets are compared, not the values: a NaN annotation value is
        // not equal to itself.
        if let Ok(o1) = from_bytes::<TypeObject>(data, order) {
            let b2 = to_bytes(&o1, order).expect("encode");
            let o2: TypeObject = from_bytes(&b2, order).expect("re-decode");
            assert_eq!(to_bytes(&o2, order).expect("re-encode"), b2);
            let _ = o1.hash();
        }
        if let Ok(i1) = from_bytes::<TypeInformation>(data, order) {
            let b2 = to_bytes(&i1, order).expect("encode");
            let i2: TypeInformation = from_bytes(&b2, order).expect("re-decode");
            assert_eq!(i1, i2);
        }
    }
});
