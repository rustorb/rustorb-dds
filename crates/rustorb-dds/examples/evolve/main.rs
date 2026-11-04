//! Type evolution: a writer and a reader whose types differ, matched
//! through DDS-XTypes assignability after each side fetched the other's
//! TypeObjects with the TypeLookup service (rustorb-dds#33).
//!
//! ```text
//! evolve <v1|v2> pub <domain> <count> <appendable|mutable>
//! evolve <v1|v2> sub <domain> <count> <timeout_s> <appendable|mutable>
//! ```
//!
//! The types are those of `interop/idl/EvolveV1.idl` and
//! `EvolveV2.idl` (`v1.rs` and `v2.rs`, written by
//! `rustorb-idl -brust -Wbdds_only -Wbxtypes`): version 2 adds a member
//! to each. Both versions have the same type names, so the endpoints
//! would match by name anyway; what TypeLookup brings is that each side
//! knows the other's type and checks that it is assignable.
//!
//! The topic is `evolve_appendable` or `evolve_mutable`, RELIABLE and
//! KEEP_ALL, written in XCDR2 (APPENDABLE and MUTABLE types need it to
//! evolve). The publisher waits for a reader, writes `count` samples
//! (number 1, 2, 3..., text "evolve <number>", extra 100 + number in
//! version 2) and waits for acknowledgments. The subscriber exits once it
//! has `count` samples with the expected number and text in order, or
//! with an error after `timeout_s` seconds or on a wrong sample. The
//! same program is `interop/cyclonedds/evolve.c` for Cyclone DDS.
//!
//! An endpoint is matched only once the TypeObject of the other side's
//! type has come (when it is another version). With
//! `RUSTORB_EXPECT_TYPE_LOOKUP=1` the program checks at the end that its
//! participant got a TypeObject that way; that the versions differ is up
//! to the caller.

#[allow(dead_code)] // Generated: not all of it is used.
#[path = "v1.rs"]
mod v1;
#[allow(dead_code)]
#[path = "v2.rs"]
mod v2;

use std::time::{Duration, Instant};

use rustorb_dds::{
    DataReaderQos, DataWriterQos, DescribeType, DomainParticipant, History, Reliability,
    data_representation,
};
use rustorb_typesupport::TopicType;

/// What both versions of both types have in common.
trait Sample: TopicType + DescribeType + Send + 'static {
    fn new(number: i32, text: String) -> Self;
    fn number(&self) -> i32;
    fn text(&self) -> &str;
}

macro_rules! sample {
    ($t:ty, $($extra:ident)?) => {
        impl Sample for $t {
            fn new(number: i32, text: String) -> Self {
                Self {
                    number,
                    text,
                    $($extra: 100 + number,)?
                }
            }
            fn number(&self) -> i32 {
                self.number
            }
            fn text(&self) -> &str {
                &self.text
            }
        }
    };
}

sample!(v1::evolve::Appendable,);
sample!(v1::evolve::Mutable,);
sample!(v2::evolve::Appendable, extra);
sample!(v2::evolve::Mutable, extra);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let usage = "usage: evolve <v1|v2> pub <domain> <count> <appendable|mutable>\n       \
                 evolve <v1|v2> sub <domain> <count> <timeout_s> <appendable|mutable>";
    if args.len() < 6 {
        return Err(usage.into());
    }
    let sub = args[2] == "sub";
    let kind = args.get(if sub { 6 } else { 5 }).ok_or(usage)?;
    let run = match (args[1].as_str(), kind.as_str()) {
        ("v1", "appendable") => run::<v1::evolve::Appendable>,
        ("v1", "mutable") => run::<v1::evolve::Mutable>,
        ("v2", "appendable") => run::<v2::evolve::Appendable>,
        ("v2", "mutable") => run::<v2::evolve::Mutable>,
        _ => return Err(usage.into()),
    };
    run(&args[2..], &format!("evolve_{kind}"))
}

fn run<T: Sample>(args: &[String], topic_name: &str) -> Result<(), Box<dyn std::error::Error>> {
    let domain: u32 = args[1].parse()?;
    let count: i32 = args[2].parse()?;
    let participant = DomainParticipant::new(domain)?;
    participant.register_type::<T>()?;
    let topic = participant.create_topic::<T>(topic_name)?;
    match args[0].as_str() {
        "pub" => {
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
            for number in 1..=count {
                writer.write(&T::new(number, format!("evolve {number}")))?;
                println!("rustorb-dds: sent {number}");
                std::thread::sleep(Duration::from_millis(20));
            }
            writer.wait_for_acknowledgments(Duration::from_secs(30))?;
            println!("rustorb-dds: all acknowledged");
        }
        "sub" => {
            let deadline = Instant::now() + Duration::from_secs(args[3].parse()?);
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
                let samples: Vec<T> = reader.take().into_iter().filter_map(|s| s.data).collect();
                if samples.is_empty() {
                    std::thread::sleep(Duration::from_millis(20));
                }
                for s in samples {
                    got += 1;
                    let want = format!("evolve {got}");
                    if s.number() != got || s.text() != want {
                        return Err(format!(
                            "got number {} text {:?}, want {got} {want:?}",
                            s.number(),
                            s.text()
                        )
                        .into());
                    }
                    println!("rustorb-dds: received {got}");
                }
            }
        }
        other => return Err(format!("unknown role {other:?}").into()),
    }
    if std::env::var_os("RUSTORB_EXPECT_TYPE_LOOKUP").is_some() {
        let learnt = participant.learnt_types();
        if learnt == 0 {
            return Err("rustorb-dds: no TypeObject came by TypeLookup".into());
        }
        println!("rustorb-dds: {learnt} TypeObjects came by TypeLookup");
    }
    Ok(())
}
