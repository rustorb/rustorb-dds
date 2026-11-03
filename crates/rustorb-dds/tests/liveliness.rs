//! LIVELINESS and DEADLINE (rustorb-dds#19).

#[path = "../examples/hello_world/greeting.rs"]
mod greeting;

use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{
    DataReader, DataReaderQos, DataWriter, DataWriterQos, DomainParticipant, InstanceState,
    Liveliness, LivelinessKind, ParticipantConfig, Reliability,
};

const WAIT: Duration = Duration::from_secs(10);
const LEASE: Duration = Duration::from_millis(300);

fn participant(domain: u32) -> DomainParticipant {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    c.writer.heartbeat_period = Duration::from_millis(50);
    DomainParticipant::with_config(c).unwrap()
}

fn greeting(index: u32) -> Greeting {
    Greeting {
        index,
        message: format!("hello {index}"),
    }
}

fn writer_qos(kind: LivelinessKind) -> DataWriterQos {
    DataWriterQos {
        liveliness: Liveliness {
            kind,
            lease_duration: Some(LEASE),
        },
        ..DataWriterQos::default()
    }
}

fn reader_qos() -> DataReaderQos {
    DataReaderQos {
        reliability: Reliability::reliable(),
        ..DataReaderQos::default()
    }
}

/// A writer in `a` and a reader in `b`, matched, on topic `topic`.
fn pair(
    a: &DomainParticipant,
    b: &DomainParticipant,
    topic: &str,
    writer_qos: DataWriterQos,
    reader_qos: DataReaderQos,
) -> (DataWriter<Greeting>, DataReader<Greeting>) {
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(&a.create_topic::<Greeting>(topic).unwrap(), writer_qos)
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&b.create_topic::<Greeting>(topic).unwrap(), reader_qos)
        .unwrap();
    assert!(writer.wait_for_subscriptions(1, WAIT));
    assert!(reader.wait_for_publications(1, WAIT));
    (writer, reader)
}

/// Waits until `f` holds, or panics after a while.
fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn automatic_writers_stay_alive() {
    let a = participant(121);
    let b = participant(121);
    let (writer, reader) = pair(
        &a,
        &b,
        "auto",
        writer_qos(LivelinessKind::Automatic),
        reader_qos(),
    );
    // Several leases without writing: the participant asserts it.
    std::thread::sleep(LEASE * 5);
    let s = reader.liveliness_changed_status();
    assert_eq!((s.alive_count, s.not_alive_count), (1, 0));
    assert_eq!(s.last_publication_handle, Some(writer.guid()));
    assert_eq!(writer.liveliness_lost_status().total_count, 0);
}

#[test]
fn manual_by_topic_is_lost_and_asserted_again() {
    let a = participant(122);
    let b = participant(122);
    let (writer, reader) = pair(
        &a,
        &b,
        "topic",
        writer_qos(LivelinessKind::ManualByTopic),
        reader_qos(),
    );
    // A reader in the writer's own participant sees the same.
    let local = a
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&a.create_topic::<Greeting>("topic").unwrap(), reader_qos())
        .unwrap();
    // Matching may take longer than the lease, so the writer may have
    // been lost already: writing brings it back.
    let lost_before = writer.liveliness_lost_status().total_count;
    writer.write(&greeting(1)).unwrap();
    for r in [&reader, &local] {
        wait_until("the sample", || r.wait_for_data(Duration::from_millis(50)));
        assert_eq!(r.take().len(), 1);
    }
    // Nothing for longer than the lease: lost.
    for r in [&reader, &local] {
        wait_until("not alive", || {
            r.liveliness_changed_status().not_alive_count == 1
        });
        let s = r.liveliness_changed_status();
        assert_eq!(s.alive_count, 0);
        // The instance has no live writer left.
        let samples = r.take();
        assert_eq!(samples.len(), 1);
        assert!(samples[0].data.is_none());
        assert_eq!(
            samples[0].info.instance_state,
            InstanceState::NotAliveNoWriters
        );
    }
    let s = writer.liveliness_lost_status();
    assert_eq!((s.total_count, s.total_count_change), (lost_before + 1, 1));
    assert_eq!(writer.liveliness_lost_status().total_count_change, 0);
    // assert_liveliness sends a HEARTBEAT with the liveliness flag.
    writer.assert_liveliness().unwrap();
    for r in [&reader, &local] {
        wait_until("alive again", || {
            let s = r.liveliness_changed_status();
            s.alive_count == 1 && s.not_alive_count == 0
        });
    }
    // Writing asserts it too, and keeps the instance alive.
    let deadline = Instant::now() + LEASE * 4;
    let mut i = 2;
    while Instant::now() < deadline {
        writer.write(&greeting(i)).unwrap();
        i += 1;
        std::thread::sleep(LEASE / 4);
    }
    assert_eq!(writer.liveliness_lost_status().total_count, lost_before + 1);
    let s = reader.liveliness_changed_status();
    assert_eq!((s.alive_count, s.not_alive_count), (1, 0));
}

#[test]
fn manual_by_participant() {
    let a = participant(123);
    let b = participant(123);
    let (writer, reader) = pair(
        &a,
        &b,
        "participant",
        writer_qos(LivelinessKind::ManualByParticipant),
        reader_qos(),
    );
    // The participant asserts it for its writers (matching may have
    // taken longer than the lease).
    let lost_before = writer.liveliness_lost_status().total_count;
    let until = Instant::now() + LEASE * 4;
    while Instant::now() < until {
        a.assert_liveliness().unwrap();
        std::thread::sleep(LEASE / 5);
    }
    let s = reader.liveliness_changed_status();
    assert_eq!((s.alive_count, s.not_alive_count), (1, 0));
    assert_eq!(writer.liveliness_lost_status().total_count, lost_before);
    // Then it stops.
    wait_until("not alive", || {
        reader.liveliness_changed_status().not_alive_count == 1
    });
    assert_eq!(writer.liveliness_lost_status().total_count, lost_before + 1);
    // The writer's own assert_liveliness asserts the participant.
    writer.assert_liveliness().unwrap();
    wait_until("alive again", || {
        reader.liveliness_changed_status().alive_count == 1
    });
}

#[test]
fn deleted_writers_are_not_counted_as_not_alive() {
    let a = participant(124);
    let b = participant(124);
    let (writer, reader) = pair(
        &a,
        &b,
        "deleted",
        writer_qos(LivelinessKind::Automatic),
        reader_qos(),
    );
    assert_eq!(reader.liveliness_changed_status().alive_count, 1);
    drop(writer);
    wait_until("unmatched", || reader.matched_publications().is_empty());
    let s = reader.liveliness_changed_status();
    assert_eq!((s.alive_count, s.not_alive_count), (0, 0));
    assert_eq!(s.alive_count_change, -1);
}

#[test]
fn deadlines() {
    let a = participant(125);
    let b = participant(125);
    let period = Duration::from_millis(200);
    let (writer, reader) = pair(
        &a,
        &b,
        "deadline",
        DataWriterQos {
            deadline: Some(period),
            ..DataWriterQos::default()
        },
        DataReaderQos {
            deadline: Some(period * 2),
            ..reader_qos()
        },
    );
    // Written often enough: no deadline missed.
    let until = Instant::now() + period * 5;
    let mut i = 1;
    while Instant::now() < until {
        writer.write(&greeting(i)).unwrap();
        i += 1;
        std::thread::sleep(period / 4);
    }
    assert_eq!(writer.offered_deadline_missed_status().total_count, 0);
    assert_eq!(reader.requested_deadline_missed_status().total_count, 0);
    // Then not at all: one miss per period.
    std::thread::sleep(period * 5);
    let w = writer.offered_deadline_missed_status();
    assert!((4..=6).contains(&w.total_count), "{w:?}");
    assert!(w.last_instance_handle.is_some());
    let r = reader.requested_deadline_missed_status();
    assert!((1..=3).contains(&r.total_count), "{r:?}");
    // A disposed instance is not expected to be written.
    writer.dispose(&greeting(0)).unwrap();
    let before = writer.offered_deadline_missed_status().total_count;
    std::thread::sleep(period * 3);
    let after = writer.offered_deadline_missed_status().total_count;
    assert_eq!(before, after);
}
