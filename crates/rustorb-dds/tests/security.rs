//! DDS Security between two participants of this host: authentication,
//! access control and protected samples.
#![cfg(feature = "security")]

#[path = "../examples/hello_world/greeting.rs"]
mod greeting;

use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{
    DataReaderQos, DomainParticipant, Error, History, ParticipantConfig, SecurityConfig,
};

const WAIT: Duration = Duration::from_secs(10);
const CERTS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../rustorb-dds-security/tests/certs/"
);

fn security(who: &str) -> SecurityConfig {
    let f = |n: &str| format!("file:{CERTS}{n}");
    SecurityConfig {
        identity_ca: f("identity_ca.pem"),
        identity_certificate: f(&format!("{who}_cert.pem")),
        private_key: f(&format!("{who}_key.pem")),
        permissions_ca: f("permissions_ca.pem"),
        governance: f("governance.p7s"),
        permissions: f(&format!("permissions_{who}.p7s")),
    }
}

fn participant(domain: u32, who: Option<&str>) -> rustorb_dds::Result<DomainParticipant> {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    c.shared_memory = false;
    c.security = who.map(security);
    DomainParticipant::with_config(c)
}

fn keep_all() -> DataReaderQos {
    DataReaderQos {
        history: History::KeepAll,
        ..DataReaderQos::default()
    }
}

fn wait_until(mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + WAIT;
    while Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

/// A protected topic (encrypted submessages with origin authentication,
/// encrypted payloads, signed RTPS messages) between two authenticated
/// participants.
#[test]
fn protected_topic() {
    let p1 = participant(81, Some("p1")).unwrap();
    let p2 = participant(81, Some("p2")).unwrap();
    let topic1 = p1.create_topic::<Greeting>("Square").unwrap();
    let topic2 = p2.create_topic::<Greeting>("Square").unwrap();
    let writer = p1
        .create_publisher()
        .unwrap()
        .create_datawriter(&topic1)
        .unwrap();
    let reader = p2
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&topic2, keep_all())
        .unwrap();
    assert!(
        wait_until(|| !writer.matched_subscriptions().is_empty()),
        "not matched"
    );
    let mut got = Vec::new();
    let mut i = 0;
    let ok = wait_until(|| {
        i += 1;
        writer
            .write(&Greeting {
                index: i,
                message: format!("secret {i}"),
            })
            .unwrap();
        got.extend(reader.take().into_iter().filter_map(|s| s.data));
        got.len() >= 3
    });
    assert!(ok, "no samples");
    assert!(got[0].message.starts_with("secret "));
}

/// The permissions are checked: participant1 may not publish on a topic
/// it is not granted, when the governance controls it.
#[test]
fn permissions_are_enforced() {
    let p1 = participant(82, Some("p1")).unwrap();
    // "Circle" falls under the "*" rule, which controls nothing.
    let circle = p1.create_topic::<Greeting>("Circle").unwrap();
    assert!(
        p1.create_publisher()
            .unwrap()
            .create_datawriter(&circle)
            .is_ok()
    );
    // A participant cannot join a domain its permissions do not cover.
    match participant(231, Some("p1")) {
        Err(Error::Security(_)) => {}
        other => panic!("joined: {:?}", other.map(|_| ())),
    }
}

/// A participant without security is not let in: nothing matches.
#[test]
fn unauthenticated_participants_are_ignored() {
    let p1 = participant(83, Some("p1")).unwrap();
    let p0 = participant(83, None).unwrap();
    let t1 = p1.create_topic::<Greeting>("Circle").unwrap();
    let t0 = p0.create_topic::<Greeting>("Circle").unwrap();
    let writer = p1
        .create_publisher()
        .unwrap()
        .create_datawriter(&t1)
        .unwrap();
    let _reader = p0
        .create_subscriber()
        .unwrap()
        .create_datareader(&t0)
        .unwrap();
    std::thread::sleep(Duration::from_secs(2));
    assert!(writer.matched_subscriptions().is_empty());
}
