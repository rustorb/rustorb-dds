//! Keyed instances: register, write, dispose, unregister and the reader's
//! instance states, within one participant and over UDP
//! (rustorb-dds#17).

#[path = "../examples/keyed/shape.rs"]
mod shape;

use std::time::{Duration, Instant};

use rustorb_dds::{
    DataReader, DataReaderQos, DataWriterQos, DomainParticipant, Error, History, InstanceState,
    InstanceStateMask, ParticipantConfig, Reliability, Sample, SampleState, StateFilter, ViewState,
    WriterDataLifecycle,
};
use shape::Shape;

const WAIT: Duration = Duration::from_secs(10);

fn participant(domain: u32) -> DomainParticipant {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    c.writer.heartbeat_period = Duration::from_millis(50);
    DomainParticipant::with_config(c).unwrap()
}

fn shape(id: u32, index: u32) -> Shape {
    Shape {
        id,
        index,
        message: format!("shape {id}"),
    }
}

fn reliable_keep_all() -> DataReaderQos {
    DataReaderQos {
        reliability: Reliability::reliable(),
        history: History::KeepAll,
        ..DataReaderQos::default()
    }
}

fn writer_qos(autodispose: bool) -> DataWriterQos {
    DataWriterQos {
        history: History::KeepAll,
        writer_data_lifecycle: WriterDataLifecycle {
            autodispose_unregistered_instances: autodispose,
        },
        ..DataWriterQos::default()
    }
}

/// (id or 0, index or 0, instance state) of each sample.
fn summary(r: &DataReader<Shape>, samples: &[Sample<Shape>]) -> Vec<(u32, u32, InstanceState)> {
    samples
        .iter()
        .map(|s| match &s.data {
            Some(d) => (d.id, d.index, s.info.instance_state),
            None => (
                r.get_key_value(s.info.instance_handle).map_or(0, |k| k.id),
                0,
                s.info.instance_state,
            ),
        })
        .collect()
}

/// Takes until `n` samples arrived or the wait runs out.
fn take_n(r: &DataReader<Shape>, n: usize) -> Vec<(u32, u32, InstanceState)> {
    let deadline = Instant::now() + WAIT;
    let mut got = Vec::new();
    while got.len() < n && Instant::now() < deadline {
        if r.wait_for_data(Duration::from_millis(50)) {
            let s = r.take();
            got.extend(summary(r, &s));
        }
    }
    got
}

#[test]
fn instances_in_one_participant() {
    use InstanceState::*;
    let p = participant(11);
    let topic = p.create_topic::<Shape>("shapes").unwrap();
    let reader = p
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &topic,
            DataReaderQos {
                history: History::KeepLast(2),
                ..DataReaderQos::default()
            },
        )
        .unwrap();
    let writer = p
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(&topic, writer_qos(false))
        .unwrap();

    // Unregistering an instance not registered fails.
    assert!(matches!(
        writer.unregister_instance(&shape(9, 0)),
        Err(Error::PreconditionNotMet)
    ));
    let h1 = writer.register_instance(&shape(1, 0)).unwrap();
    assert_eq!(writer.lookup_instance(&shape(1, 99)), Some(h1));
    assert_eq!(writer.lookup_instance(&shape(2, 0)), None);
    // Registering sends nothing.
    assert!(reader.take().is_empty());

    // KEEP_LAST(2) is per instance.
    for i in 1..=3 {
        writer.write(&shape(1, i)).unwrap();
        writer.write(&shape(2, i)).unwrap();
    }
    assert_eq!(reader.lookup_instance(&shape(1, 0)), Some(h1));
    let first = reader.read_next_instance(None, StateFilter::ANY);
    assert_eq!(summary(&reader, &first), [(1, 2, Alive), (1, 3, Alive)]);
    assert_eq!(first[0].info.sample_rank, 1);
    let h2 = reader.lookup_instance(&shape(2, 0)).unwrap();
    let second = reader.read_next_instance(Some(first[0].info.instance_handle), StateFilter::ANY);
    assert_eq!(second[0].info.instance_handle, h2);
    let nothing = reader.read_next_instance(Some(h1.max(h2)), StateFilter::ANY);
    assert!(nothing.is_empty());

    // Dispose 2, unregister 3 (registered by the write), and see the
    // states.
    writer.write(&shape(3, 1)).unwrap();
    writer.dispose(&shape(2, 0)).unwrap();
    writer.unregister_instance(&shape(3, 0)).unwrap();
    assert_eq!(writer.lookup_instance(&shape(3, 0)), None);
    let not_alive = StateFilter {
        instance_states: InstanceStateMask::NOT_ALIVE,
        ..StateFilter::ANY
    };
    let s = reader.take_with(not_alive);
    assert_eq!(
        summary(&reader, &s),
        [
            (2, 3, NotAliveDisposed),
            (3, 1, NotAliveNoWriters),
            (2, 0, NotAliveDisposed),
            (3, 0, NotAliveNoWriters),
        ]
    );
    assert!(!s[2].info.valid_data);
    assert_eq!(s[1].info.view_state, ViewState::New);
    // Instance 1 is untouched and was read before.
    let s = reader.take();
    assert_eq!(summary(&reader, &s), [(1, 2, Alive), (1, 3, Alive)]);
    assert!(s.iter().all(
        |s| s.info.sample_state == SampleState::Read && s.info.view_state == ViewState::NotNew
    ));

    // Instance 2 comes back: NEW, one generation on.
    writer.write(&shape(2, 4)).unwrap();
    let s = reader.take_instance(h2);
    assert_eq!(summary(&reader, &s), [(2, 4, Alive)]);
    assert_eq!(s[0].info.view_state, ViewState::New);
    assert_eq!(s[0].info.disposed_generation_count, 1);

    // Deleting the writer unregisters 1 and 2, without disposing them.
    drop(writer);
    let s = reader.take();
    assert_eq!(
        summary(&reader, &s),
        [(1, 0, NotAliveNoWriters), (2, 0, NotAliveNoWriters)]
    );
}

#[test]
fn instances_over_udp() {
    use InstanceState::*;
    let a = participant(12);
    let b = participant(12);
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(
            &a.create_topic::<Shape>("shapes").unwrap(),
            writer_qos(true),
        )
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &b.create_topic::<Shape>("shapes").unwrap(),
            reliable_keep_all(),
        )
        .unwrap();
    assert!(writer.wait_for_subscriptions(1, WAIT));
    assert!(reader.wait_for_publications(1, WAIT));

    for i in 1..=6 {
        writer.write(&shape((i - 1) % 3 + 1, i)).unwrap();
    }
    writer.dispose(&shape(2, 0)).unwrap();
    // With autodispose, unregistering disposes too.
    writer.unregister_instance(&shape(3, 0)).unwrap();
    writer.wait_for_acknowledgments(WAIT).unwrap();
    let got = take_n(&reader, 8);
    assert_eq!(
        got,
        [
            (1, 1, Alive),
            (2, 2, NotAliveDisposed),
            (3, 3, NotAliveDisposed),
            (1, 4, Alive),
            (2, 5, NotAliveDisposed),
            (3, 6, NotAliveDisposed),
            (2, 0, NotAliveDisposed),
            (3, 0, NotAliveDisposed),
        ]
    );

    // The writer goes away: instance 1 is disposed (autodispose); its
    // samples are already taken, so only the change comes.
    drop(writer);
    let got = take_n(&reader, 1);
    assert_eq!(got, [(1, 0, NotAliveDisposed)]);
}

#[test]
fn deleting_a_writer_unregisters_its_instances() {
    let a = participant(13);
    let b = participant(13);
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(
            &a.create_topic::<Shape>("shapes").unwrap(),
            writer_qos(false),
        )
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &b.create_topic::<Shape>("shapes").unwrap(),
            reliable_keep_all(),
        )
        .unwrap();
    assert!(writer.wait_for_subscriptions(1, WAIT));
    assert!(reader.wait_for_publications(1, WAIT));
    writer.write(&shape(1, 1)).unwrap();
    writer.wait_for_acknowledgments(WAIT).unwrap();
    assert_eq!(take_n(&reader, 1), [(1, 1, InstanceState::Alive)]);
    // Deleting the writer unregisters instance 1 (without disposing it).
    drop(writer);
    let got = take_n(&reader, 1);
    assert_eq!(got, [(1, 0, InstanceState::NotAliveNoWriters)]);
}
