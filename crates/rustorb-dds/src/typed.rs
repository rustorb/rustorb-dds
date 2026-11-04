//! What the untyped participant needs to know about a topic type: how to
//! serialize its samples and find their instances. A type the program was
//! built with ([`TopicType`]) or one known at run time ([`DynamicType`]).

use std::marker::PhantomData;
use std::sync::Arc;

use rustorb_typesupport::{ByteOrder, Decoder, Encoder, Encoding, TopicType};
use rustorb_xtypes::dynamic::{DynamicData, DynamicType};

use crate::error::Result;
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

/// The samples of a topic, of Rust type `T`.
pub(crate) trait SampleType<T>: Send + Sync {
    /// The name of the type.
    fn type_name(&self) -> &str;

    /// Whether the type has key members.
    fn keyed(&self) -> bool;

    /// The RTPS SerializedPayload of `v`.
    fn to_payload(&self, v: &T, encoding: Encoding, order: ByteOrder) -> Result<Vec<u8>>;

    /// A sample read from a payload, when it can be read.
    fn decode(&self, payload: &[u8]) -> Option<T>;

    /// The instance of `v`.
    fn instance_of(&self, v: &T) -> Result<InstanceHandle>;

    /// The same type, for the participant's instance lookups.
    fn key_ops(&self) -> Arc<dyn KeyOps>;
}

/// [`KeyOps`] and [`SampleType`] for the type `T`.
pub(crate) struct Typed<T>(PhantomData<fn() -> T>);

impl<T> Typed<T> {
    pub(crate) fn new() -> Self {
        Typed(PhantomData)
    }
}

/// Whether the key payload `key_payload` (the key holder, DDS-XTypes 1.3,
/// 7.6.8, in the encoding and byte order its encapsulation header says,
/// which is how Cyclone DDS 0.10.4 sends a dispose without a key hash) is
/// the key holder written by `write_key` that way.
fn key_payload_matches(key_payload: &[u8], write_key: impl FnOnce(&mut Encoder) -> bool) -> bool {
    let Ok((dec, header)) = Decoder::xcdr_payload(key_payload) else {
        return false;
    };
    let mut enc = Encoder::new(header.id.order, header.id.format.encoding());
    if !write_key(&mut enc) {
        return false;
    }
    let ours = enc.into_bytes();
    let body = dec.remaining_bytes();
    body.len() >= ours.len()
        && body[..ours.len()] == ours[..]
        && body[ours.len()..].iter().all(|b| *b == 0)
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

    fn key_matches(&self, key_payload: &[u8], value: &[u8]) -> bool {
        let Ok(v) = T::from_payload(value) else {
            return false;
        };
        key_payload_matches(key_payload, |enc| T::write_key(&v, enc).is_ok())
    }
}

impl<T: TopicType + 'static> SampleType<T> for Typed<T> {
    fn type_name(&self) -> &str {
        T::TYPE_NAME
    }

    fn keyed(&self) -> bool {
        T::KEYED
    }

    fn to_payload(&self, v: &T, encoding: Encoding, order: ByteOrder) -> Result<Vec<u8>> {
        Ok(v.to_payload(encoding, order)?)
    }

    fn decode(&self, payload: &[u8]) -> Option<T> {
        T::from_payload(payload).ok()
    }

    fn instance_of(&self, v: &T) -> Result<InstanceHandle> {
        Ok(instance_of(v)?)
    }

    fn key_ops(&self) -> Arc<dyn KeyOps> {
        Arc::new(Typed::<T>::new())
    }
}

/// [`KeyOps`] and [`SampleType`] for a type known at run time, whose
/// samples are [`DynamicData`].
#[derive(Clone)]
pub(crate) struct Dynamic {
    ty: DynamicType,
    name: String,
    keyed: bool,
}

impl Dynamic {
    pub(crate) fn new(ty: &DynamicType) -> Self {
        Dynamic {
            name: ty.get_name(),
            keyed: ty.is_keyed(),
            ty: ty.clone(),
        }
    }
}

impl KeyOps for Dynamic {
    fn keyed(&self) -> bool {
        self.keyed
    }

    fn instance_of_value(&self, payload: &[u8]) -> Option<InstanceHandle> {
        if !self.keyed {
            return Some(InstanceHandle::KEYLESS);
        }
        let v = DynamicData::from_payload(&self.ty, payload).ok()?;
        v.key_hash().ok().map(InstanceHandle)
    }

    fn key_matches(&self, key_payload: &[u8], value: &[u8]) -> bool {
        let Ok(v) = DynamicData::from_payload(&self.ty, value) else {
            return false;
        };
        key_payload_matches(key_payload, |enc| v.serialize_key(enc).is_ok())
    }
}

impl SampleType<DynamicData> for Dynamic {
    fn type_name(&self) -> &str {
        &self.name
    }

    fn keyed(&self) -> bool {
        self.keyed
    }

    fn to_payload(&self, v: &DynamicData, encoding: Encoding, order: ByteOrder) -> Result<Vec<u8>> {
        if v.get_type() != self.ty {
            return Err(rustorb_xtypes::Error::BadParameter(
                "a sample of another type than the topic's".into(),
            )
            .into());
        }
        Ok(v.to_payload(encoding, order)?)
    }

    fn decode(&self, payload: &[u8]) -> Option<DynamicData> {
        DynamicData::from_payload(&self.ty, payload).ok()
    }

    fn instance_of(&self, v: &DynamicData) -> Result<InstanceHandle> {
        if !self.keyed {
            return Ok(InstanceHandle::KEYLESS);
        }
        Ok(InstanceHandle(v.key_hash()?))
    }

    fn key_ops(&self) -> Arc<dyn KeyOps> {
        Arc::new(self.clone())
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
