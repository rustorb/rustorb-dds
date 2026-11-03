//! Decodes arbitrary input as an RTPS message.
//!
//! Run with `cargo +nightly fuzz run rtps_message` from the repository
//! root.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustorb_rtps::{Endianness, Message};

fuzz_target!(|data: &[u8]| {
    if let Ok((_, submessages)) = Message::parse(data) {
        for s in submessages {
            let _ = s;
        }
    }
    let Ok(m1) = Message::decode(data) else {
        return;
    };
    // Once decoded, encoding and decoding again must be stable.
    for e in [Endianness::Big, Endianness::Little] {
        let Ok(b2) = m1.encode(e) else {
            continue;
        };
        let Ok(m2) = Message::decode(&b2) else {
            continue;
        };
        let b3 = m2.encode(e).expect("re-encode");
        assert_eq!(Message::decode(&b3).expect("re-decode"), m2);
    }
});
