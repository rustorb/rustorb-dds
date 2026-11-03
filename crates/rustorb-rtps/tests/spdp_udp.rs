//! Two participants on this host find each other with SPDP over UDP.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use rustorb_rtps::spdp::{ParticipantData, ParticipantEvent, Spdp, SpdpConfig};
use rustorb_rtps::udp::{Incoming, UdpConfig, UdpTransport};
use rustorb_rtps::{GuidPrefix, VendorId};

struct Node {
    transport: UdpTransport,
    incoming: Incoming,
    spdp: Spdp,
}

fn node(domain_id: u32) -> Node {
    let mut config = UdpConfig::new(domain_id);
    config.interfaces = vec![Ipv4Addr::LOCALHOST];
    let (transport, incoming) = UdpTransport::open(&config).unwrap();
    let vendor = VendorId::UNKNOWN;
    let mut data = ParticipantData::new(GuidPrefix::generate(vendor), vendor);
    data.domain_id = Some(domain_id);
    data.metatraffic_unicast_locators = transport.discovery_unicast_locators();
    data.metatraffic_multicast_locators = transport.discovery_multicast_locators();
    data.default_unicast_locators = transport.user_unicast_locators();
    data.default_multicast_locators = transport.user_multicast_locators();
    let spdp = Spdp::new(
        data,
        SpdpConfig::new(domain_id, transport.discovery_multicast_locators()),
    );
    Node {
        transport,
        incoming,
        spdp,
    }
}

impl Node {
    /// Announces if due and handles what arrived; returns the events.
    fn step(&mut self) -> Vec<ParticipantEvent> {
        let now = Instant::now();
        if let Some((bytes, to)) = self.spdp.poll_announce(now) {
            for l in to {
                self.transport.send(&bytes, l).unwrap();
            }
        }
        let mut events = Vec::new();
        while let Some(d) = self.incoming.recv_timeout(Duration::from_millis(20)) {
            events.extend(self.spdp.handle_message(&d.data, now));
        }
        events
    }
}

#[test]
fn two_participants_discover_each_other() {
    let mut a = node(61);
    let mut b = node(61);
    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut a_saw_b, mut b_saw_a) = (false, false);
    while !(a_saw_b && b_saw_a) {
        assert!(
            Instant::now() < deadline,
            "participants did not find each other"
        );
        for e in a.step() {
            if let ParticipantEvent::Discovered(d) = e {
                assert_eq!(d.guid_prefix, b.spdp.local().guid_prefix);
                assert_eq!(
                    d.default_unicast_locators,
                    b.transport.user_unicast_locators()
                );
                a_saw_b = true;
            }
        }
        for e in b.step() {
            if let ParticipantEvent::Discovered(d) = e {
                assert_eq!(d.guid_prefix, a.spdp.local().guid_prefix);
                b_saw_a = true;
            }
        }
    }
}

#[test]
fn generated_prefixes_differ() {
    let v = VendorId([0x01, 0x23]);
    let a = GuidPrefix::generate(v);
    let b = GuidPrefix::generate(v);
    assert_ne!(a, b);
    assert_eq!(&a.0[..2], &[0x01, 0x23]);
}
