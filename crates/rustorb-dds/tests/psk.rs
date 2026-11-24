//! RTPS messages protected with a pre-shared key (DDS Security 1.2,
//! rustorb-dds#84): the PSK plugins alone, and the builtin plugins with a
//! governance that asks for it.
#![cfg(feature = "security")]

#[path = "../examples/hello_world/greeting.rs"]
mod greeting;

use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{
    DataReaderQos, DomainParticipant, Error, History, ParticipantConfig, PskConfig, SecurityConfig,
};

const WAIT: Duration = Duration::from_secs(10);
const CERTS: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../rustorb-dds-security/tests/certs/"
);

fn psk(passphrase: &str) -> PskConfig {
    PskConfig {
        secret_passphrase: format!("data:,{passphrase}"),
        secret_passphrase_extra: None,
        symmetric_cipher_algorithm: None,
        protection_kind: None,
    }
}

fn config(domain: u32) -> ParticipantConfig {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    c.shared_memory = false;
    c
}

fn with_psk(domain: u32, p: Option<PskConfig>) -> DomainParticipant {
    let mut c = config(domain);
    c.psk = p;
    DomainParticipant::with_config(c).unwrap()
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

/// Samples go from `p1` to `p2` on `topic`.
fn samples_flow(p1: &DomainParticipant, p2: &DomainParticipant, topic: &str) -> bool {
    let t1 = p1.create_topic::<Greeting>(topic).unwrap();
    let t2 = p2.create_topic::<Greeting>(topic).unwrap();
    let writer = p1
        .create_publisher()
        .unwrap()
        .create_datawriter(&t1)
        .unwrap();
    let reader = p2
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &t2,
            DataReaderQos {
                history: History::KeepAll,
                ..DataReaderQos::default()
            },
        )
        .unwrap();
    if !wait_until(|| !writer.matched_subscriptions().is_empty()) {
        return false;
    }
    let mut got = Vec::new();
    let mut i = 0;
    wait_until(|| {
        i += 1;
        writer
            .write(&Greeting {
                index: i,
                message: format!("psk {i}"),
            })
            .unwrap();
        got.extend(reader.take().into_iter().filter_map(|s| s.data));
        got.len() >= 3
    })
}

/// The PSK plugins: participants with the same passphrase find each
/// other and exchange samples; one with another passphrase, or with
/// none, does not even discover them.
#[test]
fn participants_with_the_same_passphrase_talk() {
    let p1 = with_psk(87, Some(psk("5612:Open Sesame")));
    let p2 = with_psk(87, Some(psk("5612:Open Sesame")));
    let other = with_psk(87, Some(psk("5612:Close Sesame")));
    let plain = with_psk(87, None);
    assert!(samples_flow(&p1, &p2, "Circle"), "no samples");
    assert!(p1.discovered_participants().contains(&p2.guid().prefix));
    std::thread::sleep(Duration::from_secs(1));
    for p in [&p1, &p2] {
        let found = p.discovered_participants();
        assert!(!found.contains(&other.guid().prefix));
        assert!(!found.contains(&plain.guid().prefix));
    }
    assert!(!plain.discovered_participants().contains(&p1.guid().prefix));
    assert!(!other.discovered_participants().contains(&p1.guid().prefix));
}

/// SIGN with AES-128, set with the properties of 10.5.1 and 10.8.1.
#[test]
fn signed_messages_from_properties() {
    let make = || {
        let mut c = config(88);
        c.set_properties([
            (
                "dds.sec.crypto.rtps_psk_secret_passphrase",
                "data:,77:a secret",
            ),
            (
                "dds.sec.crypto.rtps_psk_symmetric_cipher_algorithm",
                "AES128-GCM",
            ),
            ("dds.sec.access.rtps_psk_protection_kind", "SIGN"),
        ])
        .unwrap();
        assert!(c.security.is_none());
        assert!(c.psk.is_some());
        DomainParticipant::with_config(c).unwrap()
    };
    let p1 = make();
    let p2 = make();
    // An ENCRYPT participant expects another transformation.
    let enc = with_psk(88, Some(psk("77:a secret")));
    assert!(samples_flow(&p1, &p2, "Circle"), "no samples");
    std::thread::sleep(Duration::from_secs(1));
    assert!(!p1.discovered_participants().contains(&enc.guid().prefix));
    // Properties without the passphrase are refused.
    let mut c = config(88);
    assert!(matches!(
        c.set_properties([("dds.sec.access.rtps_psk_protection_kind", "SIGN")]),
        Err(Error::Security(_))
    ));
    assert!(matches!(
        c.set_properties([(
            "dds.sec.crypto.rtps_psk_secret_passphrase",
            "data:,255:reserved id"
        )]),
        Ok(())
    ));
    assert!(matches!(
        DomainParticipant::with_config(c),
        Err(Error::Security(_))
    ));
}

/// The passphrases can be replaced while the system runs: each
/// participant also accepts the passphrases of
/// `dds.sec.crypto.rtps_psk_secret_passphrase_extra`.
#[test]
fn extra_passphrases_let_the_secret_change() {
    let mut old = psk("1:old secret");
    old.secret_passphrase_extra = Some("data:,2:new secret".into());
    let mut new = psk("2:new secret");
    new.secret_passphrase_extra = Some("data:,1:old secret".into());
    let p1 = with_psk(89, Some(old));
    let p2 = with_psk(89, Some(new));
    assert!(samples_flow(&p1, &p2, "Circle"), "no samples");
    assert!(samples_flow(&p2, &p1, "Square"), "no samples back");
}

fn security(who: &str, governance: &str) -> SecurityConfig {
    let f = |n: &str| format!("file:{CERTS}{n}");
    SecurityConfig {
        identity_ca: f("identity_ca.pem"),
        identity_certificate: f(&format!("{who}_cert.pem")),
        private_key: f(&format!("{who}_key.pem")),
        permissions_ca: f("permissions_ca.pem"),
        governance: f(governance),
        permissions: f(&format!("permissions_{who}.p7s")),
        password: None,
        crl: None,
        ocsp: None,
        logging: None,
    }
}

/// The builtin plugins with a governance whose rtps_psk_protection_kind
/// is ENCRYPT: the bootstrapping messages (SPDP, the handshake, the key
/// exchange) go protected with the pre-shared key, the others with the
/// participant keys. Without the passphrase, the participant cannot be
/// made.
#[test]
fn the_governance_asks_for_psk_protection() {
    let make = |who: &str, p: Option<PskConfig>| {
        let mut c = config(90);
        c.security = Some(security(who, "governance_psk.p7s"));
        c.psk = p;
        DomainParticipant::with_config(c)
    };
    assert!(matches!(make("p1", None), Err(Error::Security(_))));
    let p1 = make("p1", Some(psk("9:governed"))).unwrap();
    let p2 = make("p2", Some(psk("9:governed"))).unwrap();
    // Another passphrase: not even discovered, so no handshake.
    let p3 = make("p2", Some(psk("9:not this one"))).unwrap();
    assert!(samples_flow(&p1, &p2, "Square"), "no samples");
    std::thread::sleep(Duration::from_secs(1));
    assert!(!p1.discovered_participants().contains(&p3.guid().prefix));
}
