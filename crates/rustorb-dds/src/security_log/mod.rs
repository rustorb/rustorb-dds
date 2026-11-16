//! The builtin logging plugin of DDS Security 1.2 (9.6 and 10.6,
//! DDS:Logging:DDS_LogTopic), feature `security`.
//!
//! [`LogOptions`] in [`SecurityConfig::logging`](crate::SecurityConfig)
//! turn it on. The security events of the participant at or above the
//! options' level are appended to a file, published on the topic
//! [`LOG_TOPIC`] with the type [`BuiltinLoggingTypeV2`], or both.
//!
//! The topic follows the builtin rule of 10.6: any participant may write
//! it, a participant needs a grant to subscribe to it, and its data is
//! encrypted and its metadata signed.

use std::collections::HashMap;
use std::fs::File;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::mpsc::Sender;
use std::time::{SystemTime, UNIX_EPOCH};

#[allow(missing_docs, clippy::all)]
#[path = "dds_security_log.rs"]
pub mod idl;

pub use idl::dds::security::{
    BuiltinLoggingTypeV2, LoggingLevel, NameValuePair, NameValuePairSeq, StructuredDataEntry, TimeT,
};

/// The name of the topic of the builtin logging plugin (10.6).
pub const LOG_TOPIC: &str = "DDS:Security:LogTopicV2";

/// The `facility` of the log messages: security and authorization
/// messages (10.6).
pub const FACILITY: u8 = 10;

/// The options of the logging plugin (9.6.2.1, LogOptions). They are set
/// when the participant is created and cannot change afterwards.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LogOptions {
    /// The messages at this level or more severe are logged (`log_level`).
    pub log_level: LoggingLevel,
    /// The file the messages are appended to (`log_file`), if any.
    pub log_file: Option<PathBuf>,
    /// Whether the messages are published on [`LOG_TOPIC`]
    /// (`distribute`).
    pub distribute: bool,
}

impl Default for LogOptions {
    /// ERROR_LEVEL, no file, not distributed.
    fn default() -> Self {
        LogOptions {
            log_level: LoggingLevel::ErrorLevel,
            log_file: None,
            distribute: false,
        }
    }
}

impl LogOptions {
    /// The property of [`Self::log_level`]: the name of a level of 10.6
    /// (`ERROR_LEVEL`, or `ERROR`, any case) or its number (0 for
    /// EMERGENCY to 7 for DEBUG).
    pub const LOGGING_LEVEL: &'static str = "dds.sec.log.builtin.DDS_LogTopic.logging_level";
    /// The property of [`Self::log_file`]: a path.
    pub const LOG_FILE: &'static str = "dds.sec.log.builtin.DDS_LogTopic.log_file";
    /// The property of [`Self::distribute`]: `true` or `false`.
    pub const DISTRIBUTE: &'static str = "dds.sec.log.builtin.DDS_LogTopic.distribute";

    /// The options given by the properties [`Self::LOGGING_LEVEL`],
    /// [`Self::LOG_FILE`] and [`Self::DISTRIBUTE`]; `None` when there is
    /// none of them, an error when a value is not understood.
    pub fn from_properties(props: &HashMap<&str, &str>) -> Result<Option<Self>, String> {
        let level = props.get(Self::LOGGING_LEVEL);
        let file = props.get(Self::LOG_FILE);
        let distribute = props.get(Self::DISTRIBUTE);
        if level.is_none() && file.is_none() && distribute.is_none() {
            return Ok(None);
        }
        let mut o = LogOptions::default();
        if let Some(l) = level {
            o.log_level = parse_level(l)
                .ok_or_else(|| format!("{}: unknown level {l}", Self::LOGGING_LEVEL))?;
        }
        o.log_file = file.filter(|f| !f.is_empty()).map(PathBuf::from);
        if let Some(d) = distribute {
            o.distribute = match d.to_ascii_lowercase().as_str() {
                "true" | "1" | "yes" => true,
                "false" | "0" | "no" => false,
                _ => return Err(format!("{}: not a boolean: {d}", Self::DISTRIBUTE)),
            };
        }
        Ok(Some(o))
    }
}

const LEVELS: [(LoggingLevel, &str); 8] = [
    (LoggingLevel::EmergencyLevel, "EMERGENCY"),
    (LoggingLevel::AlertLevel, "ALERT"),
    (LoggingLevel::CriticalLevel, "CRITICAL"),
    (LoggingLevel::ErrorLevel, "ERROR"),
    (LoggingLevel::WarningLevel, "WARNING"),
    (LoggingLevel::NoticeLevel, "NOTICE"),
    (LoggingLevel::InformationalLevel, "INFORMATIONAL"),
    (LoggingLevel::DebugLevel, "DEBUG"),
];

/// A level from its name (`ERROR_LEVEL` or `ERROR`, any case) or number.
pub fn parse_level(s: &str) -> Option<LoggingLevel> {
    let s = s.trim().to_ascii_uppercase();
    if let Ok(n) = s.parse::<usize>() {
        return LEVELS.get(n).map(|(l, _)| *l);
    }
    let name = s.strip_suffix("_LEVEL").unwrap_or(&s);
    LEVELS.iter().find(|(_, n)| *n == name).map(|(l, _)| *l)
}

/// The name of a level, as 10.6 writes it (`ERROR_LEVEL`).
pub fn level_name(level: LoggingLevel) -> String {
    let n = LEVELS
        .iter()
        .find(|(l, _)| *l == level)
        .map_or("DEBUG", |(_, n)| n);
    format!("{n}_LEVEL")
}

/// The security plugins, the `plugin_class` of a message (Table 47).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Plugin {
    Authentication,
    AccessControl,
}

impl Plugin {
    fn name(self) -> &'static str {
        match self {
            Plugin::Authentication => "Authentication",
            Plugin::AccessControl => "AccessControl",
        }
    }
}

/// Where the distributed messages go.
enum Sink {
    /// Kept until the participant's writer of [`LOG_TOPIC`] is there.
    Waiting(Vec<BuiltinLoggingTypeV2>),
    Writer(Sender<BuiltinLoggingTypeV2>),
    Closed,
}

/// The logging plugin of a participant (10.6.1): set_log_options and
/// enable_logging are done when it is made.
pub(crate) struct Logger {
    options: LogOptions,
    domain_id: u32,
    guid: Mutex<String>,
    file: Option<Mutex<File>>,
    sink: Mutex<Sink>,
    hostname: String,
    appname: String,
    procid: String,
}

impl Logger {
    /// Opens the log file (appending) if the options name one.
    pub(crate) fn new(options: LogOptions, domain_id: u32) -> std::io::Result<Self> {
        let file = match &options.log_file {
            Some(p) => Some(Mutex::new(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)?,
            )),
            None => None,
        };
        let sink = if options.distribute {
            Sink::Waiting(Vec::new())
        } else {
            Sink::Closed
        };
        Ok(Logger {
            options,
            domain_id,
            guid: Mutex::new(String::new()),
            file,
            sink: Mutex::new(sink),
            hostname: hostname(),
            appname: std::env::current_exe()
                .ok()
                .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                .unwrap_or_default(),
            procid: std::process::id().to_string(),
        })
    }

    pub(crate) fn options(&self) -> &LogOptions {
        &self.options
    }

    /// The GUID of the participant, once it is known.
    pub(crate) fn set_guid(&self, guid: &[u8; 16]) {
        *self.guid.lock().unwrap_or_else(|e| e.into_inner()) = guid_text(guid);
    }

    /// The messages to distribute go to `tx` from now on, those kept so
    /// far first.
    pub(crate) fn attach(&self, tx: Sender<BuiltinLoggingTypeV2>) {
        let mut s = self.sink.lock().unwrap_or_else(|e| e.into_inner());
        if let Sink::Waiting(kept) = std::mem::replace(&mut *s, Sink::Closed) {
            for m in kept {
                let _ = tx.send(m);
            }
            *s = Sink::Writer(tx);
        }
    }

    /// No more messages are distributed (the participant is going).
    pub(crate) fn close(&self) {
        *self.sink.lock().unwrap_or_else(|e| e.into_inner()) = Sink::Closed;
    }

    /// Logs a message if `level` is at or above the options' level
    /// (10.6.1, log).
    pub(crate) fn log(&self, level: LoggingLevel, plugin: Plugin, method: &str, message: &str) {
        if level > self.options.log_level {
            return;
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let guid = self.guid.lock().unwrap_or_else(|e| e.into_inner()).clone();
        if let Some(f) = &self.file {
            let line = format!(
                "{} {} {}.{} guid={guid} domain_id={}: {message}\n",
                rfc3339(now.as_secs() as i64, now.subsec_nanos()),
                level_name(level),
                plugin.name(),
                method,
                self.domain_id,
            );
            let mut f = f.lock().unwrap_or_else(|e| e.into_inner());
            let _ = f.write_all(line.as_bytes());
        }
        let mut sink = self.sink.lock().unwrap_or_else(|e| e.into_inner());
        if matches!(*sink, Sink::Closed) {
            return;
        }
        let pair = |n: &str, v: String| NameValuePair {
            name: n.into(),
            value: v,
        };
        let m = BuiltinLoggingTypeV2 {
            facility: FACILITY,
            severity: level,
            timestamp: TimeT {
                sec: now.as_secs() as i64,
                nanosec: now.subsec_nanos(),
            },
            hostname: self.hostname.clone(),
            hostip: String::new(),
            appname: self.appname.clone(),
            procid: self.procid.clone(),
            msgid: format!("{}.{method}", plugin.name()),
            message: message.into(),
            structured_data: vec![StructuredDataEntry {
                key: "DDS".into(),
                value: vec![
                    pair("guid", guid),
                    pair("domain_id", self.domain_id.to_string()),
                    pair("plugin_class", plugin.name().into()),
                    pair("plugin_method", method.into()),
                ],
            }],
        };
        match &mut *sink {
            Sink::Waiting(kept) => {
                // A participant that fails to be created never takes
                // them: keep a bounded number.
                if kept.len() < 256 {
                    kept.push(m);
                }
            }
            Sink::Writer(tx) => {
                if tx.send(m).is_err() {
                    *sink = Sink::Closed;
                }
            }
            Sink::Closed => {}
        }
    }
}

/// A GUID as its four 32-bit words in hexadecimal, dot separated.
pub(crate) fn guid_text(g: &[u8; 16]) -> String {
    g.chunks(4)
        .map(|c| c.iter().map(|b| format!("{b:02x}")).collect::<String>())
        .collect::<Vec<_>>()
        .join(".")
}

fn hostname() -> String {
    std::env::var("HOSTNAME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| {
            std::fs::read_to_string("/etc/hostname")
                .ok()
                .map(|h| h.trim().to_string())
        })
        .unwrap_or_default()
}

/// A time as RFC 3339 in UTC, with nanoseconds.
fn rfc3339(secs: i64, nanos: u32) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // The civil date of a count of days since 1970-01-01 (the proleptic
    // Gregorian calendar, in eras of 400 years).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{nanos:09}Z",
        rem / 3600,
        rem / 60 % 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_by_name_and_number() {
        assert_eq!(parse_level("ERROR_LEVEL"), Some(LoggingLevel::ErrorLevel));
        assert_eq!(parse_level("debug"), Some(LoggingLevel::DebugLevel));
        assert_eq!(parse_level("0"), Some(LoggingLevel::EmergencyLevel));
        assert_eq!(parse_level("8"), None);
        assert_eq!(parse_level("FATAL"), None);
        assert_eq!(level_name(LoggingLevel::NoticeLevel), "NOTICE_LEVEL");
    }

    #[test]
    fn options_from_properties() {
        let none = HashMap::new();
        assert_eq!(LogOptions::from_properties(&none), Ok(None));
        let p: HashMap<&str, &str> = [
            (LogOptions::LOGGING_LEVEL, "WARNING_LEVEL"),
            (LogOptions::LOG_FILE, "/tmp/x.log"),
            (LogOptions::DISTRIBUTE, "true"),
        ]
        .into();
        assert_eq!(
            LogOptions::from_properties(&p),
            Ok(Some(LogOptions {
                log_level: LoggingLevel::WarningLevel,
                log_file: Some("/tmp/x.log".into()),
                distribute: true,
            }))
        );
        let p: HashMap<&str, &str> = [(LogOptions::DISTRIBUTE, "maybe")].into();
        assert!(LogOptions::from_properties(&p).is_err());
    }

    #[test]
    fn times_and_guids() {
        assert_eq!(rfc3339(0, 0), "1970-01-01T00:00:00.000000000Z");
        assert_eq!(rfc3339(951_782_400, 5), "2000-02-29T00:00:00.000000005Z");
        assert_eq!(rfc3339(1_794_787_199, 0), "2026-11-15T23:59:59.000000000Z");
        let mut g = [0u8; 16];
        g[0] = 0x01;
        g[15] = 0xc1;
        assert_eq!(guid_text(&g), "01000000.00000000.00000000.000000c1");
    }

    #[test]
    fn messages_below_the_level_are_left_out() {
        let dir = std::env::temp_dir().join(format!("rustorb-dds-log-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("security.log");
        let _ = std::fs::remove_file(&path);
        let (tx, rx) = std::sync::mpsc::channel();
        let l = Logger::new(
            LogOptions {
                log_level: LoggingLevel::WarningLevel,
                log_file: Some(path.clone()),
                distribute: true,
            },
            3,
        )
        .unwrap();
        l.log(
            LoggingLevel::ErrorLevel,
            Plugin::Authentication,
            "m1",
            "kept before",
        );
        l.set_guid(&[1; 16]);
        l.attach(tx);
        l.log(
            LoggingLevel::InformationalLevel,
            Plugin::AccessControl,
            "m2",
            "dropped",
        );
        l.log(
            LoggingLevel::WarningLevel,
            Plugin::AccessControl,
            "m3",
            "sent",
        );
        let got: Vec<BuiltinLoggingTypeV2> = rx.try_iter().collect();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].message, "kept before");
        assert_eq!(got[1].severity, LoggingLevel::WarningLevel);
        assert_eq!(got[1].facility, 10);
        let dds = &got[1].structured_data[0];
        assert_eq!(dds.key, "DDS");
        assert_eq!(dds.value[0].value, "01010101.01010101.01010101.01010101");
        assert_eq!(dds.value[3].value, "m3");
        l.close();
        l.log(
            LoggingLevel::ErrorLevel,
            Plugin::AccessControl,
            "m4",
            "file only",
        );
        assert_eq!(rx.try_iter().count(), 0);
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3, "{text}");
        assert!(lines[0].contains("ERROR_LEVEL Authentication.m1 guid= domain_id=3: kept before"));
        assert!(lines[2].contains("AccessControl.m4"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
