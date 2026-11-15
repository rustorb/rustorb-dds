//! Hello world over DDS.
//!
//! In one terminal: `cargo run --example hello_world -- sub`
//! In another:      `cargo run --example hello_world -- pub`
//!
//! Both join domain 0 and use the topic `hello_world` of type
//! `HelloWorld::Greeting`. The full form, used by `interop/run.sh`:
//!
//! ```text
//! hello_world pub <domain> <count> <interval_ms> [best_effort|reliable|transient_local|transient|persistent|filtered [size]]
//! hello_world sub <domain> <count> <timeout_s> [best_effort|reliable|transient_local|transient|filtered [size]]
//! ```
//!
//! The publisher waits for a subscriber, then writes `count` samples. The
//! subscriber exits after `count` samples, or with an error after
//! `timeout_s` seconds.
//!
//! The last argument picks the RELIABILITY of both sides (best effort by
//! default). When reliable, the publisher keeps every sample (KEEP_ALL)
//! and waits for the subscriber to acknowledge them before it exits, and
//! the subscriber checks that it gets 1, 2, 3... with none missing.
//!
//! `transient_local` is reliable and TRANSIENT_LOCAL: the publisher
//! writes its samples at once, before any subscriber is there, then
//! waits for one and for its acknowledgments; the subscriber creates its
//! reader two seconds late, and must still get them all.
//!
//! `transient` is like `transient_local` with TRANSIENT durability, and
//! the publisher deletes its writer once it has written: its participant
//! keeps the samples (with a KEEP_ALL DURABILITY_SERVICE) and sends them
//! to the subscriber. The publisher then waits for another participant
//! to come and go. `persistent` (publisher only) writes them with a
//! PERSISTENT writer of a first participant, deletes that participant,
//! and creates a PERSISTENT writer in a second one, which takes them up
//! from its file (in `$RUSTORB_DDS_PERSISTENCE_DIR`) and sends them.
//!
//! `filtered` is reliable, and the subscriber reads a content filtered
//! topic, `index > %0 AND message LIKE 'Hello%'` with `count / 2` for
//! `%0`: of the `count` samples sent, it must get those after the first
//! `count / 2`, in order, and no others.
//!
//! The message is "Hello, world!", or with `size`, `size` letters that
//! depend on the index (`a` + (i + index) % 26 for the i-th), so that a
//! sample too large for a datagram goes as DATA_FRAG. The subscriber
//! checks it.
//!
//! The subscriber waits for samples on a `WaitSet`, with the
//! StatusCondition of its reader enabled for DATA_AVAILABLE.
//!
//! Setting `RUSTORB_DROP_ONE_IN=n` drops one in `n` user datagrams
//! received, at random, to test that reliable delivery repairs losses.
//!
//! With the feature `security`, `RUSTORB_DDS_SECURITY=<file>` turns on DDS
//! Security: the file holds the `dds.sec.*` properties of
//! [`SecurityConfig::from_properties`](rustorb_dds::SecurityConfig), one
//! `name=value` per line.
//!
//! Both sides register the type with its DDS-XTypes description, so they
//! announce its TypeInformation. `RUSTORB_NO_TYPE_INFORMATION=1` leaves
//! that out (matching by type name only), and `RUSTORB_XCDR2=1` makes the
//! publisher write XCDR2.

mod greeting;

use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{
    DataReaderQos, DataWriterQos, DomainParticipant, DurabilityKind, DurabilityService, Error,
    History, ParticipantConfig, Reliability, StatusMask, WaitSet, data_representation,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let role = args.get(1).map(String::as_str).unwrap_or("pub");
    let num = |i: usize, default: u64| -> Result<u64, Box<dyn std::error::Error>> {
        Ok(match args.get(i) {
            Some(v) => v.parse()?,
            None => default,
        })
    };
    let domain = num(2, 0)? as u32;
    let count = num(3, 10)? as u32;
    let filtered = args.get(5).map(String::as_str) == Some("filtered");
    let durability = match args.get(5).map(String::as_str) {
        Some("transient_local") => DurabilityKind::TransientLocal,
        Some("transient") => DurabilityKind::Transient,
        Some("persistent") => DurabilityKind::Persistent,
        _ => DurabilityKind::Volatile,
    };
    let (reliable, transient_local) = match args.get(5).map(String::as_str) {
        None | Some("best_effort") => (false, false),
        Some("reliable" | "filtered") => (true, false),
        Some("transient_local" | "transient" | "persistent") => (true, true),
        Some(other) => return Err(format!("unknown reliability {other:?}").into()),
    };
    // The samples outlive the writer.
    let outlive = durability >= DurabilityKind::Transient;
    let size = match args.get(6) {
        Some(v) => Some(v.parse::<usize>()?),
        None => None,
    };
    let message = |index: u32| -> String {
        match size {
            None => "Hello, world!".into(),
            Some(n) => (0..n)
                .map(|i| char::from(b'a' + ((i as u64 + index as u64) % 26) as u8))
                .collect(),
        }
    };
    let mut config = ParticipantConfig::new(domain);
    if let Ok(n) = std::env::var("RUSTORB_DROP_ONE_IN") {
        config.drop_one_in = Some(n.parse()?);
    }
    #[cfg(feature = "security")]
    if let Some(path) = std::env::var_os("RUSTORB_DDS_SECURITY") {
        let text = std::fs::read_to_string(path)?;
        let props = text
            .lines()
            .filter_map(|l| l.split_once('='))
            .map(|(k, v)| (k.trim(), v.trim()));
        config.security = Some(
            rustorb_dds::SecurityConfig::from_properties(props)
                .ok_or("RUSTORB_DDS_SECURITY: a dds.sec property is missing")?,
        );
    }
    let join = |config: ParticipantConfig| -> Result<_, Box<dyn std::error::Error>> {
        let participant = DomainParticipant::with_config(config)?;
        // A content filter needs the type's description.
        if filtered || std::env::var_os("RUSTORB_NO_TYPE_INFORMATION").is_none() {
            participant.register_type::<Greeting>()?;
        }
        let topic = participant.create_topic::<Greeting>("hello_world")?;
        Ok((participant, topic))
    };
    let first = (role == "pub" && durability == DurabilityKind::Persistent)
        .then(|| join(config.clone()))
        .transpose()?;
    let (participant, topic) = join(config)?;
    match role {
        "pub" => {
            let interval = Duration::from_millis(num(4, 1000)?);
            let qos = if reliable {
                DataWriterQos {
                    reliability: Reliability::reliable(),
                    history: History::KeepAll,
                    durability,
                    durability_service: DurabilityService {
                        history: History::KeepAll,
                        ..DurabilityService::default()
                    },
                    ..DataWriterQos::default()
                }
            } else {
                DataWriterQos {
                    reliability: Reliability::best_effort(),
                    ..DataWriterQos::default()
                }
            };
            let qos = if std::env::var_os("RUSTORB_XCDR2").is_some() {
                DataWriterQos {
                    data_representation: vec![data_representation::XCDR2],
                    ..qos
                }
            } else {
                qos
            };
            // The samples are written by the first participant's writer,
            // and taken up by the second's.
            let publisher = match &first {
                Some((p, _)) => p.create_publisher()?,
                None => participant.create_publisher()?,
            };
            let writer = publisher
                .create_datawriter_with_qos(first.as_ref().map_or(&topic, |f| &f.1), qos.clone())?;
            if !transient_local {
                println!("rustorb-dds: waiting for a subscriber...");
                while !writer.wait_for_subscriptions(1, Duration::from_secs(1)) {}
                // Give the subscriber time to match this writer too.
                std::thread::sleep(Duration::from_millis(500));
            }
            for index in 1..=count {
                let g = Greeting {
                    index,
                    message: message(index),
                };
                writer.write(&g)?;
                println!("rustorb-dds: sent {index}");
                std::thread::sleep(interval);
            }
            if outlive {
                drop(writer);
                drop(publisher);
                println!("rustorb-dds: writer deleted");
                if let Some(first) = first {
                    drop(first);
                    // For the second participant to see it leave.
                    std::thread::sleep(Duration::from_secs(1));
                    let _writer = participant
                        .create_publisher()?
                        .create_datawriter_with_qos(&topic, qos)?;
                    println!("rustorb-dds: samples taken up by a new participant");
                    wait_for_a_visit(&participant);
                } else {
                    wait_for_a_visit(&participant);
                }
                return Ok(());
            }
            if transient_local {
                println!("rustorb-dds: waiting for a subscriber...");
                while !writer.wait_for_subscriptions(1, Duration::from_secs(1)) {}
            }
            if reliable {
                writer.wait_for_acknowledgments(Duration::from_secs(30))?;
                println!("rustorb-dds: all acknowledged");
            }
        }
        "sub" => {
            let deadline = Instant::now() + Duration::from_secs(num(4, 3600)?);
            let qos = DataReaderQos {
                reliability: if reliable {
                    Reliability::reliable()
                } else {
                    Reliability::best_effort()
                },
                history: History::KeepAll,
                durability,
                ..DataReaderQos::default()
            };
            if transient_local {
                // Join late: what was written before must still come.
                std::thread::sleep(Duration::from_secs(2));
            }
            // With a filter, the first `skip` samples are not to come.
            let skip = if filtered { count / 2 } else { 0 };
            let subscriber = participant.create_subscriber()?;
            let reader = if filtered {
                let cft = participant.create_contentfilteredtopic(
                    "hello_world_filtered",
                    &topic,
                    "index > %0 AND message LIKE 'Hello%'",
                    &[skip.to_string()],
                )?;
                subscriber.create_datareader_with_qos(&cft, qos)?
            } else {
                subscriber.create_datareader_with_qos(&topic, qos)?
            };
            let status = reader.status_condition();
            status.set_enabled_statuses(StatusMask::DATA_AVAILABLE);
            let waitset = WaitSet::new();
            waitset.attach_condition(&status)?;
            println!("rustorb-dds: waiting for greetings...");
            let mut got = 0;
            let mut without_data = 0;
            while got < count - skip {
                let left = deadline.saturating_duration_since(Instant::now());
                match waitset.wait(left) {
                    Err(Error::Timeout) => {
                        let matched = reader.subscription_matched_status().current_count;
                        return Err(format!(
                            "rustorb-dds: got {got} of {count} samples \
                             ({without_data} without data, {matched} writers matched)"
                        )
                        .into());
                    }
                    r => r?,
                };
                // Samples without data tell that the writer is gone.
                let taken = reader.take();
                without_data += taken.iter().filter(|s| s.data.is_none()).count();
                for s in taken.into_iter().filter_map(|s| s.data) {
                    if s.message != message(s.index) {
                        let start: String = s.message.chars().take(40).collect();
                        return Err(format!(
                            "bad message {start:?}... ({} octets) for {}",
                            s.message.len(),
                            s.index
                        )
                        .into());
                    }
                    println!("rustorb-dds: received {}", s.index);
                    got += 1;
                    if reliable && s.index != got + skip {
                        return Err(format!("expected {}, got {}", got + skip, s.index).into());
                    }
                }
            }
        }
        _ => return Err(format!("unknown role {role:?}: use pub or sub").into()),
    }
    Ok(())
}

/// Waits, 30 s at most, for another participant to come, then for
/// it to leave.
fn wait_for_a_visit(participant: &DomainParticipant) {
    println!("rustorb-dds: waiting for a subscriber to come and go...");
    let deadline = Instant::now() + Duration::from_secs(30);
    while participant.discovered_participants().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
    while !participant.discovered_participants().is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(100));
    }
}
