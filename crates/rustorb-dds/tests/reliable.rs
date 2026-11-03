//! Reliable writers and readers between two participants over UDP on
//! this host (rustorb-dds#16).

#[path = "../examples/hello_world/greeting.rs"]
mod greeting;

use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{
    DataReader, DataReaderQos, DataWriter, DataWriterQos, DomainParticipant, Error, History,
    ParticipantConfig, Reliability, ResourceLimits,
};

const WAIT: Duration = Duration::from_secs(10);

fn participant(domain: u32, drop_one_in: Option<u32>) -> DomainParticipant {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    c.writer.heartbeat_period = Duration::from_millis(50);
    c.drop_one_in = drop_one_in;
    DomainParticipant::with_config(c).unwrap()
}

fn greeting(index: u32) -> Greeting {
    Greeting {
        index,
        message: format!("hello {index}"),
    }
}

fn reliable_reader() -> DataReaderQos {
    DataReaderQos {
        reliability: Reliability::reliable(),
        history: History::KeepAll,
    }
}

fn keep_all_writer(max_samples: Option<usize>) -> DataWriterQos {
    DataWriterQos {
        reliability: Reliability {
            max_blocking_time: Duration::from_secs(5),
            ..Reliability::reliable()
        },
        history: History::KeepAll,
        resource_limits: ResourceLimits {
            max_samples,
            ..ResourceLimits::default()
        },
        ..DataWriterQos::default()
    }
}

fn wait_matched<W, R>(writer: &DataWriter<W>, reader: &DataReader<R>)
where
    W: rustorb_dds::TopicType,
    R: rustorb_dds::TopicType,
{
    assert!(
        writer.wait_for_subscriptions(1, WAIT),
        "writer saw no reader"
    );
    assert!(
        reader.wait_for_publications(1, WAIT),
        "reader saw no writer"
    );
}

/// Takes samples until `n` arrived or the wait runs out.
fn take_n(reader: &DataReader<Greeting>, n: usize) -> Vec<u32> {
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

#[test]
fn every_sample_arrives_in_order_despite_losses() {
    // The reader's side drops one in three user datagrams it receives
    // (DATA and HEARTBEAT), the writer's side one in five (ACKNACK).
    let a = participant(81, Some(5));
    let b = participant(81, Some(3));
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(
            &a.create_topic::<Greeting>("lossy").unwrap(),
            keep_all_writer(Some(8)),
        )
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &b.create_topic::<Greeting>("lossy").unwrap(),
            reliable_reader(),
        )
        .unwrap();
    wait_matched(&writer, &reader);

    // 100 samples through a history of 8: write blocks until the reader
    // acknowledges.
    for i in 1..=100 {
        writer.write(&greeting(i)).unwrap();
    }
    writer.wait_for_acknowledgments(WAIT).unwrap();
    let got = take_n(&reader, 100);
    assert_eq!(got, (1..=100).collect::<Vec<_>>());
}

#[test]
fn a_full_history_times_out() {
    let a = participant(82, None);
    let b = participant(82, None);
    let mut qos = keep_all_writer(Some(2));
    qos.reliability.max_blocking_time = Duration::from_millis(200);
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(&a.create_topic::<Greeting>("full").unwrap(), qos)
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &b.create_topic::<Greeting>("full").unwrap(),
            reliable_reader(),
        )
        .unwrap();
    wait_matched(&writer, &reader);
    // The reader's participant goes away without saying so: its last
    // acknowledgments never come, and the writer's history fills up.
    std::mem::forget(reader);
    drop(b);
    writer.write(&greeting(1)).unwrap();
    writer.write(&greeting(2)).unwrap();
    let start = Instant::now();
    let mut timed_out = false;
    for i in 3..=4 {
        if let Err(Error::Timeout) = writer.write(&greeting(i)) {
            timed_out = true;
            break;
        }
    }
    assert!(timed_out, "write did not block on a full history");
    assert!(start.elapsed() >= Duration::from_millis(200));
    assert!(matches!(
        writer.wait_for_acknowledgments(Duration::from_millis(100)),
        Err(Error::Timeout)
    ));
}

#[test]
fn a_late_reader_of_a_volatile_writer_gets_only_new_samples() {
    let a = participant(83, None);
    let b = participant(83, None);
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(
            &a.create_topic::<Greeting>("late").unwrap(),
            keep_all_writer(None),
        )
        .unwrap();
    // A first reader keeps the early samples unacknowledged for a while,
    // so they are still in the writer's history when the second comes.
    let first = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &b.create_topic::<Greeting>("late").unwrap(),
            reliable_reader(),
        )
        .unwrap();
    wait_matched(&writer, &first);
    for i in 1..=3 {
        writer.write(&greeting(i)).unwrap();
    }
    let c = participant(83, None);
    let late = c
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &c.create_topic::<Greeting>("late").unwrap(),
            reliable_reader(),
        )
        .unwrap();
    assert!(writer.wait_for_subscriptions(2, WAIT));
    assert!(late.wait_for_publications(1, WAIT));
    std::thread::sleep(Duration::from_millis(300));
    for i in 4..=6 {
        writer.write(&greeting(i)).unwrap();
    }
    writer.wait_for_acknowledgments(WAIT).unwrap();
    assert_eq!(take_n(&first, 6), [1, 2, 3, 4, 5, 6]);
    assert_eq!(take_n(&late, 3), [4, 5, 6]);
    // Nothing more comes to the late reader.
    assert!(!late.wait_for_data(Duration::from_millis(300)));
}

#[test]
fn reliability_matching() {
    let a = participant(84, None);
    let b = participant(84, None);
    let ta = a.create_topic::<Greeting>("rel").unwrap();
    let tb = b.create_topic::<Greeting>("rel").unwrap();
    let best_effort_writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(
            &ta,
            DataWriterQos {
                reliability: Reliability::best_effort(),
                ..DataWriterQos::default()
            },
        )
        .unwrap();
    // The default writer is reliable.
    let reliable_writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter(&ta)
        .unwrap();
    let reliable = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&tb, reliable_reader())
        .unwrap();
    let best_effort = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &tb,
            DataReaderQos {
                history: History::KeepAll,
                ..DataReaderQos::default()
            },
        )
        .unwrap();
    // A reliable reader matches only the reliable writer; a best-effort
    // reader matches both.
    assert!(best_effort.wait_for_publications(2, WAIT));
    assert!(reliable_writer.wait_for_subscriptions(2, WAIT));
    assert_eq!(reliable.matched_publications(), [reliable_writer.guid()]);
    assert_eq!(
        best_effort_writer.matched_subscriptions(),
        [best_effort.guid()]
    );
    std::thread::sleep(Duration::from_millis(300));

    // The reliable writer serves both of its readers; acknowledgments
    // come only from the reliable one.
    for i in 1..=3 {
        reliable_writer.write(&greeting(i)).unwrap();
    }
    reliable_writer.wait_for_acknowledgments(WAIT).unwrap();
    assert_eq!(take_n(&reliable, 3), [1, 2, 3]);
    assert_eq!(take_n(&best_effort, 3), [1, 2, 3]);
    // A best-effort writer has nothing to wait for.
    best_effort_writer.write(&greeting(9)).unwrap();
    best_effort_writer
        .wait_for_acknowledgments(Duration::ZERO)
        .unwrap();
    assert_eq!(take_n(&best_effort, 1), [9]);
}
