//! Property_t, BinaryProperty_t and DataHolder (Token) (DDS Security 1.2,
//! 7.3), with their CDR.

use crate::Error;
use crate::cdr::{CdrReader, CdrWriter};

/// Property_t: a name and a string value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Property {
    /// name.
    pub name: String,
    /// value.
    pub value: String,
    /// Whether discovery sends it (not serialized).
    pub propagate: bool,
}

/// BinaryProperty_t: a name and octets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BinaryProperty {
    /// name.
    pub name: String,
    /// value.
    pub value: Vec<u8>,
    /// Whether discovery sends it (not serialized).
    pub propagate: bool,
}

impl BinaryProperty {
    /// A property of `value`.
    pub fn new(name: &str, value: impl Into<Vec<u8>>) -> Self {
        BinaryProperty {
            name: name.into(),
            value: value.into(),
            propagate: true,
        }
    }

    /// A property holding `s` with its NUL (7.3.3.1).
    pub fn string(name: &str, s: &str) -> Self {
        let mut v = s.as_bytes().to_vec();
        v.push(0);
        Self::new(name, v)
    }
}

/// Writes a BinaryPropertySeq.
pub fn write_binary_properties(w: &mut CdrWriter, props: &[BinaryProperty]) {
    w.u32(props.len() as u32);
    for p in props {
        w.string(&p.name);
        w.sequence(&p.value);
    }
}

/// The big-endian CDR of a BinaryPropertySeq: what the handshake hashes
/// and signs (10.3.2.5).
pub fn binary_properties_cdr(props: &[BinaryProperty]) -> Vec<u8> {
    let mut w = CdrWriter::new();
    write_binary_properties(&mut w, props);
    w.into_inner()
}

/// DataHolder, the shape of every Token.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DataHolder {
    /// class_id.
    pub class_id: String,
    /// properties.
    pub properties: Vec<Property>,
    /// binary_properties.
    pub binary_properties: Vec<BinaryProperty>,
}

/// Token.
pub type Token = DataHolder;

impl DataHolder {
    /// An empty holder of `class_id`.
    pub fn new(class_id: &str) -> Self {
        DataHolder {
            class_id: class_id.into(),
            ..Default::default()
        }
    }

    /// Whether it is TokenNIL.
    pub fn is_nil(&self) -> bool {
        self.class_id.is_empty() && self.properties.is_empty() && self.binary_properties.is_empty()
    }

    /// Adds a property.
    pub fn with_property(mut self, name: &str, value: &str) -> Self {
        self.properties.push(Property {
            name: name.into(),
            value: value.into(),
            propagate: true,
        });
        self
    }

    /// Adds a binary property.
    pub fn with_binary(mut self, p: BinaryProperty) -> Self {
        self.binary_properties.push(p);
        self
    }

    /// The value of the property `name`.
    pub fn property(&self, name: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.value.as_str())
    }

    /// The value of the binary property `name`.
    pub fn binary(&self, name: &str) -> Option<&[u8]> {
        self.binary_properties
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.value.as_slice())
    }

    /// The binary property `name` as a string (its NUL dropped).
    pub fn binary_str(&self, name: &str) -> Option<&str> {
        let v = self.binary(name)?;
        std::str::from_utf8(v.strip_suffix(&[0]).unwrap_or(v)).ok()
    }

    /// Writes it (the properties that propagate).
    pub fn write(&self, w: &mut CdrWriter) {
        w.string(&self.class_id);
        let props: Vec<_> = self.properties.iter().filter(|p| p.propagate).collect();
        w.u32(props.len() as u32);
        for p in props {
            w.string(&p.name);
            w.string(&p.value);
        }
        let bins: Vec<_> = self
            .binary_properties
            .iter()
            .filter(|p| p.propagate)
            .cloned()
            .collect();
        write_binary_properties(w, &bins);
    }

    /// Reads one.
    pub fn read(r: &mut CdrReader<'_>) -> Result<Self, Error> {
        let class_id = r.string()?;
        let n = r.u32()? as usize;
        let mut properties = Vec::with_capacity(n.min(64));
        for _ in 0..n {
            properties.push(Property {
                name: r.string()?,
                value: r.string()?,
                propagate: true,
            });
        }
        let n = r.u32()? as usize;
        let mut binary_properties = Vec::with_capacity(n.min(64));
        for _ in 0..n {
            binary_properties.push(BinaryProperty {
                name: r.string()?,
                value: r.sequence(0)?,
                propagate: true,
            });
        }
        Ok(DataHolder {
            class_id,
            properties,
            binary_properties,
        })
    }

    /// Its big-endian CDR.
    pub fn to_cdr(&self) -> Vec<u8> {
        let mut w = CdrWriter::new();
        self.write(&mut w);
        w.into_inner()
    }

    /// Its CDR, little-endian if `little`.
    pub fn to_cdr_endian(&self, little: bool) -> Vec<u8> {
        let mut w = CdrWriter::with_endian(little);
        self.write(&mut w);
        w.into_inner()
    }

    /// From CDR, little-endian if `little`.
    pub fn from_cdr(b: &[u8], little: bool) -> Result<Self, Error> {
        Self::read(&mut CdrReader::with_endian(b, little))
    }
}

/// Writes a sequence of holders.
pub fn write_holders(w: &mut CdrWriter, hs: &[DataHolder]) {
    w.u32(hs.len() as u32);
    for h in hs {
        h.write(w);
    }
}

/// Reads a sequence of holders.
pub fn read_holders(r: &mut CdrReader<'_>) -> Result<Vec<DataHolder>, Error> {
    let n = r.u32()? as usize;
    (0..n.min(1024)).map(|_| DataHolder::read(r)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holders_round_trip() {
        let h = DataHolder::new("DDS:Auth:PKI-DH:1.0+Req")
            .with_property("dds.cert.sn", "CN=x")
            .with_binary(BinaryProperty::string("c.dsign_algo", "ECDSA-SHA256"))
            .with_binary(BinaryProperty::new("challenge1", vec![7; 32]));
        let b = h.to_cdr();
        assert_eq!(DataHolder::from_cdr(&b, false).unwrap(), h);
        assert_eq!(h.binary_str("c.dsign_algo"), Some("ECDSA-SHA256"));
        assert_eq!(h.binary("c.dsign_algo").unwrap().len(), 13);
    }
}
