//! The measurements of rustorb-dds-perf (crates/rustorb-dds-perf) for
//! RustDDS, on its public API only, with the same command line, protocol
//! and JSON output, so that bench/run.py measures both the same way and
//! they can talk to each other (rustorb-dds#108).
//!
//! ```text
//! rustdds-perf ping|pong|pub|sub|disc [--domain N] [--size N | --sizes A,B,...]
//!      [--reliability reliable|best-effort] [--transport udp]
//!      [--warmup S] [--duration S] [--count N] [--rate N] [--depth N]
//!      [--reply-timeout MS] [--timeout S] [--label TEXT] [--json]
//!      [--participants N] [--start T]
//! ```
//!
//! The sample is RustorbPerf::Sample of bench/idl/Perf.idl, keyless, in
//! RustDDS's CDR serde adapter; the payload is a `serde_bytes::ByteBuf`,
//! which RustDDS writes and reads as one block of octets. A reader waits
//! with mio (RustDDS's DataReader is a mio 1 event source), and the
//! matched counts come from the status events (RustDDS 0.15 leaves
//! out the matched status of a keyless writer and reader).

use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use rustdds::dds::result::WriteError;
use rustdds::mio1::{Events, Interest, Poll, Token};
use rustdds::no_key::{DataReader, DataWriter};
use rustdds::policy::{History, Reliability, ResourceLimits};
use rustdds::serialization::{CDRDeserializerAdapter, CDRSerializerAdapter};
use rustdds::{
    DataReaderStatus, DataWriterStatus, DomainParticipant, QosPolicyBuilder, StatusEvented,
    TopicKind,
};
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;

/// RustorbPerf::Sample.
#[derive(Serialize, Deserialize, Clone, Debug)]
struct Sample {
    seq: u64,
    stamp: u64,
    payload: ByteBuf,
}

const TYPE_NAME: &str = "RustorbPerf::Sample";
/// The samples a best-effort sub keeps not yet taken (as rustorb-dds-perf).
const READER_MAX_SAMPLES: i32 = 1000;
const MAX_TAKE: usize = 256;
const VERSION: &str = env!("RUSTDDS_VERSION");

type Reader = DataReader<Sample, CDRDeserializerAdapter<Sample>>;
type Writer = DataWriter<Sample, CDRSerializerAdapter<Sample>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Ping,
    Pong,
    Pub,
    Sub,
    Disc,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Ping => "ping",
            Mode::Pong => "pong",
            Mode::Pub => "pub",
            Mode::Sub => "sub",
            Mode::Disc => "disc",
        }
    }
}

#[derive(Debug)]
struct Options {
    mode: Mode,
    domain: u16,
    sizes: Vec<usize>,
    reliable: bool,
    transport: String,
    warmup: f64,
    duration: f64,
    count: Option<u64>,
    rate: f64,
    depth: Option<i32>,
    reply_timeout: f64,
    timeout: f64,
    label: Option<String>,
    json: bool,
    participants: i32,
    start: f64,
}

fn usage(why: &str) -> ! {
    eprintln!("{why}\nusage: rustdds-perf ping|pong|pub|sub|disc [options] (see bench/rustdds/src/main.rs)");
    std::process::exit(2);
}

fn parse() -> Options {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = match args.first().map(String::as_str) {
        Some("ping") => Mode::Ping,
        Some("pong") => Mode::Pong,
        Some("pub") => Mode::Pub,
        Some("sub") => Mode::Sub,
        Some("disc") => Mode::Disc,
        Some(_) => usage("unknown mode"),
        None => usage("no mode"),
    };
    let mut o = Options {
        mode,
        domain: 0,
        sizes: Vec::new(),
        reliable: true,
        transport: "udp".into(),
        warmup: 1.0,
        duration: 5.0,
        count: None,
        rate: 0.0,
        depth: None,
        reply_timeout: 1.0,
        timeout: 60.0,
        label: None,
        json: false,
        participants: 2,
        start: 0.0,
    };
    let mut i = 1;
    while i < args.len() {
        let a = args[i].as_str();
        if a == "--json" {
            o.json = true;
            i += 1;
            continue;
        }
        let v = args
            .get(i + 1)
            .unwrap_or_else(|| usage("an option needs a value"))
            .as_str();
        i += 2;
        fn num<T: std::str::FromStr>(v: &str) -> T {
            v.parse().unwrap_or_else(|_| usage("a bad number"))
        }
        match a {
            "--domain" => o.domain = num(v),
            "--size" => o.sizes = vec![num(v)],
            "--sizes" => o.sizes = v.split(',').map(num).collect(),
            "--reliability" => o.reliable = v == "reliable",
            "--transport" => o.transport = v.into(),
            "--warmup" => o.warmup = num(v),
            "--duration" => o.duration = num(v),
            "--count" => o.count = Some(num(v)),
            "--rate" => o.rate = num(v),
            "--depth" => o.depth = Some(num(v)),
            "--reply-timeout" => o.reply_timeout = num::<f64>(v) / 1000.0,
            "--timeout" => o.timeout = num(v),
            "--label" => o.label = Some(v.into()),
            "--participants" => o.participants = num(v),
            "--start" => o.start = num(v),
            _ => usage("unknown option"),
        }
    }
    if o.sizes.is_empty() {
        o.sizes = if matches!(o.mode, Mode::Ping | Mode::Pong) {
            (5..=20).map(|e| 1usize << e).collect()
        } else {
            vec![1024]
        };
    }
    o
}

fn wall_s() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

fn secs(s: f64) -> Duration {
    Duration::from_secs_f64(s.max(0.0))
}

fn die(what: &str, e: impl std::fmt::Debug) -> ! {
    eprintln!("{what}: {e:?}");
    std::process::exit(1);
}

/// A minimal JSON object writer, keys in order.
struct Json(String);

impl Json {
    fn head(o: &Options, size: usize) -> Json {
        let mut j = Json(String::from("{"));
        j.s("tool", "rustorb-dds-perf");
        j.s("impl", "rustdds");
        j.s("version", VERSION);
        j.s("mode", o.mode.name());
        j.u("size", size as u64);
        j.s(
            "reliability",
            if o.reliable {
                "reliable"
            } else {
                "best-effort"
            },
        );
        j.s("transport", &o.transport);
        if let Some(l) = &o.label {
            j.s("label", l);
        }
        j
    }
    fn key(&mut self, k: &str) {
        if self.0.len() > 1 {
            self.0.push(',');
        }
        self.0.push_str(&format!("\"{k}\":"));
    }
    fn s(&mut self, k: &str, v: &str) {
        self.key(k);
        self.0.push('"');
        for c in v.chars() {
            match c {
                '"' => self.0.push_str("\\\""),
                '\\' => self.0.push_str("\\\\"),
                c if (c as u32) < 0x20 => self.0.push_str(&format!("\\u{:04x}", c as u32)),
                c => self.0.push(c),
            }
        }
        self.0.push('"');
    }
    fn u(&mut self, k: &str, v: u64) {
        self.key(k);
        self.0.push_str(&v.to_string());
    }
    fn f(&mut self, k: &str, v: f64) {
        self.key(k);
        self.0.push_str(&format!("{v:.3}"));
    }
    fn f6(&mut self, k: &str, v: f64) {
        self.key(k);
        self.0.push_str(&format!("{v:.6}"));
    }
    fn print(mut self) {
        self.0.push('}');
        println!("{}", self.0);
    }
}

struct Perf {
    _participant: DomainParticipant,
    writer: Option<Writer>,
    reader: Option<Reader>,
    poll: Poll,
    events: Events,
    /// The current matched counts, from the status events.
    writer_matched: i32,
    reader_matched: i32,
}

const DATA: Token = Token(0);

impl Perf {
    fn new(o: &Options) -> Perf {
        let throughput = matches!(o.mode, Mode::Pub | Mode::Sub);
        let depth = o
            .depth
            .filter(|d| *d > 0)
            .unwrap_or(if throughput { 100 } else { 1 });
        let (w, r) = match o.mode {
            Mode::Ping => (Some("RustorbPerf_ping"), Some("RustorbPerf_pong")),
            Mode::Pong => (Some("RustorbPerf_pong"), Some("RustorbPerf_ping")),
            Mode::Pub => (Some("RustorbPerf_data"), None),
            Mode::Sub => (None, Some("RustorbPerf_data")),
            Mode::Disc => (Some("RustorbPerf_disc"), Some("RustorbPerf_disc")),
        };
        let participant =
            DomainParticipant::new(o.domain).unwrap_or_else(|e| die("DomainParticipant::new", e));
        let reliability = if o.reliable {
            Reliability::Reliable {
                max_blocking_time: rustdds::Duration::from_millis(100),
            }
        } else {
            Reliability::BestEffort
        };
        let base = || QosPolicyBuilder::new().reliability(reliability);
        // Flow control: a write blocks once depth samples are unacknowledged.
        let wq = if throughput && o.reliable {
            base()
                .history(History::KeepAll)
                .resource_limits(ResourceLimits {
                    max_samples: depth,
                    max_instances: -1,
                    max_samples_per_instance: -1,
                })
        } else {
            base().history(History::KeepLast { depth })
        }
        .build();
        let rq = if throughput && o.reliable {
            base().history(History::KeepAll)
        } else if throughput {
            // A best-effort sub slower than its pub drops what does not
            // fit. The others have KEEP_ALL with at most
            // READER_MAX_SAMPLES samples; RustDDS 0.15 delivers nothing
            // to a reader with RESOURCE_LIMITS, so KEEP_LAST of as many
            // (the oldest are dropped instead of the newest).
            base().history(History::KeepLast {
                depth: READER_MAX_SAMPLES,
            })
        } else {
            base().history(History::KeepLast { depth })
        }
        .build();
        let topic = |name: &str, qos| {
            participant
                .create_topic(name.into(), TYPE_NAME.into(), qos, TopicKind::NoKey)
                .unwrap_or_else(|e| die("create_topic", e))
        };
        let writer = w.map(|name| {
            let t = topic(name, &wq);
            participant
                .create_publisher(&wq)
                .unwrap_or_else(|e| die("create_publisher", e))
                .create_datawriter_no_key::<Sample, CDRSerializerAdapter<Sample>>(
                    &t,
                    Some(wq.clone()),
                )
                .unwrap_or_else(|e| die("create_datawriter", e))
        });
        let mut reader = r.map(|name| {
            let t = topic(name, &rq);
            participant
                .create_subscriber(&rq)
                .unwrap_or_else(|e| die("create_subscriber", e))
                .create_datareader_no_key::<Sample, CDRDeserializerAdapter<Sample>>(
                    &t,
                    Some(rq.clone()),
                )
                .unwrap_or_else(|e| die("create_datareader", e))
        });
        let poll = Poll::new().unwrap_or_else(|e| die("Poll::new", e));
        if let Some(r) = reader.as_mut() {
            poll.registry()
                .register(r, DATA, Interest::READABLE)
                .unwrap_or_else(|e| die("register", e));
        }
        Perf {
            _participant: participant,
            writer,
            reader,
            poll,
            events: Events::with_capacity(16),
            writer_matched: 0,
            reader_matched: 0,
        }
    }

    /// Reads the status events into the matched counts.
    fn statuses(&mut self) {
        if let Some(w) = &self.writer {
            while let Some(s) = w.try_recv_status() {
                if let DataWriterStatus::PublicationMatched { current, .. } = s {
                    self.writer_matched = current.count();
                }
            }
        }
        if let Some(r) = &self.reader {
            while let Some(s) = r.try_recv_status() {
                if let DataReaderStatus::SubscriptionMatched { current, .. } = s {
                    self.reader_matched = current.count();
                }
            }
        }
    }

    fn matched(&mut self) -> (i32, i32) {
        self.statuses();
        (self.writer_matched, self.reader_matched)
    }

    fn wait_for_match(&mut self, o: &Options) {
        let deadline = Instant::now() + secs(o.timeout);
        loop {
            let (w, r) = self.matched();
            if (self.writer.is_none() || w > 0) && (self.reader.is_none() || r > 0) {
                std::thread::sleep(Duration::from_millis(300));
                return;
            }
            if Instant::now() > deadline {
                eprintln!("{}: nothing matched in {:.0} s", o.mode.name(), o.timeout);
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Waits up to `d` for the reader to say there is data.
    fn wait_data(&mut self, d: Duration) -> bool {
        if d.is_zero() {
            return false;
        }
        match self.poll.poll(&mut self.events, Some(d)) {
            Ok(()) => !self.events.is_empty(),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => false,
            Err(e) => die("poll", e),
        }
    }

    /// Takes up to MAX_TAKE samples.
    fn take(&mut self, out: &mut Vec<Sample>) {
        out.clear();
        let r = self.reader.as_mut().expect("a reader");
        while out.len() < MAX_TAKE {
            match r.take_next_sample() {
                Ok(Some(s)) => out.push(s.into_value()),
                Ok(None) => break,
                Err(e) => die("take", e),
            }
        }
    }

    /// Writes; false when it blocked to its max_blocking_time.
    fn write(&self, s: Sample) -> bool {
        match self.writer.as_ref().expect("a writer").write(s, None) {
            Ok(()) => true,
            Err(WriteError::WouldBlock { .. }) => false,
            Err(e) => die("write", e),
        }
    }
}

fn sample(seq: u64, stamp: u64, payload: &ByteBuf) -> Sample {
    Sample {
        seq,
        stamp,
        payload: payload.clone(),
    }
}

/// The nearest-rank percentile of sorted v.
fn pct(v: &[f64], p: f64) -> f64 {
    if v.is_empty() {
        return 0.0;
    }
    let k = ((p * v.len() as f64 - 1e-9).ceil() as usize).clamp(1, v.len());
    v[k - 1]
}

fn ping(o: &Options, p: &mut Perf) {
    p.wait_for_match(o);
    let start = Instant::now();
    let mut seq = 0u64;
    let mut got = Vec::new();
    if !o.json {
        println!(
            "{:>8} {:>8} {:>6} {:>9} {:>9} {:>9} {:>9} {:>9}  (round trip, us)",
            "size", "count", "lost", "p50", "p90", "p99", "p99.9", "max"
        );
    }
    for &size in &o.sizes {
        let payload = ByteBuf::from(vec![0x5a; size]);
        let warm_end = Instant::now() + secs(o.warmup);
        let end = warm_end + secs(o.duration);
        let mut rtt: Vec<f64> = Vec::with_capacity(1 << 16);
        let mut lost = 0u64;
        p.take(&mut got);
        loop {
            let t = Instant::now();
            let measuring = t >= warm_end;
            if t >= end || (measuring && o.count.is_some_and(|c| rtt.len() as u64 >= c)) {
                break;
            }
            seq += 1;
            let sent = Instant::now();
            let stamp = sent.duration_since(start).as_nanos() as u64;
            if !p.write(sample(seq, stamp, &payload)) {
                if measuring {
                    lost += 1;
                }
                continue;
            }
            let deadline = sent + secs(o.reply_timeout);
            loop {
                // What came before the wait-up is taken first (the reader's
                // notification may have been used by the take before).
                p.take(&mut got);
                if got.iter().any(|s| s.seq == seq) {
                    if measuring {
                        rtt.push(sent.elapsed().as_secs_f64() * 1e6);
                    }
                    break;
                }
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() {
                    if measuring {
                        lost += 1;
                    }
                    break;
                }
                p.wait_data(left);
            }
        }
        let n = rtt.len();
        let mean = if n > 0 {
            rtt.iter().sum::<f64>() / n as f64
        } else {
            0.0
        };
        rtt.sort_by(f64::total_cmp);
        let (mn, mx) = (
            rtt.first().copied().unwrap_or(0.0),
            rtt.last().copied().unwrap_or(0.0),
        );
        if o.json {
            let mut j = Json::head(o, size);
            j.u("count", n as u64);
            j.u("lost", lost);
            j.f("seconds", o.duration);
            j.f("rtt_min_us", mn);
            j.f("rtt_mean_us", mean);
            j.f("rtt_p50_us", pct(&rtt, 0.5));
            j.f("rtt_p90_us", pct(&rtt, 0.9));
            j.f("rtt_p99_us", pct(&rtt, 0.99));
            j.f("rtt_p999_us", pct(&rtt, 0.999));
            j.f("rtt_max_us", mx);
            j.print();
        } else {
            println!(
                "{size:>8} {n:>8} {lost:>6} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {mx:>9.1}",
                pct(&rtt, 0.5),
                pct(&rtt, 0.9),
                pct(&rtt, 0.99),
                pct(&rtt, 0.999)
            );
        }
    }
}

fn pong(o: &Options, p: &mut Perf) {
    let deadline = Instant::now() + secs(o.timeout);
    let mut matched = false;
    let mut echoed = 0u64;
    let mut got = Vec::new();
    loop {
        p.take(&mut got);
        let n = got.len();
        for s in got.drain(..) {
            p.write(s);
            echoed += 1;
        }
        if n == 0 {
            p.wait_data(Duration::from_millis(200));
        }
        let (w, r) = p.matched();
        if matched {
            if w == 0 && r == 0 {
                break;
            }
        } else if w > 0 || r > 0 {
            matched = true;
        } else if Instant::now() > deadline {
            eprintln!("pong: no ping in {:.0} s", o.timeout);
            std::process::exit(1);
        }
    }
    if !o.json {
        println!("pong: sent back {echoed} samples");
    }
}

fn publish(o: &Options, p: &mut Perf) {
    p.wait_for_match(o);
    let size = o.sizes[0];
    let payload = ByteBuf::from(vec![0x5a; size]);
    let start = Instant::now();
    let give_up = start + secs(o.warmup + o.duration + o.timeout);
    let (mut seq, mut blocked) = (0u64, 0u64);
    loop {
        if seq % 256 == 0 && (p.matched().0 == 0 || Instant::now() >= give_up) {
            break;
        }
        let stamp = start.elapsed().as_nanos() as u64;
        if p.write(sample(seq + 1, stamp, &payload)) {
            seq += 1;
        } else {
            blocked += 1;
            if p.matched().0 == 0 {
                break;
            }
        }
        if o.rate > 0.0 {
            let at = start + secs(seq as f64 / o.rate);
            std::thread::sleep(at.saturating_duration_since(Instant::now()));
        }
    }
    let secs = start.elapsed().as_secs_f64();
    if o.json {
        let mut j = Json::head(o, size);
        j.u("samples", seq);
        j.f("seconds", secs);
        j.f("samples_per_s", seq as f64 / secs);
        j.u("blocked", blocked);
        j.print();
    } else {
        println!(
            "pub: wrote {seq} samples of {size} octets in {secs:.1} s ({:.0}/s), blocked {blocked} times",
            seq as f64 / secs
        );
    }
}

fn subscribe(o: &Options, p: &mut Perf) {
    p.wait_for_match(o);
    let size = o.sizes[0];
    let mut got = Vec::new();
    let first_deadline = Instant::now() + secs(o.timeout);
    // The first sample (taken, not counted: it is in the warm-up).
    loop {
        p.take(&mut got);
        if !got.is_empty() {
            break;
        }
        let left = first_deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            eprintln!("sub: no sample in {:.0} s", o.timeout);
            std::process::exit(1);
        }
        p.wait_data(left);
    }
    let warm_end = Instant::now() + secs(o.warmup);
    let end = warm_end + secs(o.duration);
    let mut second_end = warm_end + Duration::from_secs(1);
    let (mut samples, mut octets, mut in_second) = (0u64, 0u64, 0u64);
    let (mut first, mut last) = (None::<u64>, 0u64);
    let (mut min_s, mut max_s, mut seconds) = (u64::MAX, 0u64, 0u32);
    loop {
        let t = Instant::now();
        if t >= end {
            break;
        }
        if t >= second_end && t > warm_end {
            min_s = min_s.min(in_second);
            max_s = max_s.max(in_second);
            seconds += 1;
            in_second = 0;
            second_end += Duration::from_secs(1);
        }
        loop {
            p.take(&mut got);
            let counting = Instant::now() >= warm_end;
            if counting {
                for s in &got {
                    samples += 1;
                    in_second += 1;
                    octets += s.payload.len() as u64;
                    first.get_or_insert(s.seq);
                    last = last.max(s.seq);
                }
            }
            // Back to the outer loop for the end and the next second, also
            // while samples keep coming faster than they are taken.
            if got.len() < MAX_TAKE || Instant::now() >= second_end.min(end) {
                break;
            }
        }
        p.wait_data(
            end.saturating_duration_since(Instant::now())
                .min(Duration::from_millis(50)),
        );
    }
    if seconds == 0 {
        min_s = 0;
    }
    let lost = first.map_or(0, |f| (last - f + 1).saturating_sub(samples));
    let secs = o.duration;
    let mbit = octets as f64 * 8.0 / secs / 1e6;
    if o.json {
        let mut j = Json::head(o, size);
        j.u("samples", samples);
        j.u("octets", octets);
        j.u("lost", lost);
        j.f("seconds", secs);
        j.f("samples_per_s", samples as f64 / secs);
        j.f("mbit_per_s", mbit);
        j.u("samples_per_s_min", min_s);
        j.u("samples_per_s_max", max_s);
        j.print();
    } else {
        println!(
            "sub: {samples} samples of {size} octets in {secs:.1} s: {:.0}/s, {mbit:.1} Mbit/s, {lost} lost",
            samples as f64 / secs
        );
    }
}

/// disc: the seconds from the common start until the participant is
/// created and until its writer and reader match those of all the
/// participants (its own included); stays to --duration for the others.
fn discovery(o: &Options) {
    let start = if o.start > 0.0 { o.start } else { wall_s() };
    let mut p = Perf::new(o);
    let created = wall_s() - start;
    let mut matched = None;
    while matched.is_none() && wall_s() < start + o.duration {
        let (w, r) = p.matched();
        if w >= o.participants && r >= o.participants {
            matched = Some(wall_s() - start);
        } else {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
    std::thread::sleep(secs(start + o.duration - wall_s()));
    let (w, r) = p.matched();
    if o.json {
        let mut j = Json(String::from("{"));
        j.s("tool", "rustorb-dds-perf");
        j.s("impl", "rustdds");
        j.s("version", VERSION);
        j.s("mode", "disc");
        j.s("transport", &o.transport);
        j.u("participants", o.participants as u64);
        j.f6("create_s", created);
        j.u("writer_matched", w as u64);
        j.u("reader_matched", r as u64);
        if let Some(m) = matched {
            j.f6("match_s", m);
        }
        if let Some(l) = &o.label {
            j.s("label", l);
        }
        j.print();
    } else if let Some(m) = matched {
        println!("disc: participant at {created:.3} s, all matched at {m:.3} s");
    } else {
        println!(
            "disc: matched {w} writers and {r} readers of {}",
            o.participants
        );
    }
}

fn main() -> ExitCode {
    // RustDDS logs with `log`: RUST_LOG=rustdds=warn shows them.
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("off")).init();
    let o = parse();
    if o.transport != "udp" {
        usage("RustDDS has --transport udp only");
    }
    if o.mode == Mode::Disc {
        discovery(&o);
        return ExitCode::SUCCESS;
    }
    let mut p = Perf::new(&o);
    match o.mode {
        Mode::Ping => ping(&o, &mut p),
        Mode::Pong => pong(&o, &mut p),
        Mode::Pub => publish(&o, &mut p),
        Mode::Sub => subscribe(&o, &mut p),
        Mode::Disc => {}
    }
    ExitCode::SUCCESS
}
