//! Content filtered topics (DDS 1.4, 2.2.2.3.3): readers that get only
//! the samples that pass a filter, between participants and within one.

#[path = "../examples/keyed/shape.rs"]
mod shape;

use std::time::{Duration, Instant};

use rustorb_dds::{
    DataReader, DataReaderQos, DataWriterQos, DomainParticipant, DynamicDataFactory, Error,
    History, ParticipantConfig, Reliability, TopicDescription,
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

fn reliable_writer() -> DataWriterQos {
    DataWriterQos {
        history: History::KeepAll,
        ..DataWriterQos::default()
    }
}

fn shape(id: u32, index: u32) -> Shape {
    Shape {
        id,
        index,
        message: format!("shape {id}"),
    }
}

/// Takes from `reader` until it has `n` samples or the time is up, then
/// a little more to see that no more come.
fn take_n(reader: &DataReader<Shape>, n: usize) -> Vec<Shape> {
    let deadline = Instant::now() + WAIT;
    let mut got = Vec::new();
    while got.len() < n && Instant::now() < deadline {
        got.extend(reader.take().into_iter().filter_map(|s| s.data));
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    got.extend(reader.take().into_iter().filter_map(|s| s.data));
    got
}

fn ids(v: &[Shape]) -> Vec<(u32, u32)> {
    v.iter().map(|s| (s.id, s.index)).collect()
}

#[test]
fn filters_samples_between_participants() {
    let (a, b) = (participant(171), participant(171));
    a.register_type::<Shape>().unwrap();
    b.register_type::<Shape>().unwrap();
    let topic_a = a.create_topic::<Shape>("Shapes").unwrap();
    let topic_b = b.create_topic::<Shape>("Shapes").unwrap();
    let cft = b
        .create_contentfilteredtopic(
            "BigShapes",
            &topic_b,
            "index >= %0 AND message LIKE 'shape%'",
            &["5".into()],
        )
        .unwrap();
    assert_eq!(cft.get_name(), "BigShapes");
    assert_eq!(cft.get_type_name(), "Keyed::Shape");
    assert_eq!(cft.related_topic().name(), "Shapes");
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(&topic_a, reliable_writer())
        .unwrap();
    let sub = b.create_subscriber().unwrap();
    let filtered = sub
        .create_datareader_with_qos(&cft, reliable_reader())
        .unwrap();
    let all = sub
        .create_datareader_with_qos(&topic_b, reliable_reader())
        .unwrap();
    assert!(writer.wait_for_subscriptions(2, WAIT));
    assert!(filtered.wait_for_publications(1, WAIT));
    for index in 0..10 {
        writer.write(&shape(index % 3, index)).unwrap();
    }
    assert_eq!(
        ids(&take_n(&filtered, 5)),
        (5..10).map(|i| (i % 3, i)).collect::<Vec<_>>()
    );
    assert_eq!(take_n(&all, 10).len(), 10);

    // New parameters apply to the samples that come next.
    cft.set_expression_parameters(&["8".into()]).unwrap();
    assert_eq!(cft.expression_parameters(), vec!["8".to_string()]);
    for index in 0..10 {
        writer.write(&shape(index % 3, index)).unwrap();
    }
    assert_eq!(ids(&take_n(&filtered, 2)), vec![(2, 8), (0, 9)]);
    assert!(cft.set_expression_parameters(&[]).is_err());
}

#[test]
fn filters_samples_of_local_writers() {
    let p = participant(172);
    p.register_type::<Shape>().unwrap();
    let topic = p.create_topic::<Shape>("Local").unwrap();
    let cft = p
        .create_contentfilteredtopic("Odd", &topic, "id = 1 OR id = 3", &[])
        .unwrap();
    let writer = p
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(&topic, reliable_writer())
        .unwrap();
    let reader = p
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&cft, reliable_reader())
        .unwrap();
    assert!(writer.wait_for_subscriptions(1, WAIT));
    for id in 0..5 {
        writer.write(&shape(id, 0)).unwrap();
    }
    assert_eq!(ids(&take_n(&reader, 2)), vec![(1, 0), (3, 0)]);
}

#[test]
fn filters_dynamic_data() {
    let p = participant(173);
    let ty = rustorb_dds::DynamicType::new(
        <Shape as rustorb_xtypes::types::DescribeType>::describe_type(),
    );
    let topic = p.create_dynamic_topic("Dyn", &ty).unwrap();
    let cft = p
        .create_contentfilteredtopic("DynBig", &topic, "index BETWEEN 2 AND 3", &[])
        .unwrap();
    let writer = p
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(&topic, reliable_writer())
        .unwrap();
    let reader = p
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&cft, reliable_reader())
        .unwrap();
    assert!(writer.wait_for_subscriptions(1, WAIT));
    for index in 0..5 {
        let mut d = DynamicDataFactory::create_data(&ty);
        let id = d.get_member_id_by_name("index");
        d.set_uint32_value(id, index).unwrap();
        writer.write(&d).unwrap();
    }
    let deadline = Instant::now() + WAIT;
    let mut got = Vec::new();
    while got.len() < 2 && Instant::now() < deadline {
        got.extend(reader.take().into_iter().filter_map(|s| s.data));
        std::thread::sleep(Duration::from_millis(20));
    }
    let index = |d: &rustorb_dds::DynamicData| {
        d.get_uint32_value(d.get_member_id_by_name("index"))
            .unwrap()
    };
    assert_eq!(got.iter().map(index).collect::<Vec<_>>(), vec![2, 3]);
}

#[test]
fn needs_a_registered_type_and_a_valid_filter() {
    let p = participant(174);
    let topic = p.create_topic::<Shape>("Unregistered").unwrap();
    assert!(matches!(
        p.create_contentfilteredtopic("F", &topic, "id = 1", &[]),
        Err(Error::PreconditionNotMet)
    ));
    p.register_type::<Shape>().unwrap();
    for (expr, params) in [("nope = 1", vec![]), ("id = %1", vec!["1".to_string()])] {
        assert!(matches!(
            p.create_contentfilteredtopic("F", &topic, expr, &params),
            Err(Error::Dynamic(_))
        ));
    }
}
