//! DynamicData: a writer or a reader in a program that was not built with
//! the type of its topic. It gets the type from the other side's
//! endpoint, with the TypeLookup service, and writes or reads
//! [`DynamicData`] of it (rustorb-dds#34).
//!
//! ```text
//! dynamic pub <domain> <count> <appendable|mutable>
//! dynamic sub <domain> <count> <timeout_s> <appendable|mutable>
//! ```
//!
//! The peer is the `evolve` example, or `interop/cyclonedds/evolve.c`,
//! of either version: the topic is `evolve_appendable` or
//! `evolve_mutable`, RELIABLE and KEEP_ALL, in XCDR2. The publisher waits
//! for a reader, takes its type, and writes `count` samples (number 1,
//! 2, 3..., text "evolve <number>", and `extra` 100 + number when the
//! type has it). The subscriber takes the type of a writer and exits once
//! it has `count` samples with the expected number and text in order, or
//! with an error after `timeout_s` seconds or on a wrong sample.

use std::time::{Duration, Instant};

use rustorb_dds::{
    DataReaderQos, DataWriterQos, DomainParticipant, DynamicData, DynamicDataFactory, History,
    MEMBER_ID_INVALID, Reliability, data_representation,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: dynamic pub <domain> <count> <appendable|mutable>\n       \
                 dynamic sub <domain> <count> <timeout_s> <appendable|mutable>";
    if args.len() < 5 {
        return Err(usage.into());
    }
    let sub = args[1] == "sub";
    let kind = args.get(if sub { 5 } else { 4 }).ok_or(usage)?;
    if kind != "appendable" && kind != "mutable" {
        return Err(usage.into());
    }
    let topic_name = format!("evolve_{kind}");
    let domain: u32 = args[2].parse()?;
    let count: i32 = args[3].parse()?;
    let participant = DomainParticipant::new(domain)?;
    let wait = if sub {
        Duration::from_secs(args[4].parse()?)
    } else {
        Duration::from_secs(60)
    };
    let deadline = Instant::now() + wait;
    println!("rustorb-dds: waiting for the type of {topic_name}...");
    let ty = participant
        .discover_topic_type(&topic_name, wait)?
        .ok_or("rustorb-dds: the type of the topic did not come")?;
    println!(
        "rustorb-dds: got type {} ({} members)",
        ty.get_name(),
        ty.get_member_count()
    );
    let topic = participant.create_dynamic_topic(&topic_name, &ty)?;
    let id = |name: &str| {
        ty.get_member_by_name(name)
            .map(|m| m.get_id())
            .unwrap_or(MEMBER_ID_INVALID)
    };
    let (number, text, extra) = (id("number"), id("text"), id("extra"));
    if args[1] == "pub" {
        let qos = DataWriterQos {
            reliability: Reliability::reliable(),
            history: History::KeepAll,
            data_representation: vec![data_representation::XCDR2],
            ..DataWriterQos::default()
        };
        let writer = participant
            .create_publisher()?
            .create_datawriter_with_qos(&topic, qos)?;
        println!("rustorb-dds: waiting for a subscriber...");
        while !writer.wait_for_subscriptions(1, Duration::from_secs(1)) {}
        std::thread::sleep(Duration::from_millis(500));
        for n in 1..=count {
            let mut sample = DynamicDataFactory::create_data(&ty);
            sample.set_int32_value(number, n)?;
            sample.set_string_value(text, &format!("evolve {n}"))?;
            if extra != MEMBER_ID_INVALID {
                sample.set_int32_value(extra, 100 + n)?;
            }
            writer.write(&sample)?;
            println!("rustorb-dds: sent {n}");
            std::thread::sleep(Duration::from_millis(20));
        }
        writer.wait_for_acknowledgments(Duration::from_secs(30))?;
        println!("rustorb-dds: all acknowledged");
    } else {
        let qos = DataReaderQos {
            reliability: Reliability::reliable(),
            history: History::KeepAll,
            data_representation: vec![data_representation::XCDR2],
            ..DataReaderQos::default()
        };
        let reader = participant
            .create_subscriber()?
            .create_datareader_with_qos(&topic, qos)?;
        let mut got = 0;
        while got < count {
            if Instant::now() > deadline {
                return Err(format!("rustorb-dds: got {got} of {count} samples").into());
            }
            let samples: Vec<DynamicData> =
                reader.take().into_iter().filter_map(|s| s.data).collect();
            if samples.is_empty() {
                std::thread::sleep(Duration::from_millis(20));
            }
            for s in samples {
                got += 1;
                let want = format!("evolve {got}");
                let (n, t) = (s.get_int32_value(number)?, s.get_string_value(text)?);
                if n != got || t != want {
                    return Err(format!("got number {n} text {t:?}, want {got} {want:?}").into());
                }
                if extra != MEMBER_ID_INVALID {
                    println!(
                        "rustorb-dds: received {got} (extra {})",
                        s.get_int32_value(extra)?
                    );
                } else {
                    println!("rustorb-dds: received {got}");
                }
            }
        }
    }
    println!(
        "rustorb-dds: {} TypeObjects came by TypeLookup",
        participant.learnt_types()
    );
    Ok(())
}
