//! The signed governance and permissions documents, read and evaluated.

use rustorb_dds_security::access::{Action, ProtectionKind};
use rustorb_dds_security::pki::Cert;
use rustorb_dds_security::smime::verify;
use rustorb_dds_security::xml;

const D: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/certs/");

fn read(f: &str) -> Vec<u8> {
    std::fs::read(format!("{D}{f}")).unwrap()
}

#[test]
fn governance_rules() {
    let ca = Cert::parse(&read("permissions_ca.pem")).unwrap();
    let g = xml::governance(&verify(&read("governance.p7s"), &ca).unwrap()).unwrap();
    let d = g.domain_rule(0, "").unwrap();
    assert!(!d.allow_unauthenticated_participants);
    assert_eq!(d.rtps_protection_kind, ProtectionKind::Sign);
    assert_eq!(d.discovery_protection_kind, ProtectionKind::Encrypt);
    let t = d.topic_rule("Square").unwrap();
    assert_eq!(
        t.metadata_protection_kind,
        ProtectionKind::EncryptWithOriginAuthentication
    );
    assert_eq!(d.topic_rule("Circle").unwrap().topic_expression, "*");
    assert!(g.domain_rule(231, "").is_err());
}

#[test]
fn permission_grants() {
    let ca = Cert::parse(&read("permissions_ca.pem")).unwrap();
    let p = xml::permissions(&verify(&read("permissions_p1.p7s"), &ca).unwrap()).unwrap();
    let now = 1_800_000_000;
    let g = p.grant("CN=participant1,O=rustorb,C=JP", now).unwrap();
    assert_eq!(g.name, "p1");
    assert!(g.allows_join(0));
    assert!(g.allows(0, Action::Publish, "Square", &[]));
    assert!(g.allows(0, Action::Subscribe, "Squares", &[]));
    assert!(!g.allows(0, Action::Publish, "Circle", &[]));
    assert!(!g.allows(231, Action::Publish, "Square", &[]));
    assert!(p.grant("CN=participant2,O=rustorb,C=JP", now).is_err());
    assert!(
        p.grant("CN=participant1,O=rustorb,C=JP", 5_000_000_000)
            .is_err()
    );
}
