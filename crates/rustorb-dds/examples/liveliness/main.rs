//! LIVELINESS across participants, for `interop/run.sh`.
//!
//! ```text
//! liveliness pub <domain> <automatic|participant|topic> <lease_ms>
//! liveliness sub <domain> <automatic|participant|topic> <lease_ms> <timeout_s>
//! ```
//!
//! Both sides use the topic `liveliness` of type `HelloWorld::Greeting`,
//! RELIABLE, KEEP_ALL, with the LIVELINESS kind given (AUTOMATIC,
//! MANUAL_BY_PARTICIPANT or MANUAL_BY_TOPIC) and the lease given.
//!
//! The publisher waits for a subscriber, writes sample 1, then for three
//! leases asserts its liveliness every quarter lease (a manual writer:
//! with `assert_liveliness` on the participant or on the writer; an
//! automatic one does nothing), writes sample 2, keeps silent for four
//! leases, writes sample 3 and waits for the acknowledgments.
//!
//! The subscriber checks the LIVELINESS_CHANGED status of its reader:
//! the writer must stay alive from sample 1 to sample 2; from sample 2
//! to sample 3 a manual writer must become not alive and an automatic
//! one must stay alive; after sample 3 the writer must be alive.

#[path = "../hello_world/greeting.rs"]
mod greeting;

use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{
    DataReaderQos, DataWriterQos, DomainParticipant, History, Liveliness, LivelinessKind,
    ParticipantConfig, Reliability,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        return Err("usage: liveliness pub|sub <domain> <kind> <lease_ms> [timeout_s]".into());
    }
    let role = args[1].as_str();
    let domain: u32 = args[2].parse()?;
    let kind = match args[3].as_str() {
        "automatic" => LivelinessKind::Automatic,
        "participant" => LivelinessKind::ManualByParticipant,
        "topic" => LivelinessKind::ManualByTopic,
        other => return Err(format!("unknown liveliness kind {other:?}").into()),
    };
    let lease = Duration::from_millis(args[4].parse()?);
    let liveliness = Liveliness {
        kind,
        lease_duration: Some(lease),
    };
    let participant = DomainParticipant::with_config(ParticipantConfig::new(domain))?;
    let topic = participant.create_topic::<Greeting>("liveliness")?;
    let greeting = |index: u32| Greeting {
        index,
        message: "Hello, world!".into(),
    };
    match role {
        "pub" => {
            let writer = participant.create_publisher()?.create_datawriter_with_qos(
                &topic,
                DataWriterQos {
                    reliability: Reliability::reliable(),
                    history: History::KeepAll,
                    liveliness,
                    ..DataWriterQos::default()
                },
            )?;
            println!("rustorb-dds: waiting for a subscriber...");
            while !writer.wait_for_subscriptions(1, Duration::from_secs(1)) {}
            std::thread::sleep(Duration::from_millis(500));
            writer.write(&greeting(1))?;
            println!("rustorb-dds: sent 1; asserting for three leases");
            let until = Instant::now() + lease * 3;
            while Instant::now() < until {
                match kind {
                    LivelinessKind::ManualByParticipant => participant.assert_liveliness()?,
                    LivelinessKind::ManualByTopic => writer.assert_liveliness()?,
                    LivelinessKind::Automatic => {}
                }
                std::thread::sleep(lease / 4);
            }
            writer.write(&greeting(2))?;
            println!("rustorb-dds: sent 2; silent for four leases");
            std::thread::sleep(lease * 4);
            writer.write(&greeting(3))?;
            println!("rustorb-dds: sent 3");
            writer.wait_for_acknowledgments(Duration::from_secs(30))?;
            // Let the reader see the writer alive before it goes.
            std::thread::sleep(Duration::from_secs(1));
            let lost = writer.liveliness_lost_status().total_count;
            println!("rustorb-dds: liveliness lost {lost} times");
            let want = u32::from(kind != LivelinessKind::Automatic);
            if lost != want {
                return Err(format!("liveliness lost {lost} times, expected {want}").into());
            }
        }
        "sub" => {
            let timeout = Duration::from_secs(args.get(5).map_or(Ok(60), |s| s.parse())?);
            let deadline = Instant::now() + timeout;
            let reader = participant
                .create_subscriber()?
                .create_datareader_with_qos(
                    &topic,
                    DataReaderQos {
                        reliability: Reliability::reliable(),
                        history: History::KeepAll,
                        liveliness,
                        ..DataReaderQos::default()
                    },
                )?;
            println!("rustorb-dds: waiting for greetings...");
            // The last sample received, and whether the writer was seen
            // not alive since.
            let mut last = 0;
            let mut not_alive_seen = false;
            loop {
                if Instant::now() >= deadline {
                    return Err(format!("timed out after sample {last}").into());
                }
                let s = reader.liveliness_changed_status();
                // Before sample 1 the writer may not have asserted its
                // liveliness yet: that is not checked.
                if last >= 1 && s.not_alive_count > 0 && !not_alive_seen {
                    not_alive_seen = true;
                    println!("rustorb-dds: writer not alive after sample {last}");
                    if last != 2 || kind == LivelinessKind::Automatic {
                        return Err(format!("writer not alive after sample {last}").into());
                    }
                }
                if last == 3 {
                    if s.alive_count == 1 && s.not_alive_count == 0 {
                        println!("rustorb-dds: writer alive again");
                        break;
                    }
                } else if reader.wait_for_data(Duration::from_millis(20)) {
                    for g in reader.take().into_iter().filter_map(|s| s.data) {
                        if g.index != last + 1 {
                            return Err(format!("expected {}, got {}", last + 1, g.index).into());
                        }
                        last = g.index;
                        println!("rustorb-dds: received {last}");
                        if last == 3 && kind != LivelinessKind::Automatic && !not_alive_seen {
                            return Err("the writer was never seen not alive".into());
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        }
        _ => return Err(format!("unknown role {role:?}: use pub or sub").into()),
    }
    Ok(())
}
