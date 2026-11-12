//! Signed governance and permissions documents.

use rustorb_dds_security::pki::Cert;
use rustorb_dds_security::smime::verify;

const D: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/certs/");

fn read(f: &str) -> Vec<u8> {
    std::fs::read(format!("{D}{f}")).unwrap()
}

#[test]
fn signed_documents_verify() {
    let ca = Cert::parse(&read("permissions_ca.pem")).unwrap();
    for f in ["governance", "permissions_p1", "permissions_p2"] {
        let content = verify(&read(&format!("{f}.p7s")), &ca).unwrap();
        let xml = String::from_utf8(content).unwrap();
        assert!(xml.starts_with("<?xml"), "{f}: {xml:.40}");
        assert_eq!(
            xml.replace("\r\n", "\n").trim_end(),
            String::from_utf8(read(&format!("{f}.xml")))
                .unwrap()
                .replace("\r\n", "\n")
                .trim_end()
        );
    }
}

#[test]
fn tampered_or_foreign_documents_do_not() {
    let ca = Cert::parse(&read("permissions_ca.pem")).unwrap();
    let doc = String::from_utf8(read("governance.p7s")).unwrap();
    let tampered = doc.replace(
        "<allow_unauthenticated_participants>false",
        "<allow_unauthenticated_participants>true",
    );
    assert!(verify(tampered.as_bytes(), &ca).is_err());
    let other = Cert::parse(&read("identity_ca.pem")).unwrap();
    assert!(verify(doc.as_bytes(), &other).is_err());
}
