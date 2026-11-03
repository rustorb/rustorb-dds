//! Two participants on this host find each other with SPDP, then find
//! and match each other's endpoints with SEDP, over UDP.

use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use rustorb_rtps::sedp::{EndpointData, Sedp, SedpConfig, SedpEvent};
use rustorb_rtps::spdp::{ParticipantData, ParticipantEvent, Spdp, SpdpConfig};
use rustorb_rtps::udp::{Incoming, UdpConfig, UdpTransport};
use rustorb_rtps::{EntityId, Guid, GuidPrefix, VendorId, entity_kind};

struct Node {
    transport: UdpTransport,
    incoming: Incoming,
    spdp: Spdp,
    sedp: Sedp,
}

fn node(domain_id: u32) -> Node {
    let mut config = UdpConfig::new(domain_id);
    config.interfaces = vec![Ipv4Addr::LOCALHOST];
    let (transport, incoming) = UdpTransport::open(&config).unwrap();
    let vendor = VendorId::UNKNOWN;
    let prefix = GuidPrefix::generate(vendor);
    let mut data = ParticipantData::new(prefix, vendor);
    data.domain_id = Some(domain_id);
    data.available_builtin_endpoints.0 |= Sedp::BUILTIN_ENDPOINTS;
    data.metatraffic_unicast_locators = transport.discovery_unicast_locators();
    data.metatraffic_multicast_locators = transport.discovery_multicast_locators();
    data.default_unicast_locators = transport.user_unicast_locators();
    let spdp = Spdp::new(
        data,
        SpdpConfig::new(domain_id, transport.discovery_multicast_locators()),
    );
    Node {
        transport,
        incoming,
        spdp,
        sedp: Sedp::new(prefix, vendor, SedpConfig::default()),
    }
}

impl Node {
    fn prefix(&self) -> GuidPrefix {
        self.spdp.local().guid_prefix
    }

    /// Sends what is due and handles what arrived; returns the SEDP
    /// events.
    fn step(&mut self) -> Vec<SedpEvent> {
        let now = Instant::now();
        if let Some((bytes, to)) = self.spdp.poll_announce(now) {
            for l in to {
                self.transport.send(&bytes, l).unwrap();
            }
        }
        for o in self.sedp.poll(now) {
            for l in &o.locators {
                self.transport.send(&o.bytes, l).unwrap();
            }
        }
        let mut events = Vec::new();
        while let Some(d) = self.incoming.recv_timeout(Duration::from_millis(10)) {
            for e in self.spdp.handle_message(&d.data, now) {
                match e {
                    ParticipantEvent::Discovered(p) | ParticipantEvent::Updated(p) => {
                        self.sedp.add_participant(&p, now)
                    }
                    ParticipantEvent::Removed(prefix, _) => {
                        events.extend(self.sedp.remove_participant(&prefix))
                    }
                }
            }
            events.extend(self.sedp.handle_message(&d.data, now));
        }
        events
    }
}

#[test]
fn endpoints_match_over_udp() {
    let mut a = node(62);
    let mut b = node(62);
    let w = Guid::new(
        a.prefix(),
        EntityId::user([0, 0, 1], entity_kind::USER_WRITER_NO_KEY),
    );
    let r = Guid::new(
        b.prefix(),
        EntityId::user([0, 0, 1], entity_kind::USER_READER_NO_KEY),
    );
    a.sedp
        .add_writer(EndpointData::writer(w, "Chatter", "std_msgs::String"));
    b.sedp
        .add_reader(EndpointData::reader(r, "Chatter", "std_msgs::String"));

    let deadline = Instant::now() + Duration::from_secs(10);
    let (mut a_matched, mut b_matched) = (false, false);
    while !(a_matched && b_matched) {
        assert!(Instant::now() < deadline, "endpoints did not match");
        for e in a.step() {
            if e == (SedpEvent::Matched {
                local: w,
                remote: r,
            }) {
                a_matched = true;
            }
        }
        for e in b.step() {
            if let SedpEvent::WriterDiscovered(d) = &e {
                // No locators of its own: the participant's defaults apply.
                assert_eq!(d.unicast_locators, a.transport.user_unicast_locators());
            }
            if e == (SedpEvent::Matched {
                local: r,
                remote: w,
            }) {
                b_matched = true;
            }
        }
    }

    // Removing the writer unmatches it on the other side.
    a.sedp.remove_writer(&w);
    let deadline = Instant::now() + Duration::from_secs(10);
    'outer: loop {
        assert!(Instant::now() < deadline, "writer removal did not arrive");
        a.step();
        for e in b.step() {
            if e == SedpEvent::WriterRemoved(w) {
                break 'outer;
            }
        }
    }
}
