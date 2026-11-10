//! The shared memory transport between participants of one host, and
//! UDP with a participant that does not use it.

#![cfg(feature = "shm")]

#[path = "../examples/keyed/shape.rs"]
mod shape;

use std::time::{Duration, Instant};

use rustorb_dds::{
    DataReaderQos, DataWriterQos, DomainParticipant, History, ParticipantConfig, Reliability,
};
use shape::Shape;

const WAIT: Duration = Duration::from_secs(10);

fn participant(domain: u32, shared_memory: bool) -> DomainParticipant {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    c.shared_memory = shared_memory;
    DomainParticipant::with_config(c).unwrap()
}

/// Sends `n` reliable samples of about `size` octets from `a` to `b`.
fn exchange(a: &DomainParticipant, b: &DomainParticipant, n: u32, size: usize) {
    let ta = a.create_topic::<Shape>("Big").unwrap();
    let tb = b.create_topic::<Shape>("Big").unwrap();
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(
            &ta,
            DataWriterQos {
                history: History::KeepAll,
                ..DataWriterQos::default()
            },
        )
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &tb,
            DataReaderQos {
                reliability: Reliability::reliable(),
                history: History::KeepAll,
                ..DataReaderQos::default()
            },
        )
        .unwrap();
    assert!(writer.wait_for_subscriptions(1, WAIT));
    for i in 0..n {
        writer
            .write(&Shape {
                id: i,
                index: i,
                message: "x".repeat(size),
            })
            .unwrap();
    }
    let deadline = Instant::now() + WAIT;
    let mut got = Vec::new();
    while got.len() < n as usize && Instant::now() < deadline {
        got.extend(reader.take().into_iter().filter_map(|s| s.data));
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(got.len(), n as usize);
    assert!(got.iter().all(|s| s.message.len() == size));
}

#[test]
fn large_samples_through_shared_memory() {
    let (a, b) = (participant(191, true), participant(191, true));
    exchange(&a, &b, 30, 200_000);
}

#[test]
fn udp_with_a_participant_without_shared_memory() {
    let (a, b) = (participant(192, true), participant(192, false));
    exchange(&a, &b, 10, 100_000);
    exchange(&b, &a, 10, 1000);
}
