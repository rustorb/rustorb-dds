//! Samples too large for a datagram, sent as DATA_FRAG between two
//! participants over UDP on this host (rustorb-dds#18).

#[path = "../examples/hello_world/greeting.rs"]
mod greeting;

use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{
    DataReader, DataReaderQos, DataWriter, DataWriterQos, DomainParticipant, History,
    ParticipantConfig, Reliability,
};

const WAIT: Duration = Duration::from_secs(20);

fn participant(domain: u32, drop_one_in: Option<u32>) -> DomainParticipant {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    c.writer.heartbeat_period = Duration::from_millis(50);
    c.drop_one_in = drop_one_in;
    DomainParticipant::with_config(c).unwrap()
}

/// A greeting whose message is `size` octets, different for each index.
fn greeting(index: u32, size: usize) -> Greeting {
    Greeting {
        index,
        message: (0..size)
            .map(|i| char::from(b'a' + ((i as u32 + index) % 26) as u8))
            .collect(),
    }
}

fn pair(
    domain: u32,
    drops: (Option<u32>, Option<u32>),
    writer_qos: DataWriterQos,
    reader_qos: DataReaderQos,
) -> (
    DomainParticipant,
    DomainParticipant,
    DataWriter<Greeting>,
    DataReader<Greeting>,
) {
    let a = participant(domain, drops.0);
    let b = participant(domain, drops.1);
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(&a.create_topic::<Greeting>("large").unwrap(), writer_qos)
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&b.create_topic::<Greeting>("large").unwrap(), reader_qos)
        .unwrap();
    assert!(writer.wait_for_subscriptions(1, WAIT));
    assert!(reader.wait_for_publications(1, WAIT));
    (a, b, writer, reader)
}

/// Takes samples until `n` arrived or the wait runs out.
fn take_n(reader: &DataReader<Greeting>, n: usize) -> Vec<Greeting> {
    let deadline = Instant::now() + WAIT;
    let mut got = Vec::new();
    while got.len() < n && Instant::now() < deadline {
        if reader.wait_for_data(Duration::from_millis(50)) {
            got.extend(reader.take().into_iter().filter_map(|s| s.data));
        }
    }
    got
}

fn reliable_keep_all() -> (DataWriterQos, DataReaderQos) {
    (
        DataWriterQos {
            reliability: Reliability {
                max_blocking_time: Duration::from_secs(10),
                ..Reliability::reliable()
            },
            history: History::KeepAll,
            ..DataWriterQos::default()
        },
        DataReaderQos {
            reliability: Reliability::reliable(),
            history: History::KeepAll,
            ..DataReaderQos::default()
        },
    )
}

#[test]
fn reliable_large_samples_despite_losses() {
    // One in five datagrams is lost on both sides: lost fragments are
    // asked for again with NACK_FRAG.
    let (w, r) = reliable_keep_all();
    let (_a, _b, writer, reader) = pair(91, (Some(5), Some(5)), w, r);
    // Around the limit of a datagram, and well above it.
    let sizes = [60_000, 70_000, 300_000, 1_000_000, 10];
    for (i, size) in sizes.iter().enumerate() {
        writer.write(&greeting(i as u32 + 1, *size)).unwrap();
    }
    writer.wait_for_acknowledgments(WAIT).unwrap();
    let got = take_n(&reader, sizes.len());
    let want: Vec<Greeting> = sizes
        .iter()
        .enumerate()
        .map(|(i, size)| greeting(i as u32 + 1, *size))
        .collect();
    assert_eq!(got.len(), want.len());
    assert!(got == want, "samples differ");
}

#[test]
fn best_effort_large_samples() {
    let (_a, _b, writer, reader) = pair(
        92,
        (None, None),
        DataWriterQos {
            reliability: Reliability::best_effort(),
            ..DataWriterQos::default()
        },
        DataReaderQos::default(),
    );
    // Best effort may lose some: write until 3 arrive.
    let deadline = Instant::now() + WAIT;
    let mut got = Vec::new();
    let mut i = 0;
    while got.len() < 3 && Instant::now() < deadline {
        i += 1;
        writer.write(&greeting(i, 200_000)).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        got.extend(reader.take().into_iter().filter_map(|s| s.data));
    }
    assert!(got.len() >= 3, "got {} of {i}", got.len());
    for g in got {
        assert_eq!(g, greeting(g.index, 200_000));
    }
}
