//! The PKI-DH handshake between two identities of the test CA.

use rustorb_dds_security::auth::{Handshake, Identity, KeyAgreement, check_prefix, pdata_prefix};
use rustorb_dds_security::crypto::{Decoder, Encoder, KeyMaterial};

const D: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/certs/");

fn identity(n: &str) -> Identity {
    let r = |f: &str| std::fs::read(format!("{D}{f}")).unwrap();
    Identity::new(
        &r("identity_ca.pem"),
        &r(&format!("{n}_cert.pem")),
        &r(&format!("{n}_key.pem")),
    )
    .unwrap()
}

/// A ParticipantBuiltinTopicData of PL_CDR_BE with only the GUID.
fn pdata(prefix: [u8; 12]) -> Vec<u8> {
    let mut b = vec![0, 2, 0, 0, 0x00, 0x50, 0, 16];
    b.extend_from_slice(&prefix);
    b.extend_from_slice(&[0, 0, 1, 0xc1, 0, 1, 0, 0]);
    b
}

fn run(kagree: KeyAgreement) {
    let mut a = identity("p1");
    a.key_agreement = kagree;
    let b = identity("p2");
    let pa = a.adjusted_prefix(&[1; 16]);
    let pb = b.adjusted_prefix(&[2; 16]);
    check_prefix(&pa, a.certificate()).unwrap();
    assert!(check_prefix(&pa, b.certificate()).is_err());
    assert_eq!(pdata_prefix(&pdata(pa)).unwrap(), pa);

    let mut req = Handshake::request(&a, b"perm-a", &pdata(pa), None);
    let rep = Handshake::reply(&b, b"perm-b", &pdata(pb), &req.sent, None, None).unwrap();
    let (fin, at_a) = req.process_reply(&a, &rep.sent).unwrap();
    let at_b = rep.process_final(&fin).unwrap();
    assert_eq!(at_a.shared_secret, at_b.shared_secret);
    assert_eq!(at_a.peer_permissions, b"perm-b");
    assert_eq!(at_b.peer_permissions, b"perm-a");
    assert_eq!(at_a.peer_cert.subject(), b.certificate().subject());

    // Both derive the same key exchange key material.
    let ka = KeyMaterial::key_exchange(&at_a.challenge1, &at_a.challenge2, &at_a.shared_secret);
    let kb = KeyMaterial::key_exchange(&at_b.challenge1, &at_b.challenge2, &at_b.shared_secret);
    let sub = [0x15, 1, 4, 0, 1, 2, 3, 4];
    let c = Encoder::new(ka).encode_submessage(&sub, &[]);
    assert_eq!(Decoder::new(kb).decode_submessage(&c).unwrap().plain, sub);

    // A tampered reply is refused.
    let mut req = Handshake::request(&a, b"perm-a", &pdata(pa), None);
    let mut rep = Handshake::reply(&b, b"perm-b", &pdata(pb), &req.sent, None, None)
        .unwrap()
        .sent;
    for p in &mut rep.binary_properties {
        if p.name == "dh2" {
            p.value[5] ^= 1;
        }
    }
    assert!(req.process_reply(&a, &rep).is_err());
}

#[test]
fn ecdh_handshake() {
    run(KeyAgreement::EcdhP256);
}

#[test]
fn dh_handshake() {
    run(KeyAgreement::Dh2048);
}

#[test]
fn a_guid_of_another_subject_is_refused() {
    let a = identity("p1");
    let b = identity("p2");
    let wrong = b.adjusted_prefix(&[1; 16]);
    let req = Handshake::request(&a, b"", &pdata(wrong), None);
    assert!(
        Handshake::reply(
            &b,
            b"",
            &pdata(b.adjusted_prefix(&[2; 16])),
            &req.sent,
            None,
            None
        )
        .is_err()
    );
}

#[test]
fn certificates_of_other_cas_are_refused() {
    let r = |f: &str| std::fs::read(format!("{D}{f}")).unwrap();
    assert!(
        Identity::new(
            &r("permissions_ca.pem"),
            &r("p1_cert.pem"),
            &r("p1_key.pem")
        )
        .is_err()
    );
    assert!(Identity::new(&r("identity_ca.pem"), &r("p1_cert.pem"), &r("p2_key.pem")).is_err());
}
