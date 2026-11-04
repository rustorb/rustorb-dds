//! DDS-XTypes in matching: TypeInformation in discovery, type
//! consistency, DATA_REPRESENTATION.

#[path = "../examples/hello_world/greeting.rs"]
mod greeting;
#[path = "../examples/keyed/shape.rs"]
mod shape;
#[allow(dead_code)]
#[path = "../examples/evolve/v1.rs"]
mod v1;
#[allow(dead_code)]
#[path = "../examples/evolve/v2.rs"]
mod v2;

use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{
    DATA_REPRESENTATION_QOS_POLICY_ID, DataReaderQos, DataWriterQos, DomainParticipant, History,
    ParticipantConfig, Reliability, TypeConsistency, data_representation,
};
use shape::Shape;

const WAIT: Duration = Duration::from_secs(10);

fn participant(domain: u32) -> DomainParticipant {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    DomainParticipant::with_config(c).unwrap()
}

fn reliable_reader() -> DataReaderQos {
    DataReaderQos {
        reliability: Reliability::reliable(),
        history: History::KeepAll,
        ..DataReaderQos::default()
    }
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Writes greetings until the reader has `n`, and returns them.
fn exchange(
    writer: &rustorb_dds::DataWriter<Greeting>,
    reader: &rustorb_dds::DataReader<Greeting>,
) -> Vec<Greeting> {
    assert!(writer.wait_for_subscriptions(1, WAIT));
    assert!(reader.wait_for_publications(1, WAIT));
    for index in 1..=3 {
        writer
            .write(&Greeting {
                index,
                message: "hi".into(),
            })
            .unwrap();
    }
    let mut got = Vec::new();
    wait_until("samples", || {
        got.extend(reader.take().into_iter().filter_map(|s| s.data));
        got.len() >= 3
    });
    got
}

#[test]
fn registered_types_match_by_type_information() {
    let (a, b) = (participant(150), participant(150));
    a.register_type::<Greeting>().unwrap();
    b.register_type::<Greeting>().unwrap();
    // Registering again does nothing.
    b.register_type::<Greeting>().unwrap();
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter(&a.create_topic::<Greeting>("xt").unwrap())
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &b.create_topic::<Greeting>("xt").unwrap(),
            reliable_reader(),
        )
        .unwrap();
    assert_eq!(exchange(&writer, &reader).len(), 3);
}

#[test]
fn one_side_without_type_information_matches_by_name() {
    let (a, b) = (participant(151), participant(151));
    a.register_type::<Greeting>().unwrap();
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter(&a.create_topic::<Greeting>("xt").unwrap())
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &b.create_topic::<Greeting>("xt").unwrap(),
            reliable_reader(),
        )
        .unwrap();
    assert_eq!(exchange(&writer, &reader).len(), 3);
}

#[test]
fn force_type_validation_needs_type_information() {
    let (a, b) = (participant(152), participant(152));
    b.register_type::<Greeting>().unwrap();
    let topic_a = a.create_topic::<Greeting>("xt").unwrap();
    let topic_b = b.create_topic::<Greeting>("xt").unwrap();
    let _writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter(&topic_a)
        .unwrap();
    let qos = DataReaderQos {
        type_consistency: TypeConsistency {
            force_type_validation: true,
            ..TypeConsistency::default()
        },
        ..reliable_reader()
    };
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&topic_b, qos)
        .unwrap();
    wait_until("INCONSISTENT_TOPIC", || {
        topic_b.inconsistent_topic_status().total_count == 1
    });
    // The writer's side sees the reader's policy too.
    wait_until("INCONSISTENT_TOPIC of the writer", || {
        topic_a.inconsistent_topic_status().total_count == 1
    });
    assert!(reader.matched_publications().is_empty());
}

#[test]
fn different_type_names_are_inconsistent_topics() {
    let (a, b) = (participant(153), participant(153));
    let topic_a = a.create_topic::<Greeting>("xt").unwrap();
    let topic_b = b.create_topic::<Shape>("xt").unwrap();
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter(&topic_a)
        .unwrap();
    let _reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader(&topic_b)
        .unwrap();
    wait_until("INCONSISTENT_TOPIC", || {
        topic_b.inconsistent_topic_status().total_count == 1
    });
    wait_until("INCONSISTENT_TOPIC of the writer", || {
        topic_a.inconsistent_topic_status().total_count_change == 1
    });
    // Read: the change count is reset.
    let s = topic_a.inconsistent_topic_status();
    assert_eq!((s.total_count, s.total_count_change), (1, 0));
    assert!(writer.matched_subscriptions().is_empty());
}

#[test]
fn types_that_are_not_assignable_do_not_match_locally() {
    // In one participant, every registered type is known: a Shape reader
    // is not assignable from a Greeting writer (FINAL types with other
    // members), whatever their names.
    let p = participant(154);
    p.register_type::<Greeting>().unwrap();
    p.register_type::<Shape>().unwrap();
    let writer = p
        .create_publisher()
        .unwrap()
        .create_datawriter(&p.create_topic::<Greeting>("xt").unwrap())
        .unwrap();
    let reader = p
        .create_subscriber()
        .unwrap()
        .create_datareader(&p.create_topic::<Shape>("xt").unwrap())
        .unwrap();
    assert!(writer.matched_subscriptions().is_empty());
    assert!(reader.matched_publications().is_empty());
}

#[test]
fn xcdr2_writers() {
    let (a, b) = (participant(155), participant(155));
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(
            &a.create_topic::<Greeting>("xt").unwrap(),
            DataWriterQos {
                data_representation: vec![data_representation::XCDR2],
                ..DataWriterQos::default()
            },
        )
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &b.create_topic::<Greeting>("xt").unwrap(),
            reliable_reader(),
        )
        .unwrap();
    assert_eq!(exchange(&writer, &reader).len(), 3);
    // A reader that takes XCDR only does not match it.
    let xcdr_only = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &b.create_topic::<Greeting>("xt").unwrap(),
            DataReaderQos {
                data_representation: vec![data_representation::XCDR],
                ..DataReaderQos::default()
            },
        )
        .unwrap();
    wait_until("REQUESTED_INCOMPATIBLE_QOS", || {
        xcdr_only.requested_incompatible_qos_status().last_policy_id
            == Some(DATA_REPRESENTATION_QOS_POLICY_ID)
    });
    // An XML writer cannot be made.
    assert!(
        a.create_publisher()
            .unwrap()
            .create_datawriter_with_qos(
                &a.create_topic::<Greeting>("xt").unwrap(),
                DataWriterQos {
                    data_representation: vec![data_representation::XML],
                    ..DataWriterQos::default()
                },
            )
            .is_err()
    );
}

#[test]
fn other_versions_of_a_type_come_by_type_lookup() {
    let (a, b) = (participant(156), participant(156));
    a.register_type::<v1::evolve::Mutable>().unwrap();
    b.register_type::<v2::evolve::Mutable>().unwrap();
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(
            &a.create_topic::<v1::evolve::Mutable>("xt").unwrap(),
            DataWriterQos {
                data_representation: vec![data_representation::XCDR2],
                ..DataWriterQos::default()
            },
        )
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &b.create_topic::<v2::evolve::Mutable>("xt").unwrap(),
            reliable_reader(),
        )
        .unwrap();
    assert!(writer.wait_for_subscriptions(1, WAIT));
    assert!(reader.wait_for_publications(1, WAIT));
    // Each side asked the other for its TypeObject to check it.
    assert!(a.learnt_types() >= 1);
    assert!(b.learnt_types() >= 1);
    writer
        .write(&v1::evolve::Mutable {
            number: 7,
            text: "seven".into(),
        })
        .unwrap();
    let mut got = Vec::new();
    wait_until("the sample", || {
        got.extend(reader.take().into_iter().filter_map(|s| s.data));
        !got.is_empty()
    });
    // The member version 1 does not have takes its default.
    assert_eq!(
        got[0],
        v2::evolve::Mutable {
            extra: 0,
            number: 7,
            text: "seven".into(),
        }
    );
}

#[test]
fn types_that_come_by_type_lookup_can_be_inconsistent() {
    // An APPENDABLE writer and a MUTABLE reader: not assignable, which
    // each side finds once it has the other's TypeObject.
    let (a, b) = (participant(157), participant(157));
    a.register_type::<v1::evolve::Appendable>().unwrap();
    b.register_type::<v1::evolve::Mutable>().unwrap();
    let topic_a = a.create_topic::<v1::evolve::Appendable>("xt").unwrap();
    let topic_b = b.create_topic::<v1::evolve::Mutable>("xt").unwrap();
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter(&topic_a)
        .unwrap();
    let _reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader(&topic_b)
        .unwrap();
    wait_until("INCONSISTENT_TOPIC", || {
        topic_b.inconsistent_topic_status().total_count == 1
    });
    wait_until("INCONSISTENT_TOPIC of the writer", || {
        topic_a.inconsistent_topic_status().total_count == 1
    });
    assert!(writer.matched_subscriptions().is_empty());
    assert!(a.learnt_types() >= 1 && b.learnt_types() >= 1);
}
