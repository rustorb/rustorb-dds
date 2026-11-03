//! What the untyped participant needs to know about a topic type: how to
//! find the instance of a payload.

use std::marker::PhantomData;

use rustorb_typesupport::{Decoder, Encoder, TopicType};

use crate::instance::InstanceHandle;

/// Instance lookups for one topic type, behind a trait object.
pub(crate) trait KeyOps: Send + Sync {
    /// Whether the type has key members.
    fn keyed(&self) -> bool;

    /// The instance of a value payload, when it can be read.
    fn instance_of_value(&self, payload: &[u8]) -> Option<InstanceHandle>;

    /// Whether a key-only payload is the key of the value payload
    /// `value`.
    fn key_matches(&self, key_payload: &[u8], value: &[u8]) -> bool;
}

/// [`KeyOps`] for the type `T`.
pub(crate) struct Typed<T>(PhantomData<fn() -> T>);

impl<T> Typed<T> {
    pub(crate) fn new() -> Self {
        Typed(PhantomData)
    }
}

impl<T: TopicType> KeyOps for Typed<T> {
    fn keyed(&self) -> bool {
        T::KEYED
    }

    fn instance_of_value(&self, payload: &[u8]) -> Option<InstanceHandle> {
        if !T::KEYED {
            return Some(InstanceHandle::KEYLESS);
        }
        let v = T::from_payload(payload).ok()?;
        v.key_hash().ok().map(InstanceHandle)
    }

    /// A key payload is the key holder (DDS-XTypes 1.3, 7.6.8) in the
    /// encoding and byte order its encapsulation header says, which is
    /// how Cyclone DDS 0.10.4 sends a dispose without a key hash. It is
    /// compared with the key holder of `value` written the same way.
    fn key_matches(&self, key_payload: &[u8], value: &[u8]) -> bool {
        let Ok((dec, header)) = Decoder::xcdr_payload(key_payload) else {
            return false;
        };
        let Ok(v) = T::from_payload(value) else {
            return false;
        };
        let mut enc = Encoder::new(header.id.order, header.id.format.encoding());
        if T::write_key(&v, &mut enc).is_err() {
            return false;
        }
        let ours = enc.into_bytes();
        let body = dec.remaining_bytes();
        body.len() >= ours.len()
            && body[..ours.len()] == ours[..]
            && body[ours.len()..].iter().all(|b| *b == 0)
    }
}

/// The instance of `v`.
pub(crate) fn instance_of<T: TopicType>(v: &T) -> rustorb_typesupport::Result<InstanceHandle> {
    if T::KEYED {
        v.key_hash().map(InstanceHandle)
    } else {
        Ok(InstanceHandle::KEYLESS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustorb_typesupport::{ByteOrder, Encoding};

    use crate::test_shape::Shape;

    fn shape(id: u32) -> Shape {
        Shape {
            id,
            index: 7,
            message: "x".into(),
        }
    }

    #[test]
    fn key_payloads() {
        let ops = Typed::<Shape>::new();
        let value = shape(2)
            .to_payload(Encoding::Xcdr1, ByteOrder::Little)
            .unwrap();
        // What Cyclone DDS 0.10.4 sends with a dispose of id 2: CDR_LE and
        // the key holder.
        let cyclone = [0x00, 0x01, 0x00, 0x00, 2, 0, 0, 0];
        assert!(ops.key_matches(&cyclone, &value));
        assert!(!ops.key_matches(&[0x00, 0x01, 0x00, 0x00, 3, 0, 0, 0], &value));
        // Big endian, and XCDR2.
        assert!(ops.key_matches(&[0x00, 0x00, 0x00, 0x00, 0, 0, 0, 2], &value));
        assert!(ops.key_matches(&[0x00, 0x10, 0x00, 0x00, 0, 0, 0, 2], &value));
        assert!(!ops.key_matches(&[0x00, 0x01], &value));
        // The key hash: the key holder in big endian, padded to 16.
        let mut hash = [0u8; 16];
        hash[3] = 2;
        assert_eq!(ops.instance_of_value(&value), Some(InstanceHandle(hash)));
        assert_eq!(instance_of(&shape(2)).unwrap(), InstanceHandle(hash));
    }
}
