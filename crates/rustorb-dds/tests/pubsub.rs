//! Writers and readers find each other and exchange samples, within one
//! participant and between two participants over UDP on this host.

#[path = "../examples/hello_world/greeting.rs"]
mod greeting;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{DataReaderQos, DomainParticipant, History, ParticipantConfig, TopicType};

const WAIT: Duration = Duration::from_secs(10);

fn participant(domain: u32) -> DomainParticipant {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    DomainParticipant::with_config(c).unwrap()
}

fn greeting(index: u32) -> Greeting {
    Greeting {
        index,
        message: format!("hello {index}"),
    }
}

fn keep_all() -> DataReaderQos {
    DataReaderQos {
        history: History::KeepAll,
        ..DataReaderQos::default()
    }
}

#[test]
fn payload_is_xcdr1_plain() {
    let p = greeting(7)
        .to_payload(
            rustorb_typesupport::Encoding::Xcdr1,
            rustorb_typesupport::ByteOrder::Little,
        )
        .unwrap();
    // CDR_LE, index, string length 8 with the NUL, "hello 7\0".
    assert_eq!(&p[..4], &[0x00, 0x01, 0x00, 0x00]);
    assert_eq!(&p[4..12], &[7, 0, 0, 0, 8, 0, 0, 0]);
    assert_eq!(&p[12..20], b"hello 7\0");
    assert_eq!(Greeting::from_payload(&p).unwrap(), greeting(7));
}

#[test]
fn same_participant() {
    let p = participant(71);
    let topic = p.create_topic::<Greeting>("local").unwrap();
    let reader = p
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&topic, keep_all())
        .unwrap();
    let writer = p
        .create_publisher()
        .unwrap()
        .create_datawriter(&topic)
        .unwrap();
    assert_eq!(writer.matched_subscriptions(), [reader.guid()]);
    assert_eq!(reader.matched_publications(), [writer.guid()]);

    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    reader.set_listener(move || {
        c.fetch_add(1, Ordering::SeqCst);
    });
    for i in 1..=3 {
        writer.write(&greeting(i)).unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert!(reader.wait_for_data(Duration::ZERO));

    let read = reader.read();
    assert_eq!(read.len(), 3);
    assert!(!read[0].info.read);
    assert!(reader.read()[0].info.read);
    assert!(!reader.wait_for_data(Duration::ZERO));
    let taken: Vec<u32> = reader.take().into_iter().map(|s| s.data.index).collect();
    assert_eq!(taken, [1, 2, 3]);
    assert!(reader.take().is_empty());

    // Another topic name does not match.
    let other = p.create_topic::<Greeting>("other").unwrap();
    let w2 = p
        .create_publisher()
        .unwrap()
        .create_datawriter(&other)
        .unwrap();
    assert!(w2.matched_subscriptions().is_empty());

    // Dropping the writer unmatches it.
    drop(writer);
    assert!(reader.matched_publications().is_empty());
}

#[test]
fn keep_last_keeps_the_newest() {
    let p = participant(72);
    let topic = p.create_topic::<Greeting>("depth").unwrap();
    let qos = DataReaderQos {
        history: History::KeepLast(2),
        ..DataReaderQos::default()
    };
    let reader = p
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&topic, qos)
        .unwrap();
    let writer = p
        .create_publisher()
        .unwrap()
        .create_datawriter(&topic)
        .unwrap();
    for i in 1..=5 {
        writer.write(&greeting(i)).unwrap();
    }
    let taken: Vec<u32> = reader.take().into_iter().map(|s| s.data.index).collect();
    assert_eq!(taken, [4, 5]);
}

#[test]
fn two_participants_over_udp() {
    let a = participant(73);
    let b = participant(73);
    let ta = a.create_topic::<Greeting>("hello").unwrap();
    let tb = b.create_topic::<Greeting>("hello").unwrap();
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter(&ta)
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&tb, keep_all())
        .unwrap();
    assert!(
        writer.wait_for_subscriptions(1, WAIT),
        "writer saw no reader"
    );
    assert!(
        reader.wait_for_publications(1, WAIT),
        "reader saw no writer"
    );
    assert_eq!(a.discovered_participants(), [b.guid().prefix]);

    let (tx, rx) = std::sync::mpsc::channel();
    reader.set_listener(move || {
        let _ = tx.send(());
    });
    for i in 1..=5 {
        writer.write(&greeting(i)).unwrap();
    }
    let mut got = Vec::new();
    while got.len() < 5 {
        rx.recv_timeout(WAIT).expect("samples did not arrive");
        for s in reader.take() {
            assert_eq!(s.info.writer, writer.guid());
            assert!(s.info.source_timestamp.is_some());
            got.push(s.data);
        }
    }
    assert_eq!(got, (1..=5).map(greeting).collect::<Vec<_>>());

    // The writer goes away: the reader hears of it.
    drop(writer);
    let deadline = Instant::now() + WAIT;
    while !reader.matched_publications().is_empty() {
        assert!(Instant::now() < deadline, "unmatch did not arrive");
        std::thread::sleep(Duration::from_millis(20));
    }
}
