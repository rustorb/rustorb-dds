//! Tests of the UDP transport on this host.
//!
//! Each test uses its own domain id so that tests running at the same
//! time do not see each other's traffic.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use rustorb_rtps::Locator;
use rustorb_rtps::udp::{Channel, Datagram, Incoming, UdpConfig, UdpTransport};

const WAIT: Duration = Duration::from_secs(5);

fn loopback_config(domain_id: u32) -> UdpConfig {
    let mut c = UdpConfig::new(domain_id);
    c.interfaces = vec![Ipv4Addr::LOCALHOST];
    c
}

/// Waits for a datagram with the given payload, skipping anything else
/// (other software on the host may use the same group).
fn wait_for(incoming: &Incoming, payload: &[u8]) -> Datagram {
    let deadline = Instant::now() + WAIT;
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        if let Some(d) = incoming.recv_timeout(left) {
            if d.data == payload {
                return d;
            }
        }
    }
    panic!("datagram not received");
}

#[test]
fn participant_ids_are_picked_in_order() {
    let config = loopback_config(51);
    let (a, _ia) = UdpTransport::open(&config).unwrap();
    let (b, _ib) = UdpTransport::open(&config).unwrap();
    assert_ne!(a.participant_id(), b.participant_id());
    // 7400 + 250 * 51 = 20150.
    assert_eq!(
        a.discovery_multicast_locators(),
        [Locator::udpv4("239.255.0.1:20150".parse().unwrap())]
    );
    assert_eq!(
        a.user_multicast_locators(),
        [Locator::udpv4("239.255.0.1:20151".parse().unwrap())]
    );
    let pid = a.participant_id();
    assert_eq!(
        a.discovery_unicast_locators(),
        [Locator::udpv4(
            format!("127.0.0.1:{}", 20160 + 2 * pid).parse().unwrap()
        )]
    );
    assert_eq!(
        a.user_unicast_locators(),
        [Locator::udpv4(
            format!("127.0.0.1:{}", 20161 + 2 * pid).parse().unwrap()
        )]
    );
}

#[test]
fn fixed_participant_id_in_use_fails() {
    let mut config = loopback_config(52);
    config.participant_id = Some(3);
    let (a, _ia) = UdpTransport::open(&config).unwrap();
    assert_eq!(a.participant_id(), 3);
    assert!(UdpTransport::open(&config).is_err());
}

#[test]
fn unicast_between_two_participants() {
    let config = loopback_config(53);
    let (a, _ia) = UdpTransport::open(&config).unwrap();
    let (b, ib) = UdpTransport::open(&config).unwrap();

    let to_user = b.user_unicast_locators()[0];
    a.send(b"user", &to_user).unwrap();
    let d = wait_for(&ib, b"user");
    assert_eq!(d.channel, Channel::UserUnicast);

    let to_disc = b.discovery_unicast_locators()[0];
    a.send(b"disc", &to_disc).unwrap();
    let d = wait_for(&ib, b"disc");
    assert_eq!(d.channel, Channel::DiscoveryUnicast);
    // Unicast is sent from the sender's user unicast port.
    assert_eq!(
        Locator::udpv4(match d.source {
            std::net::SocketAddr::V4(s) => s,
            _ => panic!(),
        }),
        a.user_unicast_locators()[0]
    );
}

#[test]
fn multicast_reaches_every_participant_on_the_host() {
    let config = loopback_config(54);
    let (a, ia) = UdpTransport::open(&config).unwrap();
    let (_b, ib) = UdpTransport::open(&config).unwrap();

    let group = a.discovery_multicast_locators()[0];
    a.send(b"spdp", &group).unwrap();
    for incoming in [&ia, &ib] {
        let d = wait_for(incoming, b"spdp");
        assert_eq!(d.channel, Channel::DiscoveryMulticast);
    }

    let group = a.user_multicast_locators()[0];
    a.send(b"data", &group).unwrap();
    let d = wait_for(&ib, b"data");
    assert_eq!(d.channel, Channel::UserMulticast);
}

#[test]
fn non_udpv4_locators_are_refused() {
    let (a, _ia) = UdpTransport::open(&loopback_config(55)).unwrap();
    let v6 = Locator::udpv6("[::1]:7400".parse().unwrap());
    assert_eq!(
        a.send(b"x", &v6).unwrap_err().kind(),
        std::io::ErrorKind::Unsupported
    );
    assert!(a.send(b"x", &Locator::INVALID).is_err());
}

#[test]
fn closing_ends_incoming() {
    let (a, ia) = UdpTransport::open(&loopback_config(56)).unwrap();
    drop(a);
    assert_eq!(ia.recv_timeout(Duration::from_secs(2)), None);
}

#[test]
fn default_interfaces_open() {
    // With no interfaces given, the system picks them. This only checks
    // that opening works and gives one unicast address.
    let (a, _ia) = UdpTransport::open(&UdpConfig::new(57)).unwrap();
    assert_eq!(a.discovery_unicast_locators().len(), 1);
}
