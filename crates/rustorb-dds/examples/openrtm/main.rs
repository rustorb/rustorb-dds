//! Talks with OpenRTM-aist 2.0 data ports that use its Fast DDS
//! transport (`interface_type=fast-rtps`): like an OutPort or an InPort
//! of `RTC::TimedLong`, on the DDS topic of the connection.
//!
//! ```text
//! openrtm out [domain [count [interval_ms [topic]]]]
//! openrtm in [domain [count [timeout_s [topic]]]]
//! ```
//!
//! `out` waits for a reader, then writes `data` 1, 2, 3... every
//! `interval_ms` (default 1000) until it has written `count` (default: no
//! end). `in` prints the data it gets, and exits after `count` samples,
//! or with an error after `timeout_s` seconds (default 60). The topic is
//! `chatter` by default, as in OpenRTM-aist when the connection does not
//! set `fast-rtps.topic`.
//!
//! OpenRTM-aist puts the data type of a port on DDS under its IDL name
//! (`RTC::TimedLong`) and writes plain CDR (docs/openrtm.md), so
//! `open_rtm.rs` is what `rustorb-idl -brust -Wbdds_only
//! -Wbextensibility=final` generates for `interop/idl/OpenRTM.idl`. The
//! QoS are those of OpenRTM-aist's ports by default: BEST_EFFORT,
//! KEEP_LAST(1).

#[allow(dead_code, clippy::all)]
mod open_rtm;

use std::time::{Duration, Instant};

use open_rtm::rtc::{Time, TimedLong};
use rustorb_dds::{
    DataReaderQos, DataWriterQos, DomainParticipant, Error, History, Reliability, StatusMask,
    WaitSet,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let role = args.get(1).map(String::as_str).unwrap_or("in");
    let arg = |i: usize| -> Result<Option<u64>, Box<dyn std::error::Error>> {
        Ok(match args.get(i) {
            Some(v) => Some(v.parse()?),
            None => None,
        })
    };
    let domain = arg(2)?.unwrap_or(0) as u32;
    let count = arg(3)?;
    let topic_name = args.get(5).map(String::as_str).unwrap_or("chatter");
    let participant = DomainParticipant::new(domain)?;
    let topic = participant.create_topic::<TimedLong>(topic_name)?;
    match role {
        "out" => {
            let interval = Duration::from_millis(arg(4)?.unwrap_or(1000));
            let writer = participant.create_publisher()?.create_datawriter_with_qos(
                &topic,
                DataWriterQos {
                    reliability: Reliability::best_effort(),
                    history: History::KeepLast(1),
                    ..DataWriterQos::default()
                },
            )?;
            while !writer.wait_for_subscriptions(1, Duration::from_secs(1)) {}
            std::thread::sleep(Duration::from_millis(500));
            let mut i = 0u64;
            while count.is_none_or(|c| i < c) {
                i += 1;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default();
                let sample = TimedLong {
                    tm: Time {
                        sec: now.as_secs() as u32,
                        nsec: now.subsec_nanos(),
                    },
                    data: i as i32,
                };
                println!("rustorb-dds: write {}", sample.data);
                writer.write(&sample)?;
                std::thread::sleep(interval);
            }
        }
        "in" => {
            let deadline = Instant::now() + Duration::from_secs(arg(4)?.unwrap_or(60));
            let reader = participant
                .create_subscriber()?
                .create_datareader_with_qos(
                    &topic,
                    DataReaderQos {
                        reliability: Reliability::best_effort(),
                        history: History::KeepLast(1),
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
                        return Err(format!("rustorb-dds: got {got} samples").into());
                    }
                    r => r?,
                };
                for s in reader.take().into_iter().filter_map(|s| s.data) {
                    println!(
                        "rustorb-dds: read {} (tm {}.{:09})",
                        s.data, s.tm.sec, s.tm.nsec
                    );
                    got += 1;
                }
            }
        }
        _ => return Err(format!("unknown role {role:?}: use out or in").into()),
    }
    Ok(())
}
