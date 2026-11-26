//! `rustorb-dds-perf`: latency and throughput of rustorb-dds, in the
//! manner of Cyclone DDS's `ddsperf` (rustorb-dds#101).
//!
//! ```text
//! rustorb-dds-perf ping [options]   # round trips with a pong, for each size
//! rustorb-dds-perf pong [options]   # sends back what a ping sends
//! rustorb-dds-perf pub  [options]   # writes as fast as it can (or --rate)
//! rustorb-dds-perf sub  [options]   # counts what it receives
//! ```
//!
//! See `--help` and README.md. With `--json` each result is one line of
//! JSON on stdout, which `bench/run.py` reads.

mod stats;
mod types;

use std::process::ExitCode;
use std::time::{Duration, Instant};

use rustorb_dds::{
    DataReader, DataReaderQos, DataWriter, DataWriterQos, DomainParticipant, Error, History,
    ParticipantConfig, Reliability, ResourceLimits, StatusCondition, StatusMask, WaitSet,
};
use stats::Summary;
use types::{Fixed, FixedVisitor, Sample, Stamped};

const USAGE: &str = "\
usage: rustorb-dds-perf <ping|pong|pub|sub> [options]

  ping   measures round trips with a pong, size after size
  pong   sends back every sample a ping sends, until the ping leaves
  pub    writes samples of one size as fast as it can, or at --rate
  sub    counts the samples and octets it receives for --duration

options:
  --domain N          DDS domain (0)
  --size N            payload octets (pub, sub; 1024)
  --sizes A,B,...     payload octets, one after the other (ping, pong;
                      32,64,...,1048576)
  --reliability R     reliable or best-effort (reliable)
  --transport T       shm (shared memory to rustorb-dds participants of
                      the host, UDP to others), udp (UDP only), or loan
                      (loaned samples on shared memory; sizes must be
                      powers of two from 32 to 1048576) (shm)
  --localhost         only the loopback interface
  --warmup S          seconds of each size left out (1)
  --duration S        seconds measured for each size (5)
  --count N           ping: at most N round trips for each size (no limit)
  --rate N            pub: samples per second (0, as fast as it can)
  --depth N           history depth: KEEP_LAST(N), or for a reliable pub
                      KEEP_ALL with at most N samples (ping 1, pub 100)
  --reply-timeout MS  ping: a round trip is lost after MS ms (1000)
  --timeout S         seconds to wait for the other side (60)
  --label TEXT        put into the JSON as \"label\"
  --json              one line of JSON for each result on stdout
  --security FILE     DDS Security, the dds.sec.* properties in FILE, one
                      name=value per line (feature `security`)
";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Ping,
    Pong,
    Pub,
    Sub,
}

impl Mode {
    fn name(self) -> &'static str {
        match self {
            Mode::Ping => "ping",
            Mode::Pong => "pong",
            Mode::Pub => "pub",
            Mode::Sub => "sub",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Transport {
    Shm,
    Udp,
    Loan,
}

impl Transport {
    fn name(self) -> &'static str {
        match self {
            Transport::Shm => "shm",
            Transport::Udp => "udp",
            Transport::Loan => "loan",
        }
    }
}

#[derive(Debug, Clone)]
struct Opts {
    mode: Mode,
    domain: u32,
    sizes: Vec<usize>,
    reliable: bool,
    transport: Transport,
    localhost: bool,
    warmup: Duration,
    duration: Duration,
    count: Option<u64>,
    rate: f64,
    depth: Option<usize>,
    reply_timeout: Duration,
    timeout: Duration,
    label: Option<String>,
    json: bool,
    security: Option<std::path::PathBuf>,
}

type BoxError = Box<dyn std::error::Error>;

fn parse(args: &[String]) -> Result<Opts, BoxError> {
    let mode = match args.first().map(String::as_str) {
        Some("ping") => Mode::Ping,
        Some("pong") => Mode::Pong,
        Some("pub") => Mode::Pub,
        Some("sub") => Mode::Sub,
        Some("-h" | "--help") | None => return Err(USAGE.into()),
        Some(other) => return Err(format!("unknown mode {other:?}\n\n{USAGE}").into()),
    };
    let mut o = Opts {
        mode,
        domain: 0,
        sizes: Vec::new(),
        reliable: true,
        transport: Transport::Shm,
        localhost: false,
        warmup: Duration::from_secs(1),
        duration: Duration::from_secs(5),
        count: None,
        rate: 0.0,
        depth: None,
        reply_timeout: Duration::from_secs(1),
        timeout: Duration::from_secs(60),
        label: None,
        json: false,
        security: None,
    };
    let mut it = args[1..].iter();
    let secs = |v: &str| -> Result<Duration, BoxError> { Ok(Duration::from_secs_f64(v.parse()?)) };
    while let Some(a) = it.next() {
        let mut value = || {
            it.next()
                .map(String::as_str)
                .ok_or_else(|| format!("{a} needs a value"))
        };
        match a.as_str() {
            "--domain" => o.domain = value()?.parse()?,
            "--size" => o.sizes = vec![value()?.parse()?],
            "--sizes" => {
                o.sizes = value()?
                    .split(',')
                    .map(|s| s.trim().parse())
                    .collect::<Result<_, _>>()?
            }
            "--reliability" => {
                o.reliable = match value()? {
                    "reliable" => true,
                    "best-effort" | "best_effort" => false,
                    v => return Err(format!("unknown reliability {v:?}").into()),
                }
            }
            "--transport" => {
                o.transport = match value()? {
                    "shm" => Transport::Shm,
                    "udp" => Transport::Udp,
                    "loan" => Transport::Loan,
                    v => return Err(format!("unknown transport {v:?}").into()),
                }
            }
            "--localhost" => o.localhost = true,
            "--warmup" => o.warmup = secs(value()?)?,
            "--duration" => o.duration = secs(value()?)?,
            "--count" => o.count = Some(value()?.parse()?),
            "--rate" => o.rate = value()?.parse()?,
            "--depth" => o.depth = Some(value()?.parse()?),
            "--reply-timeout" => o.reply_timeout = Duration::from_millis(value()?.parse()?),
            "--timeout" => o.timeout = secs(value()?)?,
            "--label" => o.label = Some(value()?.to_string()),
            "--json" => o.json = true,
            "--security" => o.security = Some(value()?.into()),
            "-h" | "--help" => return Err(USAGE.into()),
            other => return Err(format!("unknown option {other:?}\n\n{USAGE}").into()),
        }
    }
    if o.sizes.is_empty() {
        o.sizes = match mode {
            Mode::Ping | Mode::Pong => (5..=20).map(|e| 1usize << e).collect(),
            Mode::Pub | Mode::Sub => vec![1024],
        };
    }
    if matches!(mode, Mode::Pub | Mode::Sub) && o.sizes.len() != 1 {
        return Err("pub and sub take one --size".into());
    }
    if o.transport == Transport::Loan
        && let Some(s) = o.sizes.iter().find(|s| !types::FIXED_SIZES.contains(s))
    {
        return Err(format!(
            "--transport loan: no loanable type of {s} octets (powers of two, 32 to 1048576)"
        )
        .into());
    }
    Ok(o)
}

/// What was received: a sample's number, stamp and payload size.
#[derive(Debug, Clone, Copy)]
struct Got {
    seq: u64,
    stamp: u64,
    len: usize,
}

/// A writer and a reader (either may be absent) of one sample type.
trait Endpoint {
    /// Writes a sample of `len` payload octets.
    fn send(&mut self, seq: u64, stamp: u64, len: usize) -> rustorb_dds::Result<()>;
    /// Takes what has come.
    fn recv(&mut self, out: &mut Vec<Got>);
    /// Waits until something has come.
    fn wait(&self, timeout: Duration) -> bool;
    /// The reader's condition for DATA_AVAILABLE.
    fn condition(&self) -> Option<StatusCondition>;
    /// How many remote endpoints match the writer and the reader.
    fn matched(&self) -> (u32, u32);
}

/// The most samples taken at once (as `MAX_TAKE` of
/// bench/cyclonedds/perf.c).
const MAX_TAKE: usize = 256;

/// The samples a best-effort sub keeps not yet taken (`max_samples`
/// of its KEEP_ALL history), as in the other tools.
const READER_MAX_SAMPLES: usize = 1000;

struct Endpoints<W, R> {
    writer: Option<DataWriter<W>>,
    reader: Option<DataReader<R>>,
}

impl<W, R> Endpoints<W, R> {
    fn wait_reader(&self, timeout: Duration) -> bool {
        self.reader
            .as_ref()
            .is_some_and(|r| r.wait_for_data(timeout))
    }
    fn reader_condition(&self) -> Option<StatusCondition> {
        self.reader.as_ref().map(|r| {
            let c = r.status_condition();
            c.set_enabled_statuses(StatusMask::DATA_AVAILABLE);
            c
        })
    }
    fn matched_counts(&self) -> (u32, u32) {
        (
            self.writer
                .as_ref()
                .map_or(0, |w| w.publication_matched_status().current_count),
            self.reader
                .as_ref()
                .map_or(0, |r| r.subscription_matched_status().current_count),
        )
    }
}

/// `RustorbPerf::Sample`, serialized.
struct Plain {
    ep: Endpoints<Sample, Sample>,
    sample: Sample,
}

impl Endpoint for Plain {
    fn send(&mut self, seq: u64, stamp: u64, len: usize) -> rustorb_dds::Result<()> {
        self.sample.seq = seq;
        self.sample.stamp = stamp;
        // Resized only when the size changes.
        self.sample.payload.resize(len, 0x5a);
        self.ep
            .writer
            .as_ref()
            .expect("a writer")
            .write(&self.sample)
    }
    fn recv(&mut self, out: &mut Vec<Got>) {
        if let Some(r) = &self.ep.reader {
            // MAX_TAKE at a time: a reader that fell behind is not
            // decoded all at once, and the caller counts the seconds
            // between takes.
            let batch = r.max_samples(MAX_TAKE).take();
            out.extend(batch.into_iter().filter_map(|s| s.data).map(|d| Got {
                seq: d.seq(),
                stamp: d.stamp(),
                len: d.payload_len(),
            }));
        }
    }
    fn wait(&self, timeout: Duration) -> bool {
        self.ep.wait_reader(timeout)
    }
    fn condition(&self) -> Option<StatusCondition> {
        self.ep.reader_condition()
    }
    fn matched(&self) -> (u32, u32) {
        self.ep.matched_counts()
    }
}

/// A `RustorbPerf::FixedN`, loaned.
struct Loaned<T: Fixed> {
    ep: Endpoints<T, T>,
}

impl<T: Fixed> Endpoint for Loaned<T> {
    fn send(&mut self, seq: u64, stamp: u64, _len: usize) -> rustorb_dds::Result<()> {
        let w = self.ep.writer.as_ref().expect("a writer");
        let mut s = w.loan_sample()?;
        // The data is left as it is: what the slot had, or zeros.
        s.set(seq, stamp);
        w.write_loaned(s)
    }
    fn recv(&mut self, out: &mut Vec<Got>) {
        if let Some(r) = &self.ep.reader {
            let batch = r.take_loaned_max(MAX_TAKE);
            out.extend(batch.into_iter().filter_map(|s| s.data).map(|d| Got {
                seq: d.seq(),
                stamp: d.stamp(),
                len: d.payload_len(),
            }));
        }
    }
    fn wait(&self, timeout: Duration) -> bool {
        self.ep.wait_reader(timeout)
    }
    fn condition(&self) -> Option<StatusCondition> {
        self.ep.reader_condition()
    }
    fn matched(&self) -> (u32, u32) {
        self.ep.matched_counts()
    }
}

/// The topics and QoS of an endpoint.
struct Spec<'a> {
    participant: &'a DomainParticipant,
    write: Option<&'a str>,
    read: Option<&'a str>,
    writer_qos: DataWriterQos,
    reader_qos: DataReaderQos,
}

impl Spec<'_> {
    fn make<W, R>(&self) -> rustorb_dds::Result<Endpoints<W, R>>
    where
        W: rustorb_dds::TopicType + rustorb_dds::DescribeType + Send + Sync + 'static,
        R: rustorb_dds::TopicType + rustorb_dds::DescribeType + Send + Sync + 'static,
    {
        let writer = match self.write {
            Some(name) => {
                self.participant.register_type::<W>()?;
                let t = self.participant.create_topic::<W>(name)?;
                Some(
                    self.participant
                        .create_publisher()?
                        .create_datawriter_with_qos(&t, self.writer_qos.clone())?,
                )
            }
            None => None,
        };
        let reader = match self.read {
            Some(name) => {
                self.participant.register_type::<R>()?;
                let t = self.participant.create_topic::<R>(name)?;
                Some(
                    self.participant
                        .create_subscriber()?
                        .create_datareader_with_qos(&t, self.reader_qos.clone())?,
                )
            }
            None => None,
        };
        Ok(Endpoints { writer, reader })
    }
}

struct MakeLoaned<'a, 'b>(&'a Spec<'b>);

impl FixedVisitor for MakeLoaned<'_, '_> {
    type Out = rustorb_dds::Result<Box<dyn Endpoint>>;
    fn visit<T: Fixed>(self) -> Self::Out {
        Ok(Box::new(Loaned::<T> {
            ep: self.0.make::<T, T>()?,
        }))
    }
}

/// The endpoints: one for every size with loans, else one for all.
fn endpoints(
    o: &Opts,
    participant: &DomainParticipant,
) -> Result<Vec<Box<dyn Endpoint>>, BoxError> {
    let (write, read) = match o.mode {
        Mode::Ping => (Some("RustorbPerf_ping"), Some("RustorbPerf_pong")),
        Mode::Pong => (Some("RustorbPerf_pong"), Some("RustorbPerf_ping")),
        Mode::Pub => (Some("RustorbPerf_data"), None),
        Mode::Sub => (None, Some("RustorbPerf_data")),
    };
    let reliability = if o.reliable {
        Reliability::reliable()
    } else {
        Reliability::best_effort()
    };
    let throughput = matches!(o.mode, Mode::Pub | Mode::Sub);
    let depth = o.depth.unwrap_or(if throughput { 100 } else { 1 });
    let writer_qos = if throughput && o.reliable {
        // Flow control: write blocks once `depth` samples are not yet
        // acknowledged.
        DataWriterQos {
            reliability,
            history: History::KeepAll,
            resource_limits: ResourceLimits {
                max_samples: Some(depth),
                ..ResourceLimits::default()
            },
            ..DataWriterQos::default()
        }
    } else {
        DataWriterQos {
            reliability,
            history: History::KeepLast(depth),
            ..DataWriterQos::default()
        }
    };
    let reader_qos = DataReaderQos {
        reliability,
        history: if throughput {
            History::KeepAll
        } else {
            History::KeepLast(depth)
        },
        // A best-effort sub slower than its pub drops what does not fit,
        // as a lost datagram, instead of keeping all (#110).
        resource_limits: if throughput && !o.reliable {
            ResourceLimits {
                max_samples: Some(READER_MAX_SAMPLES),
                ..ResourceLimits::default()
            }
        } else {
            ResourceLimits::default()
        },
        ..DataReaderQos::default()
    };
    if o.transport != Transport::Loan {
        let spec = Spec {
            participant,
            write,
            read,
            writer_qos,
            reader_qos,
        };
        return Ok(vec![Box::new(Plain {
            ep: spec.make()?,
            sample: Sample::default(),
        })]);
    }
    let mut out = Vec::new();
    for &size in &o.sizes {
        let (w, r) = (
            write.map(|n| format!("{n}_{size}")),
            read.map(|n| format!("{n}_{size}")),
        );
        let spec = Spec {
            participant,
            write: w.as_deref(),
            read: r.as_deref(),
            writer_qos: writer_qos.clone(),
            reader_qos: reader_qos.clone(),
        };
        out.push(types::with_fixed(size, MakeLoaned(&spec)).expect("a checked size")?);
    }
    Ok(out)
}

fn participant(o: &Opts) -> Result<DomainParticipant, BoxError> {
    let mut config = if o.localhost {
        ParticipantConfig::localhost(o.domain)
    } else {
        ParticipantConfig::new(o.domain)
    };
    config.shared_memory = o.transport != Transport::Udp;
    if let Some(path) = &o.security {
        #[cfg(feature = "security")]
        {
            let text = std::fs::read_to_string(path)?;
            let props = text
                .lines()
                .filter_map(|l| l.split_once('='))
                .map(|(k, v)| (k.trim(), v.trim()));
            config.security = Some(
                rustorb_dds::SecurityConfig::from_properties(props)
                    .ok_or("--security: a dds.sec property is missing")?,
            );
        }
        #[cfg(not(feature = "security"))]
        return Err(format!(
            "--security {}: built without the feature `security`",
            path.display()
        )
        .into());
    }
    Ok(DomainParticipant::with_config(config)?)
}

/// A value in the JSON output.
enum J<'a> {
    S(&'a str),
    U(u64),
    F(f64),
}

fn json(fields: &[(&str, J<'_>)]) -> String {
    let mut s = String::from("{");
    for (i, (k, v)) in fields.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&format!("\"{k}\":"));
        match v {
            J::S(t) => {
                s.push('"');
                for c in t.chars() {
                    match c {
                        '"' => s.push_str("\\\""),
                        '\\' => s.push_str("\\\\"),
                        c if (c as u32) < 0x20 => s.push_str(&format!("\\u{:04x}", c as u32)),
                        c => s.push(c),
                    }
                }
                s.push('"');
            }
            J::U(n) => s.push_str(&n.to_string()),
            J::F(f) if f.is_finite() => s.push_str(&format!("{f:.3}")),
            J::F(_) => s.push_str("null"),
        }
    }
    s.push('}');
    s
}

impl Opts {
    fn common(&self, size: usize) -> Vec<(&'static str, J<'_>)> {
        let mut v = vec![
            ("tool", J::S("rustorb-dds-perf")),
            ("impl", J::S("rustorb-dds")),
            ("version", J::S(env!("CARGO_PKG_VERSION"))),
            ("mode", J::S(self.mode.name())),
            ("size", J::U(size as u64)),
            (
                "reliability",
                J::S(if self.reliable {
                    "reliable"
                } else {
                    "best-effort"
                }),
            ),
            ("transport", J::S(self.transport.name())),
        ];
        if let Some(l) = &self.label {
            v.push(("label", J::S(l)));
        }
        v
    }
}

/// Waits until every endpoint's writer and reader matches something.
fn wait_for_match(eps: &[Box<dyn Endpoint>], o: &Opts) -> Result<(), BoxError> {
    let deadline = Instant::now() + o.timeout;
    let (w, r) = match o.mode {
        Mode::Ping | Mode::Pong => (true, true),
        Mode::Pub => (true, false),
        Mode::Sub => (false, true),
    };
    loop {
        let ok = eps.iter().all(|e| {
            let (mw, mr) = e.matched();
            (!w || mw > 0) && (!r || mr > 0)
        });
        if ok {
            // For the other side to match too.
            std::thread::sleep(Duration::from_millis(300));
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("{}: nothing matched in {:?}", o.mode.name(), o.timeout).into());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn ping(o: &Opts, eps: &mut [Box<dyn Endpoint>]) -> Result<(), BoxError> {
    wait_for_match(eps, o)?;
    let start = Instant::now();
    let mut seq = 0u64;
    let mut got = Vec::new();
    if !o.json {
        println!(
            "{:>8} {:>8} {:>6} {:>9} {:>9} {:>9} {:>9} {:>9}  (round trip, us)",
            "size", "count", "lost", "p50", "p90", "p99", "p99.9", "max"
        );
    }
    for (i, &size) in o.sizes.iter().enumerate() {
        let ep = &mut eps[if eps.len() == 1 { 0 } else { i }];
        let warm_end = Instant::now() + o.warmup;
        let end = warm_end + o.duration;
        let mut rtts = Vec::new();
        let mut lost = 0u64;
        // Leftovers of the size before.
        got.clear();
        ep.recv(&mut got);
        loop {
            let now = Instant::now();
            let measuring = now >= warm_end;
            if now >= end || (measuring && o.count.is_some_and(|c| rtts.len() as u64 >= c)) {
                break;
            }
            seq += 1;
            let sent = Instant::now();
            let stamp = sent.duration_since(start).as_nanos() as u64;
            match ep.send(seq, stamp, size) {
                Ok(()) => {}
                Err(Error::Timeout) => {
                    if measuring {
                        lost += 1;
                    }
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
            let deadline = sent + o.reply_timeout;
            loop {
                let left = deadline.saturating_duration_since(Instant::now());
                if left.is_zero() || !ep.wait(left) {
                    if measuring {
                        lost += 1;
                    }
                    break;
                }
                got.clear();
                ep.recv(&mut got);
                if got.iter().any(|g| g.seq == seq) {
                    if measuring {
                        rtts.push(sent.elapsed().as_nanos() as u64);
                    }
                    break;
                }
            }
        }
        let s = Summary::of(&mut rtts);
        let us = |ns: u64| ns as f64 / 1000.0;
        if o.json {
            let mut f = o.common(size);
            f.extend([
                ("count", J::U(s.count as u64)),
                ("lost", J::U(lost)),
                ("seconds", J::F(o.duration.as_secs_f64())),
                ("rtt_min_us", J::F(us(s.min))),
                ("rtt_mean_us", J::F(us(s.mean))),
                ("rtt_p50_us", J::F(us(s.p50))),
                ("rtt_p90_us", J::F(us(s.p90))),
                ("rtt_p99_us", J::F(us(s.p99))),
                ("rtt_p999_us", J::F(us(s.p999))),
                ("rtt_max_us", J::F(us(s.max))),
            ]);
            println!("{}", json(&f));
        } else {
            println!(
                "{:>8} {:>8} {:>6} {:>9.1} {:>9.1} {:>9.1} {:>9.1} {:>9.1}",
                size,
                s.count,
                lost,
                us(s.p50),
                us(s.p90),
                us(s.p99),
                us(s.p999),
                us(s.max)
            );
        }
    }
    Ok(())
}

/// Whether, having matched, every endpoint has lost its matches.
fn all_gone(eps: &[Box<dyn Endpoint>]) -> bool {
    eps.iter().all(|e| e.matched() == (0, 0))
}

fn pong(o: &Opts, eps: &mut [Box<dyn Endpoint>]) -> Result<(), BoxError> {
    let waitset = WaitSet::new();
    for e in eps.iter() {
        if let Some(c) = e.condition() {
            waitset.attach_condition(&c)?;
        }
    }
    let deadline = Instant::now() + o.timeout;
    let mut matched = false;
    let mut got = Vec::new();
    let mut echoed = 0u64;
    loop {
        match waitset.wait(Duration::from_millis(200)) {
            Ok(_) | Err(Error::Timeout) => {}
            Err(e) => return Err(e.into()),
        }
        for e in eps.iter_mut() {
            got.clear();
            e.recv(&mut got);
            for g in &got {
                match e.send(g.seq, g.stamp, g.len) {
                    Ok(()) | Err(Error::Timeout) => echoed += 1,
                    Err(e) => return Err(e.into()),
                }
            }
        }
        if matched {
            if all_gone(eps) {
                break;
            }
        } else if eps.iter().any(|e| e.matched() != (0, 0)) {
            matched = true;
        } else if Instant::now() >= deadline {
            return Err(format!("pong: no ping in {:?}", o.timeout).into());
        }
    }
    if !o.json {
        println!("pong: sent back {echoed} samples");
    }
    Ok(())
}

fn publish(o: &Opts, eps: &mut [Box<dyn Endpoint>]) -> Result<(), BoxError> {
    wait_for_match(eps, o)?;
    let ep = &mut eps[0];
    let size = o.sizes[0];
    let start = Instant::now();
    // The subscriber measures warmup + duration from its first sample.
    let give_up = start + o.warmup + o.duration + o.timeout;
    let mut seq = 0u64;
    let mut blocked = 0u64;
    loop {
        if seq.is_multiple_of(256) && (ep.matched().0 == 0 || Instant::now() >= give_up) {
            break;
        }
        let stamp = start.elapsed().as_nanos() as u64;
        match ep.send(seq + 1, stamp, size) {
            Ok(()) => seq += 1,
            // Reliable and its history full for max_blocking_time.
            Err(Error::Timeout) => {
                blocked += 1;
                if ep.matched().0 == 0 {
                    break;
                }
            }
            Err(e) => return Err(e.into()),
        }
        if o.rate > 0.0 {
            let next = start + Duration::from_secs_f64(seq as f64 / o.rate);
            let now = Instant::now();
            if next > now {
                std::thread::sleep(next - now);
            }
        }
    }
    let secs = start.elapsed().as_secs_f64();
    if o.json {
        let mut f = o.common(size);
        f.extend([
            ("samples", J::U(seq)),
            ("seconds", J::F(secs)),
            ("samples_per_s", J::F(seq as f64 / secs)),
            ("blocked", J::U(blocked)),
        ]);
        println!("{}", json(&f));
    } else {
        println!(
            "pub: wrote {seq} samples of {size} octets in {secs:.1} s ({:.0}/s), blocked {blocked} times",
            seq as f64 / secs
        );
    }
    Ok(())
}

fn subscribe(o: &Opts, eps: &mut [Box<dyn Endpoint>]) -> Result<(), BoxError> {
    wait_for_match(eps, o)?;
    let ep = &mut eps[0];
    let size = o.sizes[0];
    if !ep.wait(o.timeout) {
        return Err(format!("sub: no sample in {:?}", o.timeout).into());
    }
    let warm_end = Instant::now() + o.warmup;
    let end = warm_end + o.duration;
    let mut got = Vec::new();
    let (mut samples, mut octets) = (0u64, 0u64);
    let (mut first, mut last) = (None, 0u64);
    // Samples in each second, for how steady the rate is.
    let mut per_second = Vec::new();
    let mut second_end = warm_end + Duration::from_secs(1);
    let mut in_second = 0u64;
    loop {
        let now = Instant::now();
        if now >= end {
            break;
        }
        if now >= second_end && now > warm_end {
            per_second.push(in_second);
            in_second = 0;
            second_end += Duration::from_secs(1);
        }
        ep.wait((end - now).min(Duration::from_millis(50)));
        got.clear();
        ep.recv(&mut got);
        if Instant::now() < warm_end {
            continue;
        }
        for g in &got {
            samples += 1;
            in_second += 1;
            octets += g.len as u64;
            first.get_or_insert(g.seq);
            last = last.max(g.seq);
        }
    }
    let secs = o.duration.as_secs_f64();
    // Those the writer wrote in the window that did not come.
    let lost = first.map_or(0, |f| (last - f + 1).saturating_sub(samples));
    let (min_s, max_s) = (
        per_second.iter().copied().min().unwrap_or(0),
        per_second.iter().copied().max().unwrap_or(0),
    );
    let mbit = octets as f64 * 8.0 / secs / 1e6;
    if o.json {
        let mut f = o.common(size);
        f.extend([
            ("samples", J::U(samples)),
            ("octets", J::U(octets)),
            ("lost", J::U(lost)),
            ("seconds", J::F(secs)),
            ("samples_per_s", J::F(samples as f64 / secs)),
            ("mbit_per_s", J::F(mbit)),
            ("samples_per_s_min", J::U(min_s)),
            ("samples_per_s_max", J::U(max_s)),
        ]);
        println!("{}", json(&f));
    } else {
        println!(
            "sub: {samples} samples of {size} octets in {secs:.1} s: {:.0}/s, {mbit:.1} Mbit/s, \
             {lost} lost (each second {min_s} to {max_s})",
            samples as f64 / secs
        );
    }
    Ok(())
}

fn run(args: &[String]) -> Result<(), BoxError> {
    let o = parse(args)?;
    let p = participant(&o)?;
    let mut eps = endpoints(&o, &p)?;
    match o.mode {
        Mode::Ping => ping(&o, &mut eps),
        Mode::Pong => pong(&o, &mut eps),
        Mode::Pub => publish(&o, &mut eps),
        Mode::Sub => subscribe(&o, &mut eps),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn options() {
        let o = parse(&args(
            "ping --sizes 32,1024 --reliability best-effort --json",
        ))
        .unwrap();
        assert_eq!(o.sizes, [32, 1024]);
        assert!(!o.reliable && o.json);
        let o = parse(&args("sub")).unwrap();
        assert_eq!(o.sizes, [1024]);
        assert!(parse(&args("pub --sizes 1,2")).is_err());
        assert!(parse(&args("ping --transport loan --size 100")).is_err());
        assert!(parse(&args("ping --transport loan --size 4096")).is_ok());
        assert!(parse(&args("bogus")).is_err());
    }

    #[test]
    fn json_line() {
        let s = json(&[("a", J::S("x\"y")), ("b", J::U(3)), ("c", J::F(1.5))]);
        assert_eq!(s, r#"{"a":"x\"y","b":3,"c":1.500}"#);
    }
}
