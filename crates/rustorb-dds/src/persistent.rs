//! The files in which PERSISTENT writers keep their samples (DDS 1.4,
//! 2.2.3.4), so that a later writer of the same topic and type takes
//! them up, also after a restart.
//!
//! A writer holds one file, locked while it is open: the writers of a
//! topic in one directory each take one, the first that no other holds.
//! A file starts with a header naming the topic and the type, followed
//! by one record for each change the writer made (a value, a dispose or
//! an unregister), appended as it is made. The file is written anew from
//! the history when it holds many more records than the history has
//! changes, and when it is taken up. A record cut short, by a crash
//! while it was appended, ends the file.
//!
//! All numbers are little endian.
//!
//! ```text
//! header: "RDDSPST1", u32 length + topic name, u32 length + type name
//! record: u32 length of the rest,
//!         u8 kind (0 value, 1 lifecycle), u8 flags (1 dispose, 2 unregister),
//!         16 octets instance handle, u32 seconds, u32 fraction,
//!         the value payload (kind 0)
//! ```

use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use rustorb_rtps::Time;

use crate::instance::InstanceHandle;

const MAGIC: &[u8; 8] = b"RDDSPST1";

/// The most files of one topic in a directory.
const MAX_FILES: usize = 10_000;

/// A change as kept in the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Record {
    pub(crate) instance: InstanceHandle,
    pub(crate) timestamp: Time,
    /// The value payload of a write; `None` for a dispose or an
    /// unregister.
    pub(crate) value: Option<Vec<u8>>,
    pub(crate) dispose: bool,
    pub(crate) unregister: bool,
}

impl Record {
    fn encode(&self, out: &mut Vec<u8>) {
        let value = self.value.as_deref().unwrap_or_default();
        let len = 2 + 16 + 8 + value.len();
        out.extend_from_slice(&(len as u32).to_le_bytes());
        out.push(u8::from(self.value.is_none()));
        out.push(u8::from(self.dispose) | (u8::from(self.unregister) << 1));
        out.extend_from_slice(&self.instance.0);
        out.extend_from_slice(&self.timestamp.seconds.to_le_bytes());
        out.extend_from_slice(&self.timestamp.fraction.to_le_bytes());
        out.extend_from_slice(value);
    }

    /// Reads a record; `None` at the end, or at a record cut short or
    /// not understood.
    fn decode(bytes: &mut &[u8]) -> Option<Record> {
        let len = u32_at(bytes, 0)? as usize;
        let body = bytes.get(4..4 + len)?;
        if len < 26 || body[0] > 1 {
            return None;
        }
        let r = Record {
            instance: InstanceHandle(body[2..18].try_into().ok()?),
            timestamp: Time {
                seconds: u32_at(body, 18)?,
                fraction: u32_at(body, 22)?,
            },
            value: (body[0] == 0).then(|| body[26..].to_vec()),
            dispose: body[1] & 1 != 0,
            unregister: body[1] & 2 != 0,
        };
        *bytes = &bytes[4 + len..];
        Some(r)
    }
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn string(b: &mut &[u8]) -> Option<String> {
    let len = u32_at(b, 0)? as usize;
    let s = std::str::from_utf8(b.get(4..4 + len)?).ok()?.to_owned();
    *b = &b[4 + len..];
    Some(s)
}

/// The file of a PERSISTENT writer.
#[derive(Debug)]
pub(crate) struct Store {
    file: File,
    path: PathBuf,
    header: Vec<u8>,
    /// The records in the file.
    records: usize,
}

/// The part of a file name taken from a topic name: its letters, digits,
/// `-` and `_`, the others as `_`. Topics whose names differ only there
/// share the names, and the header tells them apart.
fn file_stem(topic: &str) -> String {
    topic
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

impl Store {
    /// Takes the first file of `topic` and `type_name` in `dir` that no
    /// other writer holds, or a new one; returns it with the changes it
    /// keeps, oldest first.
    pub(crate) fn open(
        dir: &Path,
        topic: &str,
        type_name: &str,
    ) -> io::Result<(Store, Vec<Record>)> {
        std::fs::create_dir_all(dir)?;
        let mut header = MAGIC.to_vec();
        for s in [topic, type_name] {
            header.extend_from_slice(&(s.len() as u32).to_le_bytes());
            header.extend_from_slice(s.as_bytes());
        }
        let stem = file_stem(topic);
        for n in 0..MAX_FILES {
            let path = dir.join(format!("{stem}.{n}.rdds"));
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&path)?;
            match file.try_lock() {
                Ok(()) => {}
                Err(TryLockError::WouldBlock) => continue,
                Err(TryLockError::Error(e)) => return Err(e),
            }
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            let mut store = Store {
                file,
                path,
                header: header.clone(),
                records: 0,
            };
            if bytes.is_empty() {
                store.rewrite(&[])?;
                return Ok((store, Vec::new()));
            }
            let mut b = &bytes[..];
            if b.get(..8) != Some(&MAGIC[..]) {
                continue;
            }
            b = &b[8..];
            if string(&mut b).as_deref() != Some(topic)
                || string(&mut b).as_deref() != Some(type_name)
            {
                continue;
            }
            let mut records = Vec::new();
            while let Some(r) = Record::decode(&mut b) {
                records.push(r);
            }
            store.records = records.len();
            return Ok((store, records));
        }
        Err(io::Error::other(format!(
            "no free file for topic {topic:?} in {}",
            dir.display()
        )))
    }

    /// The file's path.
    #[allow(dead_code)]
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    /// How many records the file holds.
    pub(crate) fn records(&self) -> usize {
        self.records
    }

    /// Appends a change.
    pub(crate) fn append(&mut self, r: &Record) -> io::Result<()> {
        let mut bytes = Vec::new();
        r.encode(&mut bytes);
        self.file.seek(SeekFrom::End(0))?;
        self.file.write_all(&bytes)?;
        self.records += 1;
        Ok(())
    }

    /// Writes the file anew with these changes.
    pub(crate) fn rewrite<'a>(
        &mut self,
        records: impl IntoIterator<Item = &'a Record>,
    ) -> io::Result<()> {
        let mut bytes = self.header.clone();
        let mut n = 0;
        for r in records {
            r.encode(&mut bytes);
            n += 1;
        }
        self.file.set_len(0)?;
        self.file.seek(SeekFrom::Start(0))?;
        self.file.write_all(&bytes)?;
        self.records = n;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(i: u8, value: Option<&[u8]>) -> Record {
        Record {
            instance: InstanceHandle([i; 16]),
            timestamp: Time {
                seconds: 7,
                fraction: 9,
            },
            value: value.map(<[u8]>::to_vec),
            dispose: value.is_none(),
            unregister: false,
        }
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("rustorb-dds-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn records_survive_and_files_are_held() {
        let d = dir("store");
        let (mut a, kept) = Store::open(&d, "a/b", "T").unwrap();
        assert!(kept.is_empty());
        a.append(&record(1, Some(b"x"))).unwrap();
        a.append(&record(1, None)).unwrap();
        // A second writer of the topic takes another file.
        let (b, kept) = Store::open(&d, "a/b", "T").unwrap();
        assert!(kept.is_empty());
        assert_ne!(a.path(), b.path());
        drop(b);
        drop(a);
        let (mut a, kept) = Store::open(&d, "a/b", "T").unwrap();
        assert_eq!(kept, vec![record(1, Some(b"x")), record(1, None)]);
        a.rewrite(&kept[1..]).unwrap();
        // A cut record ends the file.
        a.file.write_all(&[9, 0, 0]).unwrap();
        drop(a);
        let (_a, kept) = Store::open(&d, "a/b", "T").unwrap();
        assert_eq!(kept, vec![record(1, None)]);
        // Another topic or type with the same file name is not taken up.
        let (_c, kept) = Store::open(&d, "a_b", "T").unwrap();
        assert!(kept.is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }
}
