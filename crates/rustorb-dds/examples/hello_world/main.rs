//! Hello world over DDS.
//!
//! In one terminal: `cargo run --example hello_world -- sub`
//! In another:      `cargo run --example hello_world -- pub`
//!
//! Both join domain 0 (or the domain given as the second argument) and
//! use the topic `hello_world` of type `HelloWorld::Greeting`.

mod greeting;

use std::time::Duration;

use greeting::Greeting;
use rustorb_dds::DomainParticipant;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let role = args.get(1).map(String::as_str).unwrap_or("pub");
    let domain: u32 = match args.get(2) {
        Some(d) => d.parse()?,
        None => 0,
    };
    let participant = DomainParticipant::new(domain)?;
    let topic = participant.create_topic::<Greeting>("hello_world")?;
    match role {
        "pub" => {
            let writer = participant.create_publisher()?.create_datawriter(&topic)?;
            println!("waiting for a subscriber...");
            while !writer.wait_for_subscriptions(1, Duration::from_secs(1)) {}
            for index in 1..=10 {
                let g = Greeting {
                    index,
                    message: "Hello, world!".into(),
                };
                writer.write(&g)?;
                println!("sent {index}: {}", g.message);
                std::thread::sleep(Duration::from_secs(1));
            }
        }
        "sub" => {
            let reader = participant.create_subscriber()?.create_datareader(&topic)?;
            println!("waiting for greetings...");
            loop {
                if reader.wait_for_data(Duration::from_secs(1)) {
                    for s in reader.take() {
                        println!("received {}: {}", s.data.index, s.data.message);
                        if s.data.index == 10 {
                            return Ok(());
                        }
                    }
                }
            }
        }
        _ => return Err(format!("unknown role {role:?}: use pub or sub").into()),
    }
    Ok(())
}
