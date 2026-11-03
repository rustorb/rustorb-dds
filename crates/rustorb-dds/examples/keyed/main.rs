//! Keyed instances over DDS: write, dispose and unregister.
//!
//! In one terminal: `cargo run --example keyed -- sub`
//! In another:      `cargo run --example keyed -- pub`
//!
//! Both join domain 0 and use the topic `keyed` of type `Keyed::Shape`,
//! whose key is `id`. The full form, used by `interop/run.sh`:
//!
//! ```text
//! keyed pub <domain> <count> <interval_ms>
//! keyed sub <domain> <count> <timeout_s>
//! ```
//!
//! Both sides are RELIABLE and KEEP_ALL. The publisher waits for a
//! subscriber, writes `count` samples to the instances id 1, 2, 3 in turn
//! (index 1, 2, 3...), then disposes id 2 and unregisters id 3 (the writer
//! does not dispose unregistered instances), and waits for
//! acknowledgments.
//!
//! The subscriber exits once it has `count` samples, each instance's in
//! order, and has seen id 2 NOT_ALIVE_DISPOSED and id 3
//! NOT_ALIVE_NO_WRITERS; with an error on timeout or a wrong sample.

mod shape;

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rustorb_dds::{
    DataReaderQos, DataWriterQos, DomainParticipant, History, InstanceState, Reliability,
    WriterDataLifecycle,
};
use shape::Shape;

const MESSAGE: &str = "Hello, instance!";

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
    let count = num(3, 30)? as u32;
    let participant = DomainParticipant::new(domain)?;
    let topic = participant.create_topic::<Shape>("keyed")?;
    let shape = |id, index| Shape {
        id,
        index,
        message: MESSAGE.into(),
    };
    match role {
        "pub" => {
            let interval = Duration::from_millis(num(4, 100)?);
            let qos = DataWriterQos {
                reliability: Reliability::reliable(),
                history: History::KeepAll,
                writer_data_lifecycle: WriterDataLifecycle {
                    autodispose_unregistered_instances: false,
                },
                ..DataWriterQos::default()
            };
            let writer = participant
                .create_publisher()?
                .create_datawriter_with_qos(&topic, qos)?;
            println!("rustorb-dds: waiting for a subscriber...");
            while !writer.wait_for_subscriptions(1, Duration::from_secs(1)) {}
            std::thread::sleep(Duration::from_millis(500));
            for index in 1..=count {
                let id = (index - 1) % 3 + 1;
                writer.write(&shape(id, index))?;
                println!("rustorb-dds: sent id {id} index {index}");
                std::thread::sleep(interval);
            }
            writer.dispose(&shape(2, 0))?;
            writer.unregister_instance(&shape(3, 0))?;
            println!("rustorb-dds: disposed 2, unregistered 3");
            writer.wait_for_acknowledgments(Duration::from_secs(30))?;
            println!("rustorb-dds: all acknowledged");
        }
        "sub" => {
            let deadline = Instant::now() + Duration::from_secs(num(4, 3600)?);
            let qos = DataReaderQos {
                reliability: Reliability::reliable(),
                history: History::KeepAll,
                ..DataReaderQos::default()
            };
            let reader = participant
                .create_subscriber()?
                .create_datareader_with_qos(&topic, qos)?;
            println!("rustorb-dds: waiting for shapes...");
            let mut got = 0;
            let mut last: HashMap<u32, u32> = HashMap::new();
            let (mut disposed, mut no_writers) = (false, false);
            while got < count || !disposed || !no_writers {
                if Instant::now() >= deadline {
                    return Err(format!(
                        "rustorb-dds: got {got} of {count} samples, disposed {disposed}, \
                         no writers {no_writers}"
                    )
                    .into());
                }
                if !reader.wait_for_data(Duration::from_millis(200)) {
                    continue;
                }
                for s in reader.take() {
                    match s.data {
                        Some(d) => {
                            let prev = last.get(&d.id).copied().unwrap_or(0);
                            if d.message != MESSAGE
                                || d.index <= prev
                                || (d.index - 1) % 3 + 1 != d.id
                            {
                                return Err(format!("bad sample {d:?}").into());
                            }
                            last.insert(d.id, d.index);
                            got += 1;
                            println!("rustorb-dds: received id {} index {}", d.id, d.index);
                        }
                        None => {
                            let id = reader
                                .get_key_value(s.info.instance_handle)
                                .map_or(0, |k| k.id);
                            println!("rustorb-dds: id {id} is {:?}", s.info.instance_state);
                            disposed |=
                                id == 2 && s.info.instance_state == InstanceState::NotAliveDisposed;
                            no_writers |= id == 3
                                && s.info.instance_state == InstanceState::NotAliveNoWriters;
                        }
                    }
                }
            }
        }
        _ => return Err(format!("unknown role {role:?}: use pub or sub").into()),
    }
    Ok(())
}
