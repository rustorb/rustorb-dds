//! A shared memory transport for participants of rustorb-dds on the same
//! host (feature `shm`).
//!
//! Each participant has a receive segment: a file in a shared memory
//! directory (`/dev/shm` on Linux, else the temporary directory), mapped
//! by every process that sends to it, holding a ring buffer of RTPS
//! messages; and a UDP socket on the loopback interface, whose port the
//! segment gives, that senders ring after writing. Senders take turns through a
//! lock in the segment. A message that does not fit is dropped, like a
//! datagram lost on the way: the reliable protocol repairs it.
//!
//! The participant announces its segment with locators of the vendor
//! kind [`Locator::KIND_SHM`] (the address holds the host and the segment,
//! the port tells metatraffic from user traffic), besides its UDP ones.
//! Other implementations ignore them. A sender uses a peer's shared memory
//! locator when the host is its own and it can open the segment; else the
//! peer's UDP locators ([`ShmTransport::select`]).

use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use memmap2::MmapMut;

use crate::udp::{Channel, Datagram};
use crate::{GuidPrefix, Locator};

/// The port of a shared memory locator for metatraffic.
pub const PORT_METATRAFFIC: u32 = 1;
/// The port of a shared memory locator for user traffic.
pub const PORT_USER: u32 = 2;

/// "RSHM" and the layout version.
const MAGIC: u64 = 0x5253_484d_0000_0001;
/// The header: magic, capacity, lock, head, tail and the port of the
/// receiver's bell, each on its own word.
const HEADER: usize = 64;
const OFF_MAGIC: usize = 0;
const OFF_CAPACITY: usize = 8;
const OFF_LOCK: usize = 16;
const OFF_HEAD: usize = 24;
const OFF_TAIL: usize = 32;
const OFF_BELL: usize = 40;
/// A record: its length and port, then the message, padded to 8.
const RECORD_HEADER: usize = 8;
/// The length that marks the rest of the buffer as unused.
const WRAP: u32 = u32::MAX;
/// How often the receive thread looks at the stop flag.
const POLL: Duration = Duration::from_millis(100);
/// How long a sender waits for the lock before it takes it from a holder
/// that is gone.
const LOCK_PATIENCE: Duration = Duration::from_millis(500);

/// How the transport is set up.
#[derive(Clone, Debug)]
pub struct ShmConfig {
    /// The directory of the segments: `/dev/shm` where it exists, else
    /// the temporary directory.
    pub directory: PathBuf,
    /// The size of the receive buffer, in octets (default 4 MiB). A
    /// message larger than a quarter of it cannot be sent this way.
    pub capacity: usize,
}

impl Default for ShmConfig {
    fn default() -> Self {
        let shm = Path::new("/dev/shm");
        ShmConfig {
            directory: if shm.is_dir() {
                shm.to_path_buf()
            } else {
                std::env::temp_dir()
            },
            capacity: 4 << 20,
        }
    }
}

/// An identifier of this host: from the machine id where there is one,
/// else from the host name. Shared memory is tried only with peers of
/// the same one.
fn host_id() -> [u8; 8] {
    let text = ["/etc/machine-id", "/var/lib/dbus/machine-id"]
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .or_else(|| std::fs::read_to_string("/proc/sys/kernel/hostname").ok())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| std::env::var("COMPUTERNAME").ok())
        .unwrap_or_default();
    fnv(text.trim().as_bytes()).to_be_bytes()
}

/// FNV-1a, 64 bits.
fn fnv(b: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for x in b {
        h ^= u64::from(*x);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn segment_path(dir: &Path, segment: u64) -> PathBuf {
    dir.join(format!("rustorb-dds-{segment:016x}"))
}

/// A mapped segment.
struct Segment {
    map: MmapMut,
    capacity: usize,
}

// SAFETY: the shared words are accessed through atomics, the data under
// the lock (writers) or below `head` (the reader).
unsafe impl Send for Segment {}
unsafe impl Sync for Segment {}

impl Segment {
    fn create(path: &Path, capacity: usize, bell: u16) -> io::Result<Segment> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(path)?;
        file.set_len((HEADER + capacity) as u64)?;
        let s = Segment::map(&file, capacity)?;
        s.word(OFF_CAPACITY)
            .store(capacity as u64, Ordering::Relaxed);
        s.word(OFF_BELL).store(u64::from(bell), Ordering::Relaxed);
        s.word(OFF_MAGIC).store(MAGIC, Ordering::Release);
        Ok(s)
    }

    fn open(path: &Path) -> io::Result<Segment> {
        let file = OpenOptions::new().read(true).write(true).open(path)?;
        let len = file.metadata()?.len() as usize;
        if len < HEADER {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a segment"));
        }
        let s = Segment::map(&file, len - HEADER)?;
        if s.word(OFF_MAGIC).load(Ordering::Acquire) != MAGIC
            || s.word(OFF_CAPACITY).load(Ordering::Relaxed) != (len - HEADER) as u64
        {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "not a segment"));
        }
        Ok(s)
    }

    fn map(file: &File, capacity: usize) -> io::Result<Segment> {
        // SAFETY: the file is a segment of this transport; other
        // processes change it only through the protocol below.
        let map = unsafe { MmapMut::map_mut(file) }?;
        Ok(Segment { map, capacity })
    }

    fn word(&self, off: usize) -> &AtomicU64 {
        // SAFETY: the mapping is page aligned and HEADER long at least;
        // off is a multiple of 8 below HEADER.
        unsafe { &*(self.map.as_ptr().add(off) as *const AtomicU64) }
    }

    /// The receiver's bell.
    fn bell(&self) -> SocketAddr {
        let port = self.word(OFF_BELL).load(Ordering::Relaxed) as u16;
        SocketAddr::from((Ipv4Addr::LOCALHOST, port))
    }

    fn lock_word(&self) -> &AtomicU32 {
        // SAFETY: as in `word`.
        unsafe { &*(self.map.as_ptr().add(OFF_LOCK) as *const AtomicU32) }
    }

    fn data(&self) -> *mut u8 {
        // SAFETY: HEADER is within the mapping.
        unsafe { self.map.as_ptr().add(HEADER) as *mut u8 }
    }

    /// Takes the writers' lock; false if it cannot.
    fn lock(&self) -> bool {
        let me = std::process::id().max(1);
        let lock = self.lock_word();
        let start = Instant::now();
        let mut spins = 0u32;
        loop {
            let held = lock.load(Ordering::Relaxed);
            if held == 0 {
                if lock
                    .compare_exchange(0, me, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    return true;
                }
            } else if start.elapsed() > LOCK_PATIENCE {
                // A holder that is gone left it: on Linux, one with no
                // /proc/<pid>; elsewhere, one that kept it far longer
                // than a copy takes.
                let gone = if Path::new("/proc/self").exists() {
                    !Path::new(&format!("/proc/{held}")).exists()
                } else {
                    start.elapsed() > 4 * LOCK_PATIENCE
                };
                if gone
                    && lock
                        .compare_exchange(held, me, Ordering::Acquire, Ordering::Relaxed)
                        .is_ok()
                {
                    return true;
                }
                if start.elapsed() > 8 * LOCK_PATIENCE {
                    return false;
                }
            }
            spins += 1;
            if spins < 64 {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
    }

    fn unlock(&self) {
        self.lock_word().store(0, Ordering::Release);
    }

    /// Puts a message in the ring; false if there is no room.
    fn push(&self, port: u32, data: &[u8]) -> bool {
        let need = RECORD_HEADER + data.len().next_multiple_of(8);
        if need > self.capacity / 4 || !self.lock() {
            return false;
        }
        let head = self.word(OFF_HEAD).load(Ordering::Relaxed);
        let tail = self.word(OFF_TAIL).load(Ordering::Acquire);
        let cap = self.capacity as u64;
        let at = (head % cap) as usize;
        let to_end = self.capacity - at;
        let skip = if to_end < need { to_end } else { 0 };
        if (head - tail) as usize + skip + need > self.capacity {
            self.unlock();
            return false;
        }
        let p = self.data();
        // SAFETY: [at, at + to_end) and [0, need) are within the buffer,
        // free (below tail + capacity), and only this writer, holding the
        // lock, writes there.
        unsafe {
            let mut at = at;
            if skip > 0 {
                std::ptr::copy_nonoverlapping(WRAP.to_ne_bytes().as_ptr(), p.add(at), 4);
                at = 0;
            }
            let len = data.len() as u32;
            std::ptr::copy_nonoverlapping(len.to_ne_bytes().as_ptr(), p.add(at), 4);
            std::ptr::copy_nonoverlapping(port.to_ne_bytes().as_ptr(), p.add(at + 4), 4);
            std::ptr::copy_nonoverlapping(data.as_ptr(), p.add(at + RECORD_HEADER), data.len());
        }
        self.word(OFF_HEAD)
            .store(head + (skip + need) as u64, Ordering::Release);
        self.unlock();
        true
    }

    /// Takes the messages in the ring, with their ports.
    fn drain(&self, mut f: impl FnMut(u32, Vec<u8>)) {
        let head = self.word(OFF_HEAD).load(Ordering::Acquire);
        let mut tail = self.word(OFF_TAIL).load(Ordering::Relaxed);
        let cap = self.capacity as u64;
        let p = self.data();
        while tail < head {
            let at = (tail % cap) as usize;
            let mut w = [0u8; 4];
            // SAFETY: records below head are complete (Release/Acquire on
            // head) and nobody writes them until tail passes them.
            unsafe { std::ptr::copy_nonoverlapping(p.add(at), w.as_mut_ptr(), 4) };
            let len = u32::from_ne_bytes(w);
            if len == WRAP {
                tail += (self.capacity - at) as u64;
                continue;
            }
            let mut port = [0u8; 4];
            let mut data = vec![0u8; len as usize];
            // SAFETY: as above.
            unsafe {
                std::ptr::copy_nonoverlapping(p.add(at + 4), port.as_mut_ptr(), 4);
                std::ptr::copy_nonoverlapping(
                    p.add(at + RECORD_HEADER),
                    data.as_mut_ptr(),
                    data.len(),
                );
            }
            tail += (RECORD_HEADER + (len as usize).next_multiple_of(8)) as u64;
            f(u32::from_ne_bytes(port), data);
        }
        self.word(OFF_TAIL).store(tail, Ordering::Release);
    }
}

/// A peer's segment, opened on first use.
struct Peer {
    segment: Segment,
}

/// The shared memory transport of one participant.
pub struct ShmTransport {
    host: [u8; 8],
    segment_id: u64,
    directory: PathBuf,
    path: PathBuf,
    /// Peers by segment; `None` for one that could not be opened.
    peers: Mutex<HashMap<u64, Option<Arc<Peer>>>>,
    /// Rings the peers' bells.
    bell: UdpSocket,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for ShmTransport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShmTransport")
            .field("segment", &self.path)
            .finish()
    }
}

impl ShmTransport {
    /// Creates the receive segment and socket of the participant `prefix`
    /// and starts receiving: the messages go to `tx`, as datagrams of the
    /// unicast channels.
    pub fn open(
        config: &ShmConfig,
        prefix: GuidPrefix,
        tx: mpsc::Sender<Datagram>,
    ) -> io::Result<ShmTransport> {
        let segment_id = fnv(&prefix.0) ^ u64::from(std::process::id());
        let path = segment_path(&config.directory, segment_id);
        let _ = std::fs::remove_file(&path);
        let socket = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
        socket.set_read_timeout(Some(POLL))?;
        let port = socket.local_addr()?.port();
        let segment = Arc::new(Segment::create(&path, config.capacity, port)?);
        let bell = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))?;
        bell.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("rtps-shm".into())
                .spawn(move || receive_loop(&segment, &socket, &stop, &tx))?
        };
        Ok(ShmTransport {
            host: host_id(),
            segment_id,
            directory: config.directory.clone(),
            path,
            peers: Mutex::new(HashMap::new()),
            bell,
            stop,
            thread: Some(thread),
        })
    }

    fn locator(&self, port: u32) -> Locator {
        let mut address = [0u8; 16];
        address[..8].copy_from_slice(&self.host);
        address[8..].copy_from_slice(&self.segment_id.to_be_bytes());
        Locator {
            kind: Locator::KIND_SHM,
            port,
            address,
        }
    }

    /// The locator of the metatraffic.
    pub fn metatraffic_locator(&self) -> Locator {
        self.locator(PORT_METATRAFFIC)
    }

    /// The locator of the user traffic.
    pub fn user_locator(&self) -> Locator {
        self.locator(PORT_USER)
    }

    /// The peer behind a shared memory locator of this host, opened.
    fn peer(&self, l: &Locator) -> Option<Arc<Peer>> {
        if l.kind != Locator::KIND_SHM || l.address[..8] != self.host {
            return None;
        }
        let id = u64::from_be_bytes(l.address[8..].try_into().ok()?);
        let mut peers = self.peers.lock().ok()?;
        peers
            .entry(id)
            .or_insert_with(|| {
                Segment::open(&segment_path(&self.directory, id))
                    .ok()
                    .map(|segment| Arc::new(Peer { segment }))
            })
            .clone()
    }

    /// Of a peer's locators, those to send to: its shared memory ones if
    /// one of them can be used, else the others.
    pub fn select<'a>(&self, locators: &'a [Locator]) -> Vec<&'a Locator> {
        let shm: Vec<&Locator> = locators.iter().filter(|l| self.peer(l).is_some()).collect();
        if shm.is_empty() {
            locators
                .iter()
                .filter(|l| l.kind != Locator::KIND_SHM)
                .collect()
        } else {
            shm
        }
    }

    /// Sends one message to a shared memory locator. Fails with
    /// [`io::ErrorKind::WouldBlock`] when the peer's buffer is full and
    /// [`io::ErrorKind::Unsupported`] for a locator it cannot use.
    pub fn send(&self, data: &[u8], to: &Locator) -> io::Result<()> {
        let peer = self.peer(to).ok_or_else(|| {
            io::Error::new(io::ErrorKind::Unsupported, "no such shared memory peer")
        })?;
        if !peer.segment.push(to.port, data) {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "the shared memory buffer is full",
            ));
        }
        match self.bell.send_to(&[0], peer.segment.bell()) {
            Ok(_) => Ok(()),
            // The peer is busy or its socket is full: it drains the ring
            // at its next wake-up anyway.
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => Ok(()),
            Err(e) => {
                // The peer is gone: forget it, so that the next send
                // opens it again or falls back.
                if let Ok(mut peers) = self.peers.lock() {
                    peers.retain(|_, p| !p.as_ref().is_some_and(|p| Arc::ptr_eq(p, &peer)));
                }
                Err(e)
            }
        }
    }
}

impl Drop for ShmTransport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        // On Windows this fails while a peer still maps the file; the
        // next participant with the same id removes it.
        let _ = std::fs::remove_file(&self.path);
    }
}

fn receive_loop(
    segment: &Segment,
    socket: &UdpSocket,
    stop: &AtomicBool,
    tx: &mpsc::Sender<Datagram>,
) {
    let mut buf = [0u8; 64];
    let source = SocketAddr::from((Ipv4Addr::LOCALHOST, 0));
    while !stop.load(Ordering::Relaxed) {
        // A ring, or the poll interval, wakes it; then it drains the
        // buffer, which may hold more than one message per ring.
        let _ = socket.recv(&mut buf);
        let mut closed = false;
        segment.drain(|port, data| {
            let channel = if port == PORT_METATRAFFIC {
                Channel::DiscoveryUnicast
            } else {
                Channel::UserUnicast
            };
            if tx
                .send(Datagram {
                    channel,
                    source,
                    data,
                })
                .is_err()
            {
                closed = true;
            }
        });
        if closed {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ShmConfig {
        ShmConfig {
            capacity: 4096,
            ..ShmConfig::default()
        }
    }

    fn prefix(n: u8) -> GuidPrefix {
        GuidPrefix([n; 12])
    }

    #[test]
    fn messages_go_through_in_order() {
        let (tx_a, _rx_a) = mpsc::channel();
        let (tx_b, rx_b) = mpsc::channel();
        let a = ShmTransport::open(&config(), prefix(1), tx_a).unwrap();
        let b = ShmTransport::open(&config(), prefix(2), tx_b).unwrap();
        let to = b.user_locator();
        assert_eq!(a.select(std::slice::from_ref(&to)), vec![&to]);
        // Enough messages to wrap around the ring several times, each
        // waiting for room.
        let mut got = Vec::new();
        for i in 0..200u32 {
            let msg = vec![i as u8; 1 + (i as usize * 7) % 900];
            let deadline = Instant::now() + Duration::from_secs(5);
            while a.send(&msg, &to).is_err() {
                assert!(Instant::now() < deadline);
                while let Ok(d) = rx_b.try_recv() {
                    got.push(d);
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        let deadline = Instant::now() + Duration::from_secs(5);
        while got.len() < 200 && Instant::now() < deadline {
            if let Ok(d) = rx_b.recv_timeout(Duration::from_millis(50)) {
                got.push(d);
            }
        }
        assert_eq!(got.len(), 200);
        for (i, d) in got.iter().enumerate() {
            assert_eq!(d.channel, Channel::UserUnicast);
            assert_eq!(d.data, vec![i as u8; 1 + (i * 7) % 900]);
        }
        let meta = b.metatraffic_locator();
        a.send(b"hello", &meta).unwrap();
        let d = rx_b.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            (d.channel, &d.data[..]),
            (Channel::DiscoveryUnicast, &b"hello"[..])
        );
    }

    #[test]
    fn other_hosts_and_gone_peers_use_udp() {
        let (tx, _rx) = mpsc::channel();
        let a = ShmTransport::open(&config(), prefix(3), tx).unwrap();
        let udp = Locator::udpv4("127.0.0.1:7411".parse().unwrap());
        let mut elsewhere = a.user_locator();
        elsewhere.address[0] ^= 0xff;
        let mut gone = a.user_locator();
        gone.address[15] ^= 0xff;
        assert_eq!(a.select(&[elsewhere, udp]), vec![&udp]);
        assert_eq!(a.select(&[gone, udp]), vec![&udp]);
        assert!(a.send(b"x", &gone).is_err());
        // Too large for the ring: not sent this way.
        let (tx_b, _rx_b) = mpsc::channel();
        let b = ShmTransport::open(&config(), prefix(4), tx_b).unwrap();
        assert!(a.send(&[0u8; 2048], &b.user_locator()).is_err());
    }

    #[test]
    fn dropping_removes_the_files() {
        let (tx, _rx) = mpsc::channel();
        let a = ShmTransport::open(&config(), prefix(5), tx).unwrap();
        let seg = a.path.clone();
        assert!(seg.exists());
        drop(a);
        assert!(!seg.exists());
    }
}
