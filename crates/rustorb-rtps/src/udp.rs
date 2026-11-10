//! The UDPv4 transport and the default port numbers
//! (DDSI-RTPS 2.5, 8.5.3.4, 9.5 and 9.6.2).
//!
//! One RTPS message is the payload of one UDP datagram (9.5). A
//! [`UdpTransport`] opens the four sockets a participant needs (discovery
//! and user traffic, each unicast and multicast), receives on them with
//! one thread per socket, and hands the datagrams over a channel. It does
//! not depend on an async runtime.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use socket2::{Domain, Protocol, SockAddr, Socket, Type};

use crate::types::Locator;

/// The tunable parameters of the port number expressions
/// (9.6.2.3). [`Default`] gives the values required for out-of-the-box
/// interoperability.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PortMapping {
    /// PB, the port base number.
    pub port_base: u32,
    /// DG, the domain id gain.
    pub domain_id_gain: u32,
    /// PG, the participant id gain.
    pub participant_id_gain: u32,
    /// d0, offset of the discovery multicast port.
    pub d0: u32,
    /// d1, offset of the discovery unicast port.
    pub d1: u32,
    /// d2, offset of the user multicast port.
    pub d2: u32,
    /// d3, offset of the user unicast port.
    pub d3: u32,
}

impl Default for PortMapping {
    fn default() -> Self {
        PortMapping {
            port_base: 7400,
            domain_id_gain: 250,
            participant_id_gain: 2,
            d0: 0,
            d1: 10,
            d2: 1,
            d3: 11,
        }
    }
}

impl PortMapping {
    fn port(&self, domain_id: u32, offset: u32, participant_id: Option<u32>) -> Option<u16> {
        let mut p = u64::from(self.port_base)
            + u64::from(self.domain_id_gain) * u64::from(domain_id)
            + u64::from(offset);
        if let Some(pid) = participant_id {
            p += u64::from(self.participant_id_gain) * u64::from(pid);
        }
        u16::try_from(p).ok()
    }

    /// `SPDP_WELL_KNOWN_MULTICAST_PORT` = PB + DG * domainId + d0
    /// (Table 9.15). `None` if it does not fit in 16 bits.
    pub fn discovery_multicast_port(&self, domain_id: u32) -> Option<u16> {
        self.port(domain_id, self.d0, None)
    }

    /// `SPDP_WELL_KNOWN_UNICAST_PORT` = PB + DG * domainId + d1 + PG *
    /// participantId (Table 9.15).
    pub fn discovery_unicast_port(&self, domain_id: u32, participant_id: u32) -> Option<u16> {
        self.port(domain_id, self.d1, Some(participant_id))
    }

    /// The default user multicast port, PB + DG * domainId + d2
    /// (Table 9.16).
    pub fn user_multicast_port(&self, domain_id: u32) -> Option<u16> {
        self.port(domain_id, self.d2, None)
    }

    /// The default user unicast port, PB + DG * domainId + d3 + PG *
    /// participantId (Table 9.16).
    pub fn user_unicast_port(&self, domain_id: u32, participant_id: u32) -> Option<u16> {
        self.port(domain_id, self.d3, Some(participant_id))
    }
}

/// The multicast address every participant announces on and listens to
/// (9.6.2.4.1).
pub const DEFAULT_MULTICAST_ADDRESS: Ipv4Addr = Ipv4Addr::new(239, 255, 0, 1);

/// Settings of a [`UdpTransport`].
#[derive(Clone, Debug)]
pub struct UdpConfig {
    /// The DDS domain id.
    pub domain_id: u32,
    /// The participant id. `None` picks the lowest id, starting at 0,
    /// whose unicast ports are free (9.6.2.1).
    pub participant_id: Option<u32>,
    /// The highest participant id tried when `participant_id` is `None`.
    pub max_participant_id: u32,
    /// The port number parameters.
    pub ports: PortMapping,
    /// The interfaces to use, by their IPv4 address. Empty means the
    /// system's choice: multicast is joined on the default interface and
    /// the unicast locators use the address of the default route.
    pub interfaces: Vec<Ipv4Addr>,
    /// The multicast group.
    pub multicast_address: Ipv4Addr,
    /// The time-to-live of multicast datagrams sent.
    pub multicast_ttl: u32,
    /// Whether multicast datagrams sent are also delivered to this host,
    /// so that participants on the same host find each other.
    pub multicast_loopback: bool,
    /// The receive buffer to ask for on each socket that receives
    /// (`SO_RCVBUF`; the system may give less). A large sample comes as
    /// a burst of datagrams, which a small buffer drops. `None` keeps
    /// the system's default.
    pub receive_buffer_size: Option<usize>,
    /// The send buffer to ask for on each socket (`SO_SNDBUF`). On macOS
    /// it also bounds the largest datagram a socket can send, 9216
    /// octets by default, so it must be at least the largest message
    /// sent (up to 64 KiB, see `rustorb_rtps::frag`). `None` keeps the
    /// system's default.
    pub send_buffer_size: Option<usize>,
}

impl UdpConfig {
    /// The default settings for a domain.
    pub fn new(domain_id: u32) -> Self {
        UdpConfig {
            domain_id,
            participant_id: None,
            max_participant_id: 119,
            ports: PortMapping::default(),
            interfaces: Vec::new(),
            multicast_address: DEFAULT_MULTICAST_ADDRESS,
            multicast_ttl: 1,
            multicast_loopback: true,
            receive_buffer_size: Some(4 * 1024 * 1024),
            send_buffer_size: Some(1024 * 1024),
        }
    }
}

/// Which socket a datagram came in on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Channel {
    /// Discovery traffic, multicast port (d0).
    DiscoveryMulticast,
    /// Discovery traffic, unicast port (d1).
    DiscoveryUnicast,
    /// User traffic, multicast port (d2).
    UserMulticast,
    /// User traffic, unicast port (d3).
    UserUnicast,
}

/// One received datagram: one RTPS message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Datagram {
    /// The socket it arrived on.
    pub channel: Channel,
    /// The sender.
    pub source: SocketAddr,
    /// The UDP payload.
    pub data: Vec<u8>,
}

/// The receiving end of a [`UdpTransport`].
///
/// It stops yielding datagrams once the transport is dropped.
#[derive(Debug)]
pub struct Incoming {
    rx: mpsc::Receiver<Datagram>,
}

impl Incoming {
    /// Waits for the next datagram. Returns `None` once the transport is
    /// closed.
    pub fn recv(&self) -> Option<Datagram> {
        self.rx.recv().ok()
    }

    /// Waits at most `timeout` for the next datagram.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Datagram> {
        self.rx.recv_timeout(timeout).ok()
    }

    /// Returns a datagram if one is waiting.
    pub fn try_recv(&self) -> Option<Datagram> {
        self.rx.try_recv().ok()
    }
}

/// How often the receive threads look at the stop flag.
const POLL: Duration = Duration::from_millis(100);
/// The largest UDP payload.
const MAX_DATAGRAM: usize = 65_536;

/// The UDPv4 sockets of one participant.
#[derive(Debug)]
pub struct UdpTransport {
    domain_id: u32,
    participant_id: u32,
    ports: PortMapping,
    multicast_address: Ipv4Addr,
    unicast_addresses: Vec<Ipv4Addr>,
    /// Sends unicast; this is the user unicast socket.
    unicast_sender: Arc<UdpSocket>,
    /// Sends multicast, one socket per interface.
    multicast_senders: Vec<UdpSocket>,
    stop: Arc<AtomicBool>,
    threads: Vec<JoinHandle<()>>,
    /// For other transports to deliver to the same [`Incoming`].
    tx: mpsc::Sender<Datagram>,
}

impl UdpTransport {
    /// Opens the sockets, joins the multicast group and starts receiving.
    pub fn open(config: &UdpConfig) -> io::Result<(UdpTransport, Incoming)> {
        let ports = config.ports;
        let domain_id = config.domain_id;
        let port_err = || io::Error::new(io::ErrorKind::InvalidInput, "port number out of range");

        let disc_mc_port = ports
            .discovery_multicast_port(domain_id)
            .ok_or_else(port_err)?;
        let user_mc_port = ports.user_multicast_port(domain_id).ok_or_else(port_err)?;
        let disc_mc = multicast_socket(config, disc_mc_port)?;
        let user_mc = if user_mc_port == disc_mc_port {
            None
        } else {
            Some(multicast_socket(config, user_mc_port)?)
        };

        let (participant_id, disc_uc, user_uc) = bind_unicast(config)?;

        let multicast_senders = if config.interfaces.is_empty() {
            vec![multicast_sender(config, None)?]
        } else {
            config
                .interfaces
                .iter()
                .map(|i| multicast_sender(config, Some(*i)))
                .collect::<io::Result<_>>()?
        };

        let unicast_addresses = if config.interfaces.is_empty() {
            vec![default_address(config.multicast_address)]
        } else {
            config.interfaces.clone()
        };

        let user_uc = Arc::new(user_uc);
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::channel();
        let mut threads = Vec::new();
        let mut spawn = |socket: Arc<UdpSocket>, channel: Channel| -> io::Result<()> {
            socket.set_read_timeout(Some(POLL))?;
            let stop = stop.clone();
            let tx = tx.clone();
            let handle = std::thread::Builder::new()
                .name(format!("rtps-udp-{channel:?}"))
                .spawn(move || receive_loop(&socket, channel, &stop, &tx))?;
            threads.push(handle);
            Ok(())
        };
        spawn(Arc::new(disc_mc), Channel::DiscoveryMulticast)?;
        if let Some(s) = user_mc {
            spawn(Arc::new(s), Channel::UserMulticast)?;
        }
        spawn(Arc::new(disc_uc), Channel::DiscoveryUnicast)?;
        spawn(user_uc.clone(), Channel::UserUnicast)?;

        Ok((
            UdpTransport {
                domain_id,
                participant_id,
                ports,
                multicast_address: config.multicast_address,
                unicast_addresses,
                unicast_sender: user_uc,
                multicast_senders,
                stop,
                threads,
                tx,
            },
            Incoming { rx },
        ))
    }

    /// A sender of datagrams to this transport's [`Incoming`], for
    /// another transport (shared memory) to deliver there too.
    pub fn injector(&self) -> mpsc::Sender<Datagram> {
        self.tx.clone()
    }

    /// The domain id.
    pub fn domain_id(&self) -> u32 {
        self.domain_id
    }

    /// The participant id that was chosen.
    pub fn participant_id(&self) -> u32 {
        self.participant_id
    }

    /// Where other participants reach this one's built-in endpoints by
    /// unicast (`metatrafficUnicastLocatorList`).
    pub fn discovery_unicast_locators(&self) -> Vec<Locator> {
        let port = self
            .ports
            .discovery_unicast_port(self.domain_id, self.participant_id)
            .expect("checked when opened");
        self.unicast_locators(port)
    }

    /// `metatrafficMulticastLocatorList`: the group on the discovery
    /// multicast port.
    pub fn discovery_multicast_locators(&self) -> Vec<Locator> {
        let port = self
            .ports
            .discovery_multicast_port(self.domain_id)
            .expect("checked when opened");
        vec![Locator::udpv4(SocketAddrV4::new(
            self.multicast_address,
            port,
        ))]
    }

    /// `defaultUnicastLocatorList`: where user endpoints are reached by
    /// unicast.
    pub fn user_unicast_locators(&self) -> Vec<Locator> {
        let port = self
            .ports
            .user_unicast_port(self.domain_id, self.participant_id)
            .expect("checked when opened");
        self.unicast_locators(port)
    }

    /// `defaultMulticastLocatorList`: the group on the user multicast
    /// port.
    pub fn user_multicast_locators(&self) -> Vec<Locator> {
        let port = self
            .ports
            .user_multicast_port(self.domain_id)
            .expect("checked when opened");
        vec![Locator::udpv4(SocketAddrV4::new(
            self.multicast_address,
            port,
        ))]
    }

    fn unicast_locators(&self, port: u16) -> Vec<Locator> {
        self.unicast_addresses
            .iter()
            .map(|a| Locator::udpv4(SocketAddrV4::new(*a, port)))
            .collect()
    }

    /// Sends one message to a locator. Multicast destinations are sent on
    /// every configured interface.
    ///
    /// Locators that are not UDPv4 are refused with
    /// [`io::ErrorKind::Unsupported`].
    pub fn send(&self, data: &[u8], to: &Locator) -> io::Result<()> {
        let addr = match (to.kind, to.to_socket_addr()) {
            (Locator::KIND_UDPV4, Some(SocketAddr::V4(a))) => a,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "only UDPv4 locators are supported",
                ));
            }
        };
        if addr.ip().is_multicast() {
            let mut result = Ok(());
            for s in &self.multicast_senders {
                if let Err(e) = s.send_to(data, addr) {
                    result = Err(e);
                }
            }
            result
        } else {
            self.unicast_sender.send_to(data, addr).map(|_| ())
        }
    }
}

impl Drop for UdpTransport {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
    }
}

fn receive_loop(
    socket: &UdpSocket,
    channel: Channel,
    stop: &AtomicBool,
    tx: &mpsc::Sender<Datagram>,
) {
    let mut buf = vec![0u8; MAX_DATAGRAM];
    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((n, source)) => {
                let d = Datagram {
                    channel,
                    source,
                    data: buf[..n].to_vec(),
                };
                if tx.send(d).is_err() {
                    return;
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) => {}
            // On Windows a previous send to a closed port shows up here as
            // ConnectionReset; it says nothing about this socket.
            Err(e) if e.kind() == io::ErrorKind::ConnectionReset => {}
            Err(_) => std::thread::sleep(POLL),
        }
    }
}

fn new_udp_socket() -> io::Result<Socket> {
    Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))
}

/// A socket with the buffers asked for. Failing to get them is not an
/// error: the socket works with what it has.
fn new_configured_socket(config: &UdpConfig) -> io::Result<Socket> {
    let s = new_udp_socket()?;
    if let Some(size) = config.receive_buffer_size {
        let _ = s.set_recv_buffer_size(size);
    }
    if let Some(size) = config.send_buffer_size {
        let _ = s.set_send_buffer_size(size);
    }
    Ok(s)
}

/// A socket on a multicast port. All participants of a domain on a host
/// share the port, so the address is marked reusable.
fn multicast_socket(config: &UdpConfig, port: u16) -> io::Result<UdpSocket> {
    let s = new_configured_socket(config)?;
    s.set_reuse_address(true)?;
    #[cfg(all(unix, not(any(target_os = "solaris", target_os = "illumos"))))]
    s.set_reuse_port(true)?;
    s.bind(&SockAddr::from(SocketAddrV4::new(
        Ipv4Addr::UNSPECIFIED,
        port,
    )))?;
    if config.interfaces.is_empty() {
        s.join_multicast_v4(&config.multicast_address, &Ipv4Addr::UNSPECIFIED)?;
    } else {
        for i in &config.interfaces {
            s.join_multicast_v4(&config.multicast_address, i)?;
        }
    }
    Ok(s.into())
}

fn multicast_sender(config: &UdpConfig, interface: Option<Ipv4Addr>) -> io::Result<UdpSocket> {
    let s = new_configured_socket(config)?;
    s.set_multicast_ttl_v4(config.multicast_ttl)?;
    s.set_multicast_loop_v4(config.multicast_loopback)?;
    if let Some(i) = interface {
        s.set_multicast_if_v4(&i)?;
    }
    s.bind(&SockAddr::from(SocketAddrV4::new(
        interface.unwrap_or(Ipv4Addr::UNSPECIFIED),
        0,
    )))?;
    Ok(s.into())
}

/// A socket on a unicast port, not shared with anyone.
fn unicast_socket(config: &UdpConfig, port: u16) -> io::Result<UdpSocket> {
    let s = new_configured_socket(config)?;
    // On Windows, SO_EXCLUSIVEADDRUSE would be needed to stop others
    // from taking the port; without SO_REUSEADDR the bind at least fails
    // when the port is already taken.
    s.bind(&SockAddr::from(SocketAddrV4::new(
        Ipv4Addr::UNSPECIFIED,
        port,
    )))?;
    Ok(s.into())
}

/// Picks the participant id and binds its discovery and user unicast
/// ports (9.6.2.1).
fn bind_unicast(config: &UdpConfig) -> io::Result<(u32, UdpSocket, UdpSocket)> {
    let ports = &config.ports;
    let range = match config.participant_id {
        Some(id) => id..=id,
        None => 0..=config.max_participant_id,
    };
    let mut last_err = None;
    for pid in range {
        let (Some(disc), Some(user)) = (
            ports.discovery_unicast_port(config.domain_id, pid),
            ports.user_unicast_port(config.domain_id, pid),
        ) else {
            break;
        };
        match (unicast_socket(config, disc), unicast_socket(config, user)) {
            (Ok(d), Ok(u)) => return Ok((pid, d, u)),
            (Err(e), _) | (_, Err(e)) => last_err = Some(e),
        }
    }
    Err(last_err
        .unwrap_or_else(|| io::Error::new(io::ErrorKind::AddrInUse, "no free participant id")))
}

/// The local address of the route towards `toward`, found by connecting a
/// UDP socket (no datagram is sent). Falls back to the loopback address.
fn default_address(toward: Ipv4Addr) -> Ipv4Addr {
    let found = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).and_then(|s| {
        s.connect((toward, 7400))?;
        s.local_addr()
    });
    match found {
        Ok(SocketAddr::V4(a)) if !a.ip().is_unspecified() => *a.ip(),
        _ => Ipv4Addr::LOCALHOST,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_ports() {
        let p = PortMapping::default();
        assert_eq!(p.discovery_multicast_port(0), Some(7400));
        assert_eq!(p.discovery_unicast_port(0, 0), Some(7410));
        assert_eq!(p.user_multicast_port(0), Some(7401));
        assert_eq!(p.user_unicast_port(0, 0), Some(7411));
        assert_eq!(p.discovery_unicast_port(0, 1), Some(7412));
        assert_eq!(p.user_unicast_port(0, 1), Some(7413));
        assert_eq!(p.discovery_multicast_port(1), Some(7650));
        assert_eq!(p.user_unicast_port(2, 3), Some(7400 + 500 + 11 + 6));
        // 7400 + 250 * 233 = 65650 does not fit.
        assert_eq!(p.discovery_multicast_port(233), None);
        assert_eq!(p.discovery_multicast_port(232), Some(65400));
        assert_eq!(p.user_unicast_port(232, 62), Some(65535));
        assert_eq!(p.user_unicast_port(232, 63), None);
    }
}
