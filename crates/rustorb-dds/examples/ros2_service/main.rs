//! Calls and serves a ROS 2 service: a client and a server like
//! `add_two_ints_client` and `add_two_ints_server` of ROS 2's
//! `demo_nodes_cpp`, for the service `add_two_ints` of type
//! `example_interfaces/srv/AddTwoInts`, over DDS-RPC (the
//! [`rpc`](rustorb_dds::rpc) module).
//!
//! ```text
//! ros2_service server [domain [count [timeout_s]]]
//! ros2_service client [domain [count [timeout_s]]]
//! ```
//!
//! The server answers each request `a`, `b` with `a + b`, and exits after
//! `count` requests (default: no end), or with an error after `timeout_s`
//! seconds (default 60) without one. The client waits for the service,
//! asks for `i + 10 * i` for `i` from 1 to `count` (default 1), checks
//! each sum and prints "Result of add_two_ints: <sum>".
//!
//! ROS 2 puts a service `name` on DDS as a request topic `rq/nameRequest`
//! and a reply topic `rr/nameReply`, and a service type `pkg/srv/Name` as
//! the types `pkg::srv::dds_::Name_Request_` and `Name_Response_`
//! (`example_interfaces.rs`). A request names the client's reply reader as
//! its related sample identity and the reply names it back with the
//! request's sequence number, as `rpc` does. The QoS are those of ROS 2's
//! services profile: RELIABLE, VOLATILE, KEEP_LAST(10). As in the `ros2`
//! example, the participant announces `enclave=/;` and the endpoints the
//! type hashes ROS 2 (Jazzy) gives the request and response types.

#[allow(dead_code, clippy::all)]
mod example_interfaces;

use std::time::{Duration, Instant};

use example_interfaces::example_interfaces::srv::dds::{AddTwoIntsRequest, AddTwoIntsResponse};
use rustorb_dds::rpc::{Replier, Requester, RpcConfig};
use rustorb_dds::{DomainParticipant, History, ParticipantConfig};

/// The DDS topics of the ROS 2 service `/add_two_ints`.
const REQUEST_TOPIC: &str = "rq/add_two_intsRequest";
const REPLY_TOPIC: &str = "rr/add_two_intsReply";
/// The depth of ROS 2's services QoS profile.
const DEPTH: usize = 10;
/// The USER_DATA of a ROS 2 participant.
const ENCLAVE: &[u8] = b"enclave=/;";
/// The type hashes ROS 2 Jazzy announces for the request and response
/// types (seen in its SEDP announcements).
const REQUEST_HASH: &[u8] =
    b"typehash=RIHS01_000c5fd92d6b2e1a05949348f584d6d652adea1e92d691792011ac2273508302;";
const RESPONSE_HASH: &[u8] =
    b"typehash=RIHS01_de5c030d4af33cba2749310b249737b631594703f9300495f48bffb2b44dcc2f;";

/// The configuration of a client (`client`) or a server: the writer's
/// type is the request's for a client, the response's for a server.
fn config(client: bool) -> RpcConfig {
    let mut c = RpcConfig::with_topics(REQUEST_TOPIC, REPLY_TOPIC);
    c.writer_qos.history = History::KeepLast(DEPTH);
    c.reader_qos.history = History::KeepLast(DEPTH);
    let (writer, reader) = if client {
        (REQUEST_HASH, RESPONSE_HASH)
    } else {
        (RESPONSE_HASH, REQUEST_HASH)
    };
    c.writer_qos.user_data = writer.to_vec();
    c.reader_qos.user_data = reader.to_vec();
    c
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let role = args.get(1).map(String::as_str).unwrap_or("server");
    let arg = |i: usize| -> Result<Option<u64>, Box<dyn std::error::Error>> {
        Ok(match args.get(i) {
            Some(v) => Some(v.parse()?),
            None => None,
        })
    };
    let domain = arg(2)?.unwrap_or(0) as u32;
    let count = arg(3)?;
    let timeout = Duration::from_secs(arg(4)?.unwrap_or(60));
    let mut pc = ParticipantConfig::new(domain);
    pc.user_data = ENCLAVE.to_vec();
    let participant = DomainParticipant::with_config(pc)?;
    match role {
        "server" => {
            let replier = Replier::<AddTwoIntsRequest, AddTwoIntsResponse>::with_config(
                &participant,
                &config(false),
            )?;
            let mut served = 0u64;
            while count.is_none_or(|c| served < c) {
                let requests = replier.receive_requests(timeout)?;
                if requests.is_empty() {
                    return Err(format!("rustorb-dds: served {served} requests").into());
                }
                for r in requests {
                    println!(
                        "rustorb-dds: Incoming request a: {} b: {}",
                        r.data.a, r.data.b
                    );
                    let sum = r.data.a.wrapping_add(r.data.b);
                    replier.send_reply(&AddTwoIntsResponse { sum }, &r.id)?;
                    served += 1;
                }
            }
            // Let the last replies be acknowledged before leaving.
            replier
                .reply_datawriter()
                .wait_for_acknowledgments(Duration::from_secs(5))?;
        }
        "client" => {
            let requester = Requester::<AddTwoIntsRequest, AddTwoIntsResponse>::with_config(
                &participant,
                &config(true),
            )?;
            let deadline = Instant::now() + timeout;
            if !requester.wait_for_service(timeout) {
                return Err("rustorb-dds: service not available".into());
            }
            for i in 1..=count.unwrap_or(1) as i64 {
                let (a, b) = (i, 10 * i);
                let left = deadline.saturating_duration_since(Instant::now());
                let reply = requester.request(&AddTwoIntsRequest { a, b }, left)?;
                println!("rustorb-dds: Result of add_two_ints: {}", reply.sum);
                if reply.sum != a + b {
                    return Err(format!("rustorb-dds: {a} + {b} is not {}", reply.sum).into());
                }
            }
        }
        _ => return Err(format!("unknown role {role:?}: use server or client").into()),
    }
    Ok(())
}
