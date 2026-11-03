//! Hello world over DDS.
//!
//! In one terminal: `cargo run --example hello_world -- sub`
//! In another:      `cargo run --example hello_world -- pub`
//!
//! Both join domain 0 and use the topic `hello_world` of type
//! `HelloWorld::Greeting`. The full form, used by `interop/run.sh`:
//!
//! ```text
//! hello_world pub <domain> <count> <interval_ms> [best_effort|reliable [size]]
//! hello_world sub <domain> <count> <timeout_s> [best_effort|reliable [size]]
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
//! The message is "Hello, world!", or with `size`, `size` letters that
//! depend on the index (`a` + (i + index) % 26 for the i-th), so that a
//! sample too large for a datagram goes as DATA_FRAG. The subscriber
//! checks it.
//!
//! Setting `RUSTORB_DROP_ONE_IN=n` drops one in `n` user datagrams
//! received, at random, to test that reliable delivery repairs losses.

mod greeting;

use std::time::{Duration, Instant};

use greeting::Greeting;
use rustorb_dds::{
    DataReaderQos, DataWriterQos, DomainParticipant, History, ParticipantConfig, Reliability,
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
    let reliable = match args.get(5).map(String::as_str) {
        None | Some("best_effort") => false,
        Some("reliable") => true,
        Some(other) => return Err(format!("unknown reliability {other:?}").into()),
    };
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
    let participant = DomainParticipant::with_config(config)?;
    let topic = participant.create_topic::<Greeting>("hello_world")?;
    match role {
        "pub" => {
            let interval = Duration::from_millis(num(4, 1000)?);
            let qos = if reliable {
                DataWriterQos {
                    reliability: Reliability::reliable(),
                    history: History::KeepAll,
                    ..DataWriterQos::default()
                }
            } else {
                DataWriterQos {
                    reliability: Reliability::best_effort(),
                    ..DataWriterQos::default()
                }
            };
            let writer = participant
                .create_publisher()?
                .create_datawriter_with_qos(&topic, qos)?;
            println!("rustorb-dds: waiting for a subscriber...");
            while !writer.wait_for_subscriptions(1, Duration::from_secs(1)) {}
            // Give the subscriber time to match this writer too.
            std::thread::sleep(Duration::from_millis(500));
            for index in 1..=count {
                let g = Greeting {
                    index,
                    message: message(index),
                };
                writer.write(&g)?;
                println!("rustorb-dds: sent {index}");
                std::thread::sleep(interval);
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
            };
            let reader = participant
                .create_subscriber()?
                .create_datareader_with_qos(&topic, qos)?;
            println!("rustorb-dds: waiting for greetings...");
            let mut got = 0;
            while got < count {
                if Instant::now() >= deadline {
                    return Err(format!("rustorb-dds: got {got} of {count} samples").into());
                }
                if reader.wait_for_data(Duration::from_millis(200)) {
                    // Samples without data tell that the writer is gone.
                    for s in reader.take().into_iter().filter_map(|s| s.data) {
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
                        if reliable && s.index != got {
                            return Err(format!("expected {got}, got {}", s.index).into());
                        }
                    }
                }
            }
        }
        _ => return Err(format!("unknown role {role:?}: use pub or sub").into()),
    }
    Ok(())
}
