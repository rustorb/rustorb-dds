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
        password: None,
        crl: None,
        logging: None,
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

/// The participants send each other their data on
/// DCPSParticipantsSecure, and the deletion of one goes there too: the
/// other forgets it at once, not when its lease runs out.
#[test]
fn deletion_goes_on_the_secure_participant_topic() {
    let p1 = participant(84, Some("p1")).unwrap();
    let p2 = participant(84, Some("p2")).unwrap();
    let t1 = p1.create_topic::<Greeting>("Circle").unwrap();
    let t2 = p2.create_topic::<Greeting>("Circle").unwrap();
    let writer = p1
        .create_publisher()
        .unwrap()
        .create_datawriter(&t1)
        .unwrap();
    let _reader = p2
        .create_subscriber()
        .unwrap()
        .create_datareader(&t2)
        .unwrap();
    assert!(
        wait_until(|| !writer.matched_subscriptions().is_empty()),
        "not matched"
    );
    // Room for the secure participant data to arrive.
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(p2.discovered_participants().len(), 1);
    let start = Instant::now();
    drop(p1);
    assert!(
        wait_until(|| p2.discovered_participants().is_empty()),
        "p1 still known"
    );
    assert!(start.elapsed() < Duration::from_secs(3));
}

fn pair(p1: &DomainParticipant, p2: &DomainParticipant) -> bool {
    let t1 = p1.create_topic::<Greeting>("Circle").unwrap();
    let t2 = p2.create_topic::<Greeting>("Circle").unwrap();
    let writer = p1
        .create_publisher()
        .unwrap()
        .create_datawriter(&t1)
        .unwrap();
    let _reader = p2
        .create_subscriber()
        .unwrap()
        .create_datareader(&t2)
        .unwrap();
    wait_until(|| !writer.matched_subscriptions().is_empty())
}

fn participant_with(
    domain: u32,
    security: SecurityConfig,
) -> rustorb_dds::Result<DomainParticipant> {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    c.shared_memory = false;
    c.security = Some(security);
    DomainParticipant::with_config(c)
}

/// A private key encrypted with a password (`dds.sec.auth.password`),
/// given as a property or in the URI of the key.
#[test]
fn encrypted_private_keys() {
    let mut s1 = security("p1");
    s1.private_key = format!("file:{CERTS}p1_key_pkcs8_enc.pem");
    s1.password = Some("OpenSesame".into());
    let mut s2 = security("p2");
    s2.private_key = format!("file:{CERTS}p2_key_dek.pem?password=OpenSesame");
    let p1 = participant_with(36, s1.clone()).unwrap();
    let p2 = participant_with(36, s2).unwrap();
    assert!(pair(&p1, &p2), "not matched");
    s1.password = Some("wrong".into());
    match participant_with(36, s1) {
        Err(Error::Security(e)) => assert!(e.contains("does not decrypt"), "{e}"),
        other => panic!("created: {:?}", other.map(|_| ())),
    }
}

/// A CRL of the identity CA (`rustorb.dds.sec.auth.crl`) that revokes
/// participant2: participant1 does not authenticate it, and
/// participant2 cannot be created with it.
#[test]
fn revoked_certificates_fail_authentication() {
    let crl = format!("file:{CERTS}crl_p2.pem");
    let mut s1 = security("p1");
    s1.crl = Some(crl.clone());
    let p1 = participant_with(37, s1).unwrap();
    let p2 = participant(37, Some("p2")).unwrap();
    assert!(!pair(&p1, &p2), "a revoked participant matched");
    let mut s2 = security("p2");
    s2.crl = Some(crl);
    match participant_with(37, s2) {
        Err(Error::Security(e)) => assert!(e.contains("revoked"), "{e}"),
        other => panic!("created: {:?}", other.map(|_| ())),
    }
}

/// The password and the CRL come from the PROPERTY as well.
#[test]
fn password_and_crl_properties() {
    let mut c = ParticipantConfig::new(0);
    let s = security("p1");
    let props = [
        ("dds.sec.auth.identity_ca", s.identity_ca.as_str()),
        (
            "dds.sec.auth.identity_certificate",
            s.identity_certificate.as_str(),
        ),
        ("dds.sec.auth.private_key", s.private_key.as_str()),
        ("dds.sec.auth.password", "OpenSesame"),
        ("rustorb.dds.sec.auth.crl", "file:crl.pem"),
        ("dds.sec.access.permissions_ca", s.permissions_ca.as_str()),
        ("dds.sec.access.governance", s.governance.as_str()),
        ("dds.sec.access.permissions", s.permissions.as_str()),
    ];
    c.set_properties(props).unwrap();
    let got = c.security.unwrap();
    assert_eq!(got.password.as_deref(), Some("OpenSesame"));
    assert_eq!(got.crl.as_deref(), Some("file:crl.pem"));
}

/// The builtin logging plugin (DDS:Logging:DDS_LogTopic): participant1
/// logs to a file and distributes on DDS:Security:LogTopicV2, which
/// participant2 is granted to read and participant1 is not.
#[test]
fn security_events_are_logged() {
    use rustorb_dds::security_log::{BuiltinLoggingTypeV2, LOG_TOPIC, LogOptions, LoggingLevel};
    let file = std::env::temp_dir().join(format!("rustorb-dds-seclog-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&file);
    let mut s1 = security("p1");
    s1.logging = Some(LogOptions {
        log_level: LoggingLevel::InformationalLevel,
        log_file: Some(file.clone()),
        distribute: true,
    });
    let p1 = participant_with(38, s1).unwrap();
    let p2 = participant(38, Some("p2")).unwrap();
    let reader = p2
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &p2.create_topic::<BuiltinLoggingTypeV2>(LOG_TOPIC).unwrap(),
            DataReaderQos {
                reliability: rustorb_dds::Reliability::reliable(),
                ..keep_all()
            },
        )
        .unwrap();
    let topic = p1.create_topic::<BuiltinLoggingTypeV2>(LOG_TOPIC).unwrap();
    let sub = p1.create_subscriber().unwrap();
    let mut got = None;
    assert!(
        wait_until(|| {
            // Not granted: refused, and that is logged.
            match sub.create_datareader(&topic) {
                Err(Error::Security(_)) => {}
                other => panic!("created: {:?}", other.map(|_| ())),
            }
            got = reader
                .take()
                .into_iter()
                .filter_map(|s| s.data)
                .find(|m| m.msgid == "AccessControl.check_create_datareader");
            got.is_some()
        }),
        "no log message came"
    );
    let m = got.unwrap();
    assert_eq!(m.facility, 10);
    assert_eq!(m.severity, LoggingLevel::ErrorLevel);
    assert!(
        m.message.contains("subscribe to DDS:Security:LogTopicV2"),
        "{}",
        m.message
    );
    let dds = &m.structured_data[0];
    assert_eq!(dds.key, "DDS");
    let g = p1.guid().to_bytes();
    let hex: String = g.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(dds.value[0].value.replace('.', ""), hex);
    assert_eq!(dds.value[1].value, "38");
    let text = std::fs::read_to_string(&file).unwrap();
    assert!(
        text.contains("INFORMATIONAL_LEVEL Authentication.validate_local_identity"),
        "{text}"
    );
    assert!(text.contains("authenticated as"), "{text}");
    assert!(
        text.contains("ERROR_LEVEL AccessControl.check_create_datareader"),
        "{text}"
    );
    drop(p1);
    let _ = std::fs::remove_file(&file);
}

/// The logging options come from the PROPERTY too.
#[test]
fn logging_properties() {
    use rustorb_dds::security_log::{LogOptions, LoggingLevel};
    let s = security("p1");
    let mut props = vec![
        ("dds.sec.auth.identity_ca", s.identity_ca.clone()),
        (
            "dds.sec.auth.identity_certificate",
            s.identity_certificate.clone(),
        ),
        ("dds.sec.auth.private_key", s.private_key.clone()),
        ("dds.sec.access.permissions_ca", s.permissions_ca.clone()),
        ("dds.sec.access.governance", s.governance.clone()),
        ("dds.sec.access.permissions", s.permissions.clone()),
        (LogOptions::LOGGING_LEVEL, "NOTICE_LEVEL".into()),
        (LogOptions::DISTRIBUTE, "true".into()),
    ];
    let mut c = ParticipantConfig::new(0);
    c.set_properties(props.iter().map(|(n, v)| (*n, v.as_str())))
        .unwrap();
    let l = c.security.clone().unwrap().logging.unwrap();
    assert_eq!(l.log_level, LoggingLevel::NoticeLevel);
    assert!(l.distribute);
    assert_eq!(l.log_file, None);
    props.push((LogOptions::DISTRIBUTE, "sometimes".into()));
    let e = c
        .set_properties(props.iter().map(|(n, v)| (*n, v.as_str())))
        .unwrap_err();
    assert!(e.to_string().contains("distribute"), "{e}");
}
