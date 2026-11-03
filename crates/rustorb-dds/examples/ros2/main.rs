//! Talks with ROS 2 nodes: a talker and a listener like those of ROS 2's
//! `demo_nodes_cpp`, on the topic `chatter` of type
//! `std_msgs/msg/String`.
//!
//! ```text
//! ros2 talker [domain [count [interval_ms]]]
//! ros2 listener [domain [count [timeout_s]]]
//! ```
//!
//! The talker waits for a listener, then publishes "Hello World: 1",
//! "Hello World: 2"... every
//! `interval_ms` (default 1000) until it has sent `count` (default: no
//! end). The listener prints what it hears, and exits after `count`
//! messages, or with an error after `timeout_s` seconds (default 60).
//!
//! ROS 2 puts a topic `name` on DDS as `rt/name`, and a message type
//! `pkg/msg/Name` as `pkg::msg::dds_::Name_`; `std_msgs.rs` is what
//! `rustorb-idl -brust -Wbdds_only` generates for `interop/idl/String.idl`.
//! The QoS are those of ROS 2's default profile: RELIABLE, VOLATILE,
//! KEEP_LAST(10).
//!
//! Like ROS 2 (Jazzy), the participant announces `enclave=/;` in its
//! USER_DATA, and the writer and reader `typehash=RIHS01_...;` with the
//! hash ROS 2 gives `std_msgs/msg/String`; `ros2 topic info -v` shows
//! that hash. ROS 2 lists the endpoints under the node
//! `_NODE_NAME_UNKNOWN_`: a ROS 2 node is also announced on the
//! topic `ros_discovery_info`, which this example does not do.

#[allow(dead_code, clippy::all)]
mod std_msgs;

use std::time::{Duration, Instant};

use rustorb_dds::{
    DataReaderQos, DataWriterQos, DomainParticipant, Error, History, ParticipantConfig,
    Reliability, StatusMask, WaitSet,
};
use std_msgs::std_msgs::msg::dds::String as StringMsg;

/// The DDS topic of the ROS 2 topic `/chatter`.
const TOPIC: &str = "rt/chatter";
/// The depth of ROS 2's default QoS profile.
const DEPTH: usize = 10;
/// The USER_DATA of a ROS 2 participant.
const ENCLAVE: &[u8] = b"enclave=/;";
/// The USER_DATA of ROS 2 endpoints of type `std_msgs/msg/String`: its
/// type hash, as ROS 2 Jazzy announces it.
const TYPE_HASH: &[u8] =
    b"typehash=RIHS01_df668c740482bbd48fb39d76a70dfd4bd59db1288021743503259e948f6b1a18;";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let role = args.get(1).map(String::as_str).unwrap_or("listener");
    let arg = |i: usize| -> Result<Option<u64>, Box<dyn std::error::Error>> {
        Ok(match args.get(i) {
            Some(v) => Some(v.parse()?),
            None => None,
        })
    };
    let domain = arg(2)?.unwrap_or(0) as u32;
    let count = arg(3)?;
    let mut config = ParticipantConfig::new(domain);
    config.user_data = ENCLAVE.to_vec();
    let participant = DomainParticipant::with_config(config)?;
    let topic = participant.create_topic::<StringMsg>(TOPIC)?;
    match role {
        "talker" => {
            let interval = Duration::from_millis(arg(4)?.unwrap_or(1000));
            let writer = participant.create_publisher()?.create_datawriter_with_qos(
                &topic,
                DataWriterQos {
                    reliability: Reliability::reliable(),
                    history: History::KeepLast(DEPTH),
                    user_data: TYPE_HASH.to_vec(),
                    ..DataWriterQos::default()
                },
            )?;
            // A ROS 2 talker starts at once; this one waits for a
            // listener, so that the first messages are not lost while it
            // matches this writer.
            while !writer.wait_for_subscriptions(1, Duration::from_secs(1)) {}
            std::thread::sleep(Duration::from_millis(500));
            let mut i = 0u64;
            while count.is_none_or(|c| i < c) {
                i += 1;
                let data = format!("Hello World: {i}");
                println!("rustorb-dds: Publishing: '{data}'");
                writer.write(&StringMsg { data })?;
                std::thread::sleep(interval);
            }
            writer.wait_for_acknowledgments(Duration::from_secs(10))?;
        }
        "listener" => {
            let deadline = Instant::now() + Duration::from_secs(arg(4)?.unwrap_or(60));
            let reader = participant
                .create_subscriber()?
                .create_datareader_with_qos(
                    &topic,
                    DataReaderQos {
                        reliability: Reliability::reliable(),
                        history: History::KeepLast(DEPTH),
                        user_data: TYPE_HASH.to_vec(),
                        ..DataReaderQos::default()
                    },
                )?;
            let status = reader.status_condition();
            status.set_enabled_statuses(StatusMask::DATA_AVAILABLE);
            let waitset = WaitSet::new();
            waitset.attach_condition(&status)?;
            let mut got = 0u64;
            while count.is_none_or(|c| got < c) {
                let left = deadline.saturating_duration_since(Instant::now());
                match waitset.wait(left) {
                    Err(Error::Timeout) => {
                        return Err(format!("rustorb-dds: heard {got} messages").into());
                    }
                    r => r?,
                };
                for m in reader.take().into_iter().filter_map(|s| s.data) {
                    println!("rustorb-dds: I heard: [{}]", m.data);
                    got += 1;
                }
            }
        }
        _ => return Err(format!("unknown role {role:?}: use talker or listener").into()),
    }
    Ok(())
}
