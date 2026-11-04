//! WaitSets, conditions, statuses and listeners (rustorb-dds#20).

#[path = "../examples/hello_world/greeting.rs"]
mod greeting;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{
    Condition, DataReader, DataReaderListener, DataReaderQos, DataWriter, DataWriterListener,
    DataWriterQos, DeadlineMissedStatus, DomainParticipant, Error, GuardCondition, History,
    MatchedStatus, ParticipantConfig, Reliability, SampleStateMask, StateFilter, StatusMask,
    WaitSet,
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

fn keep_all() -> DataReaderQos {
    DataReaderQos {
        history: History::KeepAll,
        ..DataReaderQos::default()
    }
}

fn local_pair(p: &DomainParticipant, topic: &str) -> (DataWriter<Greeting>, DataReader<Greeting>) {
    let topic = p.create_topic::<Greeting>(topic).unwrap();
    let reader = p
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&topic, keep_all())
        .unwrap();
    let writer = p
        .create_publisher()
        .unwrap()
        .create_datawriter(&topic)
        .unwrap();
    (writer, reader)
}

#[test]
fn guard_condition_wakes_a_waiting_thread() {
    let waitset = Arc::new(WaitSet::new());
    let guard = GuardCondition::new();
    waitset.attach_condition(&guard).unwrap();
    // Attaching twice does nothing.
    waitset.attach_condition(&guard).unwrap();
    assert_eq!(waitset.conditions(), [Condition::from(&guard)]);

    assert!(matches!(
        waitset.wait(Duration::from_millis(50)),
        Err(Error::Timeout)
    ));

    let w = waitset.clone();
    let waiter = std::thread::spawn(move || w.wait(WAIT));
    std::thread::sleep(Duration::from_millis(100));
    // One waiter at a time.
    assert!(matches!(
        waitset.wait(Duration::ZERO),
        Err(Error::PreconditionNotMet)
    ));
    let start = Instant::now();
    guard.set_trigger_value(true);
    let active = waiter.join().unwrap().unwrap();
    assert!(start.elapsed() < Duration::from_secs(1));
    assert_eq!(active, [Condition::from(&guard)]);
    assert!(active[0] == guard);

    // Still true: it returns at once.
    assert_eq!(waitset.wait(Duration::ZERO).unwrap().len(), 1);
    guard.set_trigger_value(false);
    waitset.detach_condition(&guard).unwrap();
    assert!(matches!(
        waitset.detach_condition(&guard),
        Err(Error::PreconditionNotMet)
    ));
    assert!(waitset.conditions().is_empty());
}

#[test]
fn status_condition_follows_data_available() {
    let p = participant(141, None);
    let (writer, reader) = local_pair(&p, "status");
    let status = reader.status_condition();
    assert!(status == reader.status_condition());
    assert_eq!(status.enabled_statuses(), StatusMask::ALL);
    status.set_enabled_statuses(StatusMask::DATA_AVAILABLE);
    assert_eq!(status.enabled_statuses(), StatusMask::DATA_AVAILABLE);
    // The match changed SUBSCRIPTION_MATCHED, which is not enabled.
    assert!(
        reader
            .status_changes()
            .contains(StatusMask::SUBSCRIPTION_MATCHED)
    );
    assert!(!status.trigger_value());

    let waitset = Arc::new(WaitSet::new());
    waitset.attach_condition(&status).unwrap();
    let w = waitset.clone();
    let waiter = std::thread::spawn(move || w.wait(WAIT));
    std::thread::sleep(Duration::from_millis(100));
    writer.write(&greeting(1)).unwrap();
    let active = waiter.join().unwrap().unwrap();
    assert_eq!(active, [Condition::from(&status)]);
    assert!(reader.status_changes().contains(StatusMask::DATA_AVAILABLE));

    // Taking the samples clears DATA_AVAILABLE.
    assert_eq!(reader.take().len(), 1);
    assert!(!status.trigger_value());
    assert!(matches!(
        waitset.wait(Duration::from_millis(50)),
        Err(Error::Timeout)
    ));

    // Reading SUBSCRIPTION_MATCHED clears it too.
    let m = reader.subscription_matched_status();
    assert_eq!(m.current_count, 1);
    assert_eq!(m.total_count, 1);
    assert_eq!(m.last_handle, Some(writer.guid()));
    // The writer became alive at the match.
    assert_eq!(reader.status_changes(), StatusMask::LIVELINESS_CHANGED);
    assert_eq!(reader.liveliness_changed_status().alive_count, 1);
    assert!(reader.status_changes().is_empty());
}

#[test]
fn read_condition_follows_the_sample_states() {
    let p = participant(142, None);
    let (writer, reader) = local_pair(&p, "read");
    let other = local_pair(&p, "read-other").1;
    let not_read = reader.create_readcondition(StateFilter::NOT_READ);
    let any = reader.create_readcondition(StateFilter::ANY);
    assert!(not_read != any);
    assert!(!not_read.trigger_value());

    let waitset = WaitSet::new();
    waitset.attach_condition(&not_read).unwrap();
    waitset.attach_condition(&any).unwrap();
    writer.write(&greeting(1)).unwrap();
    writer.write(&greeting(2)).unwrap();
    assert_eq!(waitset.wait(WAIT).unwrap().len(), 2);

    let read = reader.read_w_condition(&not_read).unwrap();
    assert_eq!(read.len(), 2);
    assert!(!not_read.trigger_value());
    assert!(any.trigger_value());
    assert_eq!(waitset.wait(WAIT).unwrap(), [Condition::from(&any)]);
    assert!(reader.read_w_condition(&not_read).unwrap().is_empty());

    let read_only = reader.create_readcondition(StateFilter {
        sample_states: SampleStateMask::READ,
        ..StateFilter::ANY
    });
    assert_eq!(reader.take_w_condition(&read_only).unwrap().len(), 2);
    assert!(!any.trigger_value());
    // A condition of another reader.
    assert!(matches!(
        other.read_w_condition(&any),
        Err(Error::PreconditionNotMet)
    ));
}

#[derive(Default)]
struct Matches {
    writer: Mutex<Vec<MatchedStatus>>,
    reader: Mutex<Vec<MatchedStatus>>,
    data: AtomicU32,
}

struct WriterSide(Arc<Matches>);

impl DataWriterListener<Greeting> for WriterSide {
    fn on_publication_matched(&self, writer: &DataWriter<Greeting>, status: MatchedStatus) {
        // The status is read already, and the handle works.
        assert!(writer.publication_matched_status().total_count_change == 0);
        self.0.writer.lock().unwrap().push(status);
    }
}

struct ReaderSide(Arc<Matches>);

impl DataReaderListener<Greeting> for ReaderSide {
    fn on_data_available(&self, reader: &DataReader<Greeting>) {
        let n = reader.take().len() as u32;
        self.0.data.fetch_add(n, Ordering::SeqCst);
    }

    fn on_subscription_matched(&self, _: &DataReader<Greeting>, status: MatchedStatus) {
        self.0.reader.lock().unwrap().push(status);
    }
}

fn wait_until(what: &str, mut f: impl FnMut() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !f() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn listeners_see_matches_across_participants() {
    let a = participant(143, None);
    let b = participant(143, None);
    let calls = Arc::new(Matches::default());
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(
            &a.create_topic::<Greeting>("matched").unwrap(),
            DataWriterQos {
                reliability: Reliability::reliable(),
                ..DataWriterQos::default()
            },
        )
        .unwrap();
    writer.set_listener(WriterSide(calls.clone()), StatusMask::ALL);
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(
            &b.create_topic::<Greeting>("matched").unwrap(),
            DataReaderQos {
                reliability: Reliability::reliable(),
                history: History::KeepAll,
                ..DataReaderQos::default()
            },
        )
        .unwrap();
    // Set after the reader is made: the match may come before.
    reader.set_listener(
        ReaderSide(calls.clone()),
        StatusMask::DATA_AVAILABLE | StatusMask::SUBSCRIPTION_MATCHED,
    );

    wait_until("the writer's match", || {
        !calls.writer.lock().unwrap().is_empty()
    });
    let w = calls.writer.lock().unwrap()[0];
    assert_eq!(
        (w.total_count, w.current_count, w.current_count_change),
        (1, 1, 1)
    );
    assert_eq!(w.last_handle, Some(reader.guid()));
    // The listener took the status: its flag is not set.
    assert!(
        !writer
            .status_changes()
            .contains(StatusMask::PUBLICATION_MATCHED)
    );

    wait_until("the reader's match", || {
        reader.subscription_matched_status().current_count == 1
    });
    for i in 1..=5 {
        writer.write(&greeting(i)).unwrap();
    }
    wait_until("the samples", || calls.data.load(Ordering::SeqCst) == 5);

    drop(reader);
    wait_until("the writer's unmatch", || {
        calls.writer.lock().unwrap().len() == 2
    });
    let w = calls.writer.lock().unwrap()[1];
    assert_eq!(
        (w.total_count, w.current_count, w.current_count_change),
        (1, 0, -1)
    );
}

#[test]
fn sample_lost_counts_what_never_arrived() {
    let a = participant(144, None);
    // The reader's participant drops one user datagram in four.
    let b = participant(144, Some(4));
    let writer = a
        .create_publisher()
        .unwrap()
        .create_datawriter(&a.create_topic::<Greeting>("lost").unwrap())
        .unwrap();
    let reader = b
        .create_subscriber()
        .unwrap()
        .create_datareader_with_qos(&b.create_topic::<Greeting>("lost").unwrap(), keep_all())
        .unwrap();
    assert!(writer.wait_for_subscriptions(1, WAIT));
    assert!(reader.wait_for_publications(1, WAIT));
    std::thread::sleep(Duration::from_millis(200));
    for i in 1..=200 {
        writer.write(&greeting(i)).unwrap();
        std::thread::sleep(Duration::from_millis(1));
    }
    std::thread::sleep(Duration::from_millis(300));
    let got: Vec<u32> = reader
        .take()
        .into_iter()
        .filter_map(|s| s.data.map(|g| g.index))
        .collect();
    assert!(got.len() > 1, "too few samples: {got:?}");
    let span = got[got.len() - 1] - got[0] + 1;
    assert!(reader.status_changes().contains(StatusMask::SAMPLE_LOST));
    let lost = reader.sample_lost_status();
    assert!(lost.total_count > 0);
    assert_eq!(lost.total_count, lost.total_count_change);
    // Every index between the first and the last received is either
    // received or lost.
    assert_eq!(got.len() as u32 + lost.total_count, span);
    assert_eq!(reader.sample_lost_status().total_count_change, 0);
    assert!(!reader.status_changes().contains(StatusMask::SAMPLE_LOST));
}

struct Deadlines(Arc<AtomicU32>);

impl DataWriterListener<Greeting> for Deadlines {
    fn on_offered_deadline_missed(&self, _: &DataWriter<Greeting>, s: DeadlineMissedStatus) {
        self.0.fetch_add(s.total_count_change, Ordering::SeqCst);
    }
}

#[test]
fn the_listener_mask_chooses_the_statuses() {
    let p = participant(145, None);
    let topic = p.create_topic::<Greeting>("mask").unwrap();
    let missed = Arc::new(AtomicU32::new(0));
    let writer = p
        .create_publisher()
        .unwrap()
        .create_datawriter_with_qos(
            &topic,
            DataWriterQos {
                deadline: Some(Duration::from_millis(50)),
                ..DataWriterQos::default()
            },
        )
        .unwrap();
    // Only OFFERED_DEADLINE_MISSED goes to the listener.
    writer.set_listener(
        Deadlines(missed.clone()),
        StatusMask::OFFERED_DEADLINE_MISSED,
    );
    let status = writer.status_condition();
    status.set_enabled_statuses(StatusMask::PUBLICATION_MATCHED);
    let waitset = WaitSet::new();
    waitset.attach_condition(&status).unwrap();
    assert!(matches!(
        waitset.wait(Duration::from_millis(50)),
        Err(Error::Timeout)
    ));

    let _reader = p
        .create_subscriber()
        .unwrap()
        .create_datareader(&topic)
        .unwrap();
    assert_eq!(waitset.wait(WAIT).unwrap(), [Condition::from(&status)]);
    assert_eq!(writer.publication_matched_status().current_count, 1);
    assert!(!status.trigger_value());

    writer.write(&greeting(1)).unwrap();
    wait_until("a deadline missed", || missed.load(Ordering::SeqCst) > 0);
    // The listener took it.
    assert!(
        !writer
            .status_changes()
            .contains(StatusMask::OFFERED_DEADLINE_MISSED)
    );
    assert_eq!(
        writer.offered_deadline_missed_status().total_count_change,
        0
    );

    // Without the listener, the changes are flagged.
    writer.clear_listener();
    wait_until("a flagged deadline", || {
        writer
            .status_changes()
            .contains(StatusMask::OFFERED_DEADLINE_MISSED)
    });
    assert!(writer.offered_deadline_missed_status().total_count_change > 0);
}

#[test]
fn listeners_given_at_creation_see_local_matches() {
    // The writer and reader match while they are created: only a
    // listener given then is told.
    let p = participant(160, None);
    let topic = p.create_topic::<Greeting>("created").unwrap();
    let calls = Arc::new(Matches::default());
    let reliable = DataReaderQos {
        reliability: Reliability::reliable(),
        ..DataReaderQos::default()
    };
    let _reader = p
        .create_subscriber()
        .unwrap()
        .create_datareader_with_listener(
            &topic,
            reliable,
            ReaderSide(calls.clone()),
            StatusMask::ALL,
        )
        .unwrap();
    let _writer = p
        .create_publisher()
        .unwrap()
        .create_datawriter_with_listener(
            &topic,
            DataWriterQos::default(),
            WriterSide(calls.clone()),
            StatusMask::ALL,
        )
        .unwrap();
    let w = calls.writer.lock().unwrap();
    assert_eq!(w.len(), 1);
    assert_eq!(w[0].current_count, 1);
    let r = calls.reader.lock().unwrap();
    assert_eq!(r.len(), 1);
    assert_eq!(r[0].current_count, 1);
}
