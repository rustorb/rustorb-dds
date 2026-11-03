//! DURABILITY, PARTITION, incompatible QoS and leaving a domain
//! (rustorb-dds#19).

#[path = "../examples/hello_world/greeting.rs"]
mod greeting;

use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{
    DataReader, DataReaderQos, DataWriterQos, DomainParticipant, DurabilityKind, Error, History,
    ParticipantConfig, PublisherQos, QosPolicyId, Reliability, SubscriberQos,
};

const WAIT: Duration = Duration::from_secs(10);

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

fn indices(reader: &DataReader<Greeting>, n: usize) -> Vec<u32> {
    let deadline = Instant::now() + WAIT;
    let mut got = Vec::new();
    while got.len() < n && Instant::now() < deadline {
        if reader.wait_for_data(Duration::from_millis(50)) {
            got.extend(
                reader
                    .take()
                    .into_iter()
                    .filter_map(|s| s.data)
                    .map(|d| d.index),
            );
        }
    }
    got
}

fn transient_local_writer() -> DataWriterQos {
    DataWriterQos {
        history: History::KeepLast(3),
        durability: DurabilityKind::TransientLocal,
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

#[test]
fn transient_local_late_joiners() {
    let a = participant(111);
    let b = participant(111);
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(
            &a.create_topic::<Greeting>("tl").unwrap(),
            transient_local_writer(),
        )
        .unwrap();
    // Written before anyone reads: the last 3 are kept.
    for i in 1..=5 {
        writer.write(&greeting(i)).unwrap();
    }
    let topic_b = b.create_topic::<Greeting>("tl").unwrap();
    let sub_b = b.create_subscriber().unwrap();
    let late = sub_b
        .create_datareader_with_qos(&topic_b, reader_qos(DurabilityKind::TransientLocal))
        .unwrap();
    let volatile = sub_b
        .create_datareader_with_qos(&topic_b, reader_qos(DurabilityKind::Volatile))
        .unwrap();
    // A reader in the writer's own participant gets them too.
    let local = a
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &a.create_topic::<Greeting>("tl").unwrap(),
            reader_qos(DurabilityKind::TransientLocal),
        )
        .unwrap();
    assert_eq!(indices(&local, 3), [3, 4, 5]);
    assert!(late.wait_for_publications(1, WAIT));
    assert!(volatile.wait_for_publications(1, WAIT));
    assert_eq!(indices(&late, 3), [3, 4, 5]);
    writer.write(&greeting(6)).unwrap();
    writer.wait_for_acknowledgments(WAIT).unwrap();
    assert_eq!(indices(&late, 1), [6]);
    // The volatile reader gets only what was written after it matched.
    assert_eq!(indices(&volatile, 1), [6]);
}

#[test]
fn partitions_with_wildcards() {
    let a = participant(112);
    let b = participant(112);
    let partition = |names: &[&str]| names.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let writer = a
        .create_publisher_with_qos(PublisherQos {
            partition: partition(&["sensors/front"]),
        })
        .unwrap()
        .create_datawriter(&a.create_topic::<Greeting>("p").unwrap())
        .unwrap();
    let topic_b = b.create_topic::<Greeting>("p").unwrap();
    let reader = |names: &[&str]| {
        b.create_subscriber_with_qos(SubscriberQos {
            partition: partition(names),
        })
        .unwrap()
        .create_datareader(&topic_b)
        .unwrap()
    };
    let wildcard = reader(&["sensors/*"]);
    let other = reader(&["robot", "sensors/back"]);
    let default = reader(&[]);
    assert!(wildcard.wait_for_publications(1, WAIT));
    assert!(writer.wait_for_subscriptions(1, WAIT));
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(writer.matched_subscriptions(), [wildcard.guid()]);
    assert!(other.matched_publications().is_empty());
    assert!(default.matched_publications().is_empty());
}

#[test]
fn incompatible_qos_is_counted() {
    let a = participant(113);
    let b = participant(113);
    let writer_qos = DataWriterQos {
        reliability: Reliability::best_effort(),
        deadline: Some(Duration::from_secs(2)),
        ..DataWriterQos::default()
    };
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(&a.create_topic::<Greeting>("i").unwrap(), writer_qos)
        .unwrap();
    // A reliable reader with a shorter deadline, over UDP and in the same
    // participant.
    let reader_qos = DataReaderQos {
        reliability: Reliability::reliable(),
        deadline: Some(Duration::from_secs(1)),
        ..DataReaderQos::default()
    };
    let remote = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&b.create_topic::<Greeting>("i").unwrap(), reader_qos)
        .unwrap();
    let local = a
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&a.create_topic::<Greeting>("i").unwrap(), reader_qos)
        .unwrap();
    let deadline = Instant::now() + WAIT;
    while remote.requested_incompatible_qos_status().total_count == 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let s = remote.requested_incompatible_qos_status();
    assert_eq!(s.total_count, 1);
    // Read above: no change since.
    assert_eq!(s.total_count_change, 0);
    let s = local.requested_incompatible_qos_status();
    assert_eq!((s.total_count, s.total_count_change), (1, 1));
    assert_eq!(s.last_policy_id, Some(QosPolicyId::DEADLINE));
    let ids: Vec<QosPolicyId> = s.policies.iter().map(|c| c.policy_id).collect();
    assert_eq!(ids, [QosPolicyId::DEADLINE, QosPolicyId::RELIABILITY]);
    while writer.offered_incompatible_qos_status().total_count < 2 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let s = writer.offered_incompatible_qos_status();
    assert_eq!(s.total_count, 2);
    assert!(writer.matched_subscriptions().is_empty());
}

#[test]
fn transient_and_persistent_writers_are_unsupported() {
    let a = participant(114);
    let topic = a.create_topic::<Greeting>("u").unwrap();
    let r = a.create_publisher().unwrap().create_datawriter_with_qos(
        &topic,
        DataWriterQos {
            durability: DurabilityKind::Transient,
            ..DataWriterQos::default()
        },
    );
    assert!(matches!(r, Err(Error::Unsupported(_))));
}

#[test]
fn leaving_tells_the_others() {
    let mut c = ParticipantConfig::localhost(115);
    c.announce_period = Duration::from_millis(200);
    // The lease alone would keep it for a minute.
    c.lease_duration = Duration::from_secs(60);
    let a = DomainParticipant::with_config(c.clone()).unwrap();
    let b = DomainParticipant::with_config(c).unwrap();
    let deadline = Instant::now() + WAIT;
    while a.discovered_participants().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(a.discovered_participants(), [b.guid().prefix]);
    drop(b);
    let deadline = Instant::now() + Duration::from_secs(5);
    while !a.discovered_participants().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(a.discovered_participants().is_empty());
}
