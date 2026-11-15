//! TRANSIENT and PERSISTENT durability: samples that outlive their
//! writer, in the participant or in a file (rustorb-dds#71).

#[path = "../examples/keyed/shape.rs"]
mod shape;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rustorb_dds::{
    DataReader, DataReaderQos, DataWriterQos, DomainParticipant, DurabilityKind, DurabilityService,
    History, InstanceState, ParticipantConfig, Reliability, ResourceLimits,
};
use shape::Shape;

const WAIT: Duration = Duration::from_secs(10);

fn config(domain: u32) -> ParticipantConfig {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    c.writer.heartbeat_period = Duration::from_millis(50);
    c
}

fn participant(domain: u32) -> DomainParticipant {
    DomainParticipant::with_config(config(domain)).unwrap()
}

fn persistent_participant(domain: u32, dir: &Path) -> DomainParticipant {
    let mut c = config(domain);
    c.persistence_directory = Some(dir.to_path_buf());
    DomainParticipant::with_config(c).unwrap()
}

fn temp_dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("rustorb-dds-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

fn shape(id: u32, index: u32) -> Shape {
    Shape {
        id,
        index,
        message: format!("shape {id}"),
    }
}

fn writer_qos(durability: DurabilityKind) -> DataWriterQos {
    DataWriterQos {
        history: History::KeepLast(3),
        durability,
        durability_service: DurabilityService {
            history: History::KeepLast(2),
            ..DurabilityService::default()
        },
        ..DataWriterQos::default()
    }
}

fn reader_qos(durability: DurabilityKind) -> DataReaderQos {
    DataReaderQos {
        reliability: Reliability::reliable(),
        history: History::KeepAll,
        durability,
        ..DataReaderQos::default()
    }
}

fn reader(p: &DomainParticipant, topic: &str, durability: DurabilityKind) -> DataReader<Shape> {
    p.create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &p.create_topic::<Shape>(topic).unwrap(),
            reader_qos(durability),
        )
        .unwrap()
}

/// The (id, index) of the values taken until `n` came or the wait ran
/// out, and how many instances were seen disposed.
fn take(reader: &DataReader<Shape>, n: usize, wait: Duration) -> (Vec<(u32, u32)>, usize) {
    let deadline = Instant::now() + wait;
    let mut got = Vec::new();
    let mut disposed = 0;
    while got.len() < n && Instant::now() < deadline {
        if reader.wait_for_data(Duration::from_millis(50)) {
            for s in reader.take() {
                match s.data {
                    Some(d) => got.push((d.id, d.index)),
                    None if s.info.instance_state == InstanceState::NotAliveDisposed => {
                        disposed += 1
                    }
                    None => {}
                }
            }
        }
    }
    got.sort();
    (got, disposed)
}

#[test]
fn transient_samples_outlive_the_writer() {
    let a = participant(24);
    let b = participant(24);
    let topic = a.create_topic::<Shape>("transient").unwrap();
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(&topic, writer_qos(DurabilityKind::Transient))
        .unwrap();
    for i in 1..=5 {
        writer.write(&shape(1, i)).unwrap();
        writer.write(&shape(2, i)).unwrap();
    }
    // The writer keeps 3 of each instance; once deleted, the participant
    // keeps the 2 its DURABILITY_SERVICE allows.
    drop(writer);
    let expected = vec![(1, 4), (1, 5), (2, 4), (2, 5)];
    // A late reader in another participant gets them, and so does one
    // in the same participant.
    let remote = reader(&b, "transient", DurabilityKind::Transient);
    assert_eq!(take(&remote, 4, WAIT), (expected.clone(), 0));
    let local = reader(&a, "transient", DurabilityKind::TransientLocal);
    assert_eq!(take(&local, 4, WAIT), (expected, 0));
    // A VOLATILE one does not.
    let volatile = reader(&b, "transient", DurabilityKind::Volatile);
    assert_eq!(take(&volatile, 1, Duration::from_millis(500)).0, vec![]);
    // The instances are not unregistered nor disposed by the deletion.
    assert!(take(&remote, 1, Duration::from_millis(300)).0.is_empty());
    for s in remote.read() {
        assert_eq!(s.info.instance_state, InstanceState::Alive);
    }
}

#[test]
fn disposed_instances_go_after_the_cleanup_delay() {
    let a = participant(85);
    let b = participant(85);
    let topic = a.create_topic::<Shape>("cleanup").unwrap();
    let mut qos = writer_qos(DurabilityKind::Transient);
    qos.durability_service.service_cleanup_delay = Duration::from_millis(800);
    qos.durability_service.resource_limits = ResourceLimits {
        max_instances: Some(2),
        ..ResourceLimits::default()
    };
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(&topic, qos)
        .unwrap();
    for id in 1..=3 {
        writer.write(&shape(id, 1)).unwrap();
    }
    writer.dispose(&shape(3, 0)).unwrap();
    drop(writer);
    // Instance 1, changed longest ago, is beyond max_instances; disposed
    // instance 3 stays for the cleanup delay.
    let early = reader(&b, "cleanup", DurabilityKind::Transient);
    let (got, disposed) = take(&early, 2, WAIT);
    let deadline = Instant::now() + Duration::from_millis(300);
    let mut disposed = disposed;
    while disposed == 0 && Instant::now() < deadline {
        disposed += take(&early, 1, Duration::from_millis(50)).1;
    }
    assert_eq!(got, vec![(2, 1), (3, 1)]);
    assert_eq!(disposed, 1);
    std::thread::sleep(Duration::from_millis(1000));
    let late = reader(&b, "cleanup", DurabilityKind::Transient);
    assert_eq!(take(&late, 2, Duration::from_secs(2)), (vec![(2, 1)], 0));
}

#[test]
fn persistent_samples_outlive_the_program() {
    let dir = temp_dir("persistent");
    {
        let a = persistent_participant(86, &dir);
        let topic = a.create_topic::<Shape>("persistent").unwrap();
        let writer = a
            .create_publisher()
            .unwrap()
            .create_datawriter_with_qos(&topic, writer_qos(DurabilityKind::Persistent))
            .unwrap();
        for i in 1..=4 {
            writer.write(&shape(1, i)).unwrap();
        }
        writer.write(&shape(2, 1)).unwrap();
        writer.dispose(&shape(2, 0)).unwrap();
        // The participant is deleted with the writer.
    }
    // As after a restart: a new PERSISTENT writer of the topic takes up
    // the samples, and sends them to a reader there before it...
    let b = persistent_participant(86, &dir);
    let c = participant(86);
    let early = reader(&b, "persistent", DurabilityKind::TransientLocal);
    let topic = b.create_topic::<Shape>("persistent").unwrap();
    let publisher = b.create_publisher().unwrap();
    let writer = publisher
        .create_datawriter_with_qos(&topic, writer_qos(DurabilityKind::Persistent))
        .unwrap();
    let (got, disposed) = take(&early, 4, Duration::from_secs(1));
    assert_eq!(got, vec![(1, 2), (1, 3), (1, 4), (2, 1)]);
    assert_eq!(disposed + take(&early, 1, Duration::from_millis(200)).1, 1);
    // ... and to one that comes after, elsewhere.
    let late = reader(&c, "persistent", DurabilityKind::Persistent);
    assert_eq!(take(&late, 4, WAIT).0, vec![(1, 2), (1, 3), (1, 4), (2, 1)]);
    // A second writer of the topic takes another file, empty.
    let other = publisher
        .create_datawriter_with_qos(&topic, writer_qos(DurabilityKind::Persistent))
        .unwrap();
    other.write(&shape(7, 1)).unwrap();
    writer.write(&shape(1, 5)).unwrap();
    drop(other);
    drop(writer);
    drop(publisher);
    drop(early);
    drop(late);
    drop(b);
    // Each file has kept what its writer wrote.
    let d = persistent_participant(86, &dir);
    let topic = d.create_topic::<Shape>("persistent").unwrap();
    let publisher = d.create_publisher().unwrap();
    let _w1 = publisher
        .create_datawriter_with_qos(&topic, writer_qos(DurabilityKind::Persistent))
        .unwrap();
    let _w2 = publisher
        .create_datawriter_with_qos(&topic, writer_qos(DurabilityKind::Persistent))
        .unwrap();
    let late = reader(&c, "persistent", DurabilityKind::Persistent);
    assert_eq!(
        take(&late, 5, WAIT).0,
        vec![(1, 3), (1, 4), (1, 5), (2, 1), (7, 1)]
    );
    drop(d);
    let _ = std::fs::remove_dir_all(&dir);
}
