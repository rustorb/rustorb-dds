//! The listeners of participants, publishers, subscribers and topics, and
//! how a status change finds its listener (DDS 1.4, 2.2.4.3).

#[path = "../examples/hello_world/greeting.rs"]
mod greeting;
#[path = "../examples/keyed/shape.rs"]
mod shape;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{
    AnyDataReader, AnyDataWriter, DataReader, DataReaderListener, DataWriterListener,
    DomainParticipant, DomainParticipantListener, InconsistentTopicStatus, MatchedStatus,
    ParticipantConfig, PublisherListener, StatusMask, Subscriber, SubscriberListener,
    TopicListener,
};
use shape::Shape;

const WAIT: Duration = Duration::from_secs(10);

fn participant(domain: u32) -> DomainParticipant {
    let mut c = ParticipantConfig::localhost(domain);
    c.announce_period = Duration::from_millis(200);
    DomainParticipant::with_config(c).unwrap()
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + WAIT;
    while !f() {
        assert!(Instant::now() < end, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// What the listeners were called for, in order.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    fn push(&self, s: impl Into<String>) {
        self.0.lock().unwrap().push(s.into());
    }
    fn has(&self, s: &str) -> bool {
        self.0.lock().unwrap().iter().any(|e| e == s)
    }
    fn count(&self, s: &str) -> usize {
        self.0.lock().unwrap().iter().filter(|e| *e == s).count()
    }
}

/// A listener of any kind that logs as `name`.
struct Logger {
    name: &'static str,
    log: Log,
}

impl PublisherListener for Logger {
    fn on_publication_matched(&self, writer: &AnyDataWriter, status: MatchedStatus) {
        assert_eq!(writer.topic_name(), "Listened");
        assert_eq!(writer.type_name(), "HelloWorld::Greeting");
        self.log.push(format!(
            "{}:publication_matched {}",
            self.name, status.current_count
        ));
    }
}

impl SubscriberListener for Logger {
    fn on_data_on_readers(&self, _subscriber: &Subscriber) {
        self.log.push(format!("{}:data_on_readers", self.name));
    }
    fn on_data_available(&self, reader: &AnyDataReader) {
        assert_eq!(reader.topic_name(), "Listened");
        self.log.push(format!("{}:data_available", self.name));
    }
    fn on_subscription_matched(&self, _reader: &AnyDataReader, status: MatchedStatus) {
        self.log.push(format!(
            "{}:subscription_matched {}",
            self.name, status.current_count
        ));
    }
}

impl TopicListener for Logger {
    fn on_inconsistent_topic(&self, topic_name: &str, status: InconsistentTopicStatus) {
        assert_eq!(topic_name, "xt");
        assert_eq!(status.total_count_change, 1);
        self.log.push(format!("{}:inconsistent_topic", self.name));
    }
}

impl DomainParticipantListener for Logger {}

impl<T> DataWriterListener<T> for Logger {
    fn on_publication_matched(&self, _writer: &rustorb_dds::DataWriter<T>, status: MatchedStatus) {
        self.log.push(format!(
            "{}:publication_matched {}",
            self.name, status.current_count
        ));
    }
}

impl<T> DataReaderListener<T> for Logger {
    fn on_data_available(&self, _reader: &DataReader<T>) {
        self.log.push(format!("{}:data_available", self.name));
    }
}

fn logger(name: &'static str, log: &Log) -> Logger {
    Logger {
        name,
        log: log.clone(),
    }
}

/// A writer's status goes to the publisher's listener when the writer has
/// none, and to the participant's when neither takes it.
#[test]
fn writer_statuses_go_up_to_publisher_and_participant() {
    let p = participant(14);
    let log = Log::default();
    p.set_listener(logger("participant", &log), StatusMask::ALL);
    let topic = p.create_topic::<Greeting>("Listened").unwrap();
    let publisher = p.create_publisher().unwrap();
    publisher.set_listener(logger("publisher", &log), StatusMask::PUBLICATION_MATCHED);
    let _w1 = publisher.create_datawriter(&topic).unwrap();
    // A writer with its own listener keeps the status to itself.
    let _w2 = publisher
        .create_datawriter_with_listener(
            &topic,
            Default::default(),
            logger("writer", &log),
            StatusMask::PUBLICATION_MATCHED,
        )
        .unwrap();
    // A publisher whose listener does not take it hands it on.
    let other = p.create_publisher().unwrap();
    other.set_listener(logger("other", &log), StatusMask::NONE);
    let _w3 = other.create_datawriter(&topic).unwrap();
    let _reader = p
        .create_subscriber()
        .unwrap()
        .create_datareader(&topic)
        .unwrap();
    wait_until("the three writers matched", || {
        log.has("publisher:publication_matched 1")
            && log.has("writer:publication_matched 1")
            && log.has("participant:publication_matched 1")
    });
    assert_eq!(log.count("publisher:publication_matched 1"), 1);
    assert_eq!(log.count("participant:publication_matched 1"), 1);
    assert!(!log.has("other:publication_matched 1"));
}

/// New data goes to `on_data_on_readers` of the subscriber's listener and
/// not to the readers' `on_data_available`, until `notify_datareaders`.
#[test]
fn data_on_readers_comes_before_data_available() {
    let p = participant(15);
    let log = Log::default();
    let topic = p.create_topic::<Greeting>("Listened").unwrap();
    let subscriber = p.create_subscriber().unwrap();
    let reader = subscriber
        .create_datareader_with_listener(
            &topic,
            Default::default(),
            logger("reader", &log),
            StatusMask::DATA_AVAILABLE,
        )
        .unwrap();
    struct OnReaders(Log);
    impl SubscriberListener for OnReaders {
        fn on_data_on_readers(&self, subscriber: &Subscriber) {
            self.0.push("subscriber:data_on_readers");
            subscriber.notify_datareaders();
        }
    }
    let writer = p
        .create_publisher()
        .unwrap()
        .create_datawriter(&topic)
        .unwrap();
    // Without a subscriber listener, the reader's own.
    writer
        .write(&Greeting {
            index: 1,
            message: "one".into(),
        })
        .unwrap();
    wait_until("reader:data_available", || log.has("reader:data_available"));
    assert_eq!(reader.take().len(), 1);

    // With one that takes DATA_ON_READERS, it first; then the reader's
    // through notify_datareaders.
    subscriber.set_listener(OnReaders(log.clone()), StatusMask::DATA_ON_READERS);
    writer
        .write(&Greeting {
            index: 2,
            message: "two".into(),
        })
        .unwrap();
    wait_until("both", || {
        log.has("subscriber:data_on_readers") && log.count("reader:data_available") == 2
    });
    let order: Vec<String> = log.0.lock().unwrap().clone();
    assert_eq!(
        order[order.len() - 2..],
        ["subscriber:data_on_readers", "reader:data_available"]
    );
}

/// Without notify_datareaders, the reader keeps DATA_AVAILABLE for its
/// StatusCondition; a participant listener takes DATA_ON_READERS when
/// the subscriber has none.
#[test]
fn participant_takes_data_on_readers() {
    let p = participant(16);
    let log = Log::default();
    p.set_listener(logger("participant", &log), StatusMask::DATA_ON_READERS);
    let topic = p.create_topic::<Greeting>("Listened").unwrap();
    let reader = p
        .create_subscriber()
        .unwrap()
        .create_datareader_with_listener(
            &topic,
            Default::default(),
            logger("reader", &log),
            StatusMask::DATA_AVAILABLE,
        )
        .unwrap();
    let writer = p
        .create_publisher()
        .unwrap()
        .create_datawriter(&topic)
        .unwrap();
    writer
        .write(&Greeting {
            index: 1,
            message: "one".into(),
        })
        .unwrap();
    wait_until("participant:data_on_readers", || {
        log.has("participant:data_on_readers")
    });
    assert!(!log.has("reader:data_available"));
    assert!(reader.status_changes().contains(StatusMask::DATA_AVAILABLE));
    assert_eq!(reader.take().len(), 1);
    assert!(!reader.status_changes().contains(StatusMask::DATA_AVAILABLE));

    // Without the participant's listener, a subscriber listener that
    // takes DATA_AVAILABLE but not DATA_ON_READERS gets it for its
    // readers that have no listener of their own.
    p.clear_listener();
    let sub = p.create_subscriber().unwrap();
    sub.set_listener(logger("subscriber", &log), StatusMask::DATA_AVAILABLE);
    let _r2 = sub.create_datareader(&topic).unwrap();
    writer
        .write(&Greeting {
            index: 2,
            message: "two".into(),
        })
        .unwrap();
    wait_until("subscriber:data_available", || {
        log.has("subscriber:data_available") && log.has("reader:data_available")
    });
}

/// INCONSISTENT_TOPIC goes to the topic's listener, else the
/// participant's.
#[test]
fn inconsistent_topic_listeners() {
    let (a, b) = (participant(17), participant(17));
    let log = Log::default();
    a.set_listener(logger("participant_a", &log), StatusMask::ALL);
    let topic_a = a.create_topic::<Greeting>("xt").unwrap();
    let topic_b = b.create_topic::<Shape>("xt").unwrap();
    topic_b.set_listener(logger("topic_b", &log), StatusMask::INCONSISTENT_TOPIC);
    let _writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter(&topic_a)
        .unwrap();
    let _reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader(&topic_b)
        .unwrap();
    wait_until("both", || {
        log.has("topic_b:inconsistent_topic") && log.has("participant_a:inconsistent_topic")
    });
    // Given to the listener: read.
    assert_eq!(topic_b.inconsistent_topic_status().total_count_change, 0);
    assert_eq!(topic_b.inconsistent_topic_status().total_count, 1);
}
