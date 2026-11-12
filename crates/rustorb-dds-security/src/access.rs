//! The builtin access control plugin, DDS:Access:Permissions (DDS
//! Security 1.2, 10.4): the domain governance and the participant
//! permissions, what they decide, and the security attributes they give
//! participants and endpoints.

use crate::Error;

/// A protection kind (10.4.1.2.1, 10.4.1.2.2).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ProtectionKind {
    /// No protection.
    #[default]
    None,
    /// A MAC.
    Sign,
    /// Encryption and a MAC.
    Encrypt,
    /// A MAC and receiver-specific MACs.
    SignWithOriginAuthentication,
    /// Encryption, a MAC and receiver-specific MACs.
    EncryptWithOriginAuthentication,
}

impl ProtectionKind {
    /// From its name in the governance document.
    pub fn parse(s: &str) -> Result<Self, Error> {
        Ok(match s.trim() {
            "NONE" => ProtectionKind::None,
            "SIGN" => ProtectionKind::Sign,
            "ENCRYPT" => ProtectionKind::Encrypt,
            "SIGN_WITH_ORIGIN_AUTHENTICATION" => ProtectionKind::SignWithOriginAuthentication,
            "ENCRYPT_WITH_ORIGIN_AUTHENTICATION" => ProtectionKind::EncryptWithOriginAuthentication,
            s => return Err(Error::new(format!("unknown protection kind {s}"))),
        })
    }

    /// Whether anything is applied.
    pub fn is_protected(self) -> bool {
        self != ProtectionKind::None
    }

    /// Whether it encrypts.
    pub fn encrypts(self) -> bool {
        matches!(
            self,
            ProtectionKind::Encrypt | ProtectionKind::EncryptWithOriginAuthentication
        )
    }

    /// Whether it adds receiver-specific MACs.
    pub fn origin_authenticated(self) -> bool {
        matches!(
            self,
            ProtectionKind::SignWithOriginAuthentication
                | ProtectionKind::EncryptWithOriginAuthentication
        )
    }
}

/// POSIX fnmatch() without flags: `*`, `?`, `[...]` (with `!` or `^` and
/// ranges) and `\` escapes.
pub fn fnmatch(pattern: &str, s: &str) -> bool {
    fn go(p: &[char], s: &[char]) -> bool {
        match p.first() {
            None => s.is_empty(),
            Some('*') => (0..=s.len()).any(|i| go(&p[1..], &s[i..])),
            Some('?') => !s.is_empty() && go(&p[1..], &s[1..]),
            Some('[') => {
                let Some(c) = s.first() else { return false };
                let mut i = 1;
                let negate = matches!(p.get(1), Some('!') | Some('^'));
                if negate {
                    i += 1;
                }
                let mut matched = false;
                let mut first = true;
                while i < p.len() && (first || p[i] != ']') {
                    first = false;
                    if i + 2 < p.len() && p[i + 1] == '-' && p[i + 2] != ']' {
                        if p[i] <= *c && *c <= p[i + 2] {
                            matched = true;
                        }
                        i += 3;
                    } else {
                        if p[i] == *c {
                            matched = true;
                        }
                        i += 1;
                    }
                }
                if i >= p.len() {
                    // No closing bracket: a literal '['.
                    return *c == '[' && go(&p[1..], &s[1..]);
                }
                matched != negate && go(&p[i + 1..], &s[1..])
            }
            Some('\\') if p.len() > 1 => s.first() == Some(&p[1]) && go(&p[2..], &s[1..]),
            Some(c) => s.first() == Some(c) && go(&p[1..], &s[1..]),
        }
    }
    let p: Vec<char> = pattern.chars().collect();
    let s: Vec<char> = s.chars().collect();
    go(&p, &s)
}

/// A set of domains (10.4.1.2.5.1).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Domains {
    /// The domain ids, as inclusive ranges (`None`: no upper bound).
    pub ids: Vec<(u32, Option<u32>)>,
    /// The domain tags and tag expressions; empty means the tag "".
    pub tags: Vec<String>,
}

impl Domains {
    /// Whether the set has `domain` with `tag`.
    pub fn contains(&self, domain: u32, tag: &str) -> bool {
        let id = self
            .ids
            .iter()
            .any(|(min, max)| *min <= domain && max.is_none_or(|m| domain <= m));
        let tag = if self.tags.is_empty() {
            tag.is_empty()
        } else {
            self.tags.iter().any(|t| fnmatch(t, tag))
        };
        id && tag
    }
}

/// A topic rule of the governance (10.4.1.2.6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TopicRule {
    /// topic_expression.
    pub topic_expression: String,
    /// enable_discovery_protection.
    pub enable_discovery_protection: bool,
    /// enable_liveliness_protection.
    pub enable_liveliness_protection: bool,
    /// enable_read_access_control.
    pub enable_read_access_control: bool,
    /// enable_write_access_control.
    pub enable_write_access_control: bool,
    /// metadata_protection_kind.
    pub metadata_protection_kind: ProtectionKind,
    /// data_protection_kind.
    pub data_protection_kind: ProtectionKind,
}

/// A domain rule of the governance (10.4.1.2.5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainRule {
    /// domains.
    pub domains: Domains,
    /// allow_unauthenticated_participants.
    pub allow_unauthenticated_participants: bool,
    /// enable_join_access_control.
    pub enable_join_access_control: bool,
    /// discovery_protection_kind.
    pub discovery_protection_kind: ProtectionKind,
    /// liveliness_protection_kind.
    pub liveliness_protection_kind: ProtectionKind,
    /// rtps_protection_kind.
    pub rtps_protection_kind: ProtectionKind,
    /// topic_access_rules.
    pub topic_rules: Vec<TopicRule>,
}

impl DomainRule {
    /// The first topic rule of `topic`.
    pub fn topic_rule(&self, topic: &str) -> Result<&TopicRule, Error> {
        self.topic_rules
            .iter()
            .find(|r| fnmatch(&r.topic_expression, topic))
            .ok_or_else(|| Error::new(format!("no topic rule for {topic}")))
    }

    /// The ParticipantSecurityAttributesMask and the
    /// PluginParticipantSecurityAttributesMask (9.4.2.5, 10.4.2.4), with
    /// their is_valid bits.
    pub fn participant_masks(&self) -> (u32, u32) {
        const VALID: u32 = 1 << 31;
        let mut m = VALID;
        let mut p = VALID;
        if self.rtps_protection_kind.is_protected() {
            m |= 1 << 0;
        }
        if self.discovery_protection_kind.is_protected() {
            m |= 1 << 1;
        }
        if self.liveliness_protection_kind.is_protected() {
            m |= 1 << 2;
        }
        if self.rtps_protection_kind.encrypts() {
            p |= 1 << 0;
        }
        if self.discovery_protection_kind.encrypts() {
            p |= 1 << 1;
        }
        if self.liveliness_protection_kind.encrypts() {
            p |= 1 << 2;
        }
        if self.rtps_protection_kind.origin_authenticated() {
            p |= 1 << 3;
        }
        if self.discovery_protection_kind.origin_authenticated() {
            p |= 1 << 4;
        }
        if self.liveliness_protection_kind.origin_authenticated() {
            p |= 1 << 5;
        }
        (m, p)
    }
}

impl TopicRule {
    /// The EndpointSecurityAttributesMask and the
    /// PluginEndpointSecurityAttributesMask (9.4.2.8, 10.4.2.6) of a
    /// writer (`writer`) or reader of the topic.
    pub fn endpoint_masks(&self, writer: bool) -> (u32, u32) {
        const VALID: u32 = 1 << 31;
        let mut m = VALID;
        let mut p = VALID;
        if self.enable_read_access_control {
            m |= 1 << 0;
        }
        if self.enable_write_access_control {
            m |= 1 << 1;
        }
        if self.enable_discovery_protection {
            m |= 1 << 2;
        }
        if self.metadata_protection_kind.is_protected() {
            m |= 1 << 3;
        }
        if self.enable_liveliness_protection {
            m |= 1 << 6;
        }
        if self.metadata_protection_kind.encrypts() {
            p |= 1 << 0;
        }
        if self.metadata_protection_kind.origin_authenticated() {
            p |= 1 << 2;
        }
        // The payload is the writer's; readers carry the same bits so
        // that the masks of matching endpoints compare equal.
        let _ = writer;
        if self.data_protection_kind.is_protected() {
            m |= 1 << 4;
        }
        if self.data_protection_kind.encrypts() {
            m |= 1 << 5;
            p |= 1 << 1;
        }
        (m, p)
    }
}

/// The domain governance document (10.4.1.2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Governance {
    /// The domain rules, in order.
    pub domain_rules: Vec<DomainRule>,
}

impl Governance {
    /// The first domain rule of `domain` with `tag`.
    pub fn domain_rule(&self, domain: u32, tag: &str) -> Result<&DomainRule, Error> {
        self.domain_rules
            .iter()
            .find(|r| r.domains.contains(domain, tag))
            .ok_or_else(|| Error::new(format!("no domain rule for domain {domain}")))
    }
}

/// An action of a permissions rule (10.4.1.5.3.4.1.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// publish.
    Publish,
    /// subscribe.
    Subscribe,
    /// relay.
    Relay,
}

/// The conditions of an action of a rule.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Criteria {
    /// topics: names or expressions.
    pub topics: Vec<String>,
    /// partitions: `None` when the section is absent.
    pub partitions: Option<Vec<String>>,
}

/// An allow or deny rule (10.4.1.5.3.4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rule {
    /// Whether it allows (else denies).
    pub allow: bool,
    /// domains.
    pub domains: Domains,
    /// The actions and their conditions.
    pub actions: Vec<(Action, Criteria)>,
}

impl Rule {
    /// Whether the rule applies to `action` on `topic` with `partitions`
    /// in `domain`.
    fn applies(&self, domain: u32, action: Action, topic: &str, partitions: &[String]) -> bool {
        if !self.domains.contains(domain, "") {
            return false;
        }
        self.actions.iter().any(|(a, c)| {
            if *a != action || !c.topics.iter().any(|t| fnmatch(t, topic)) {
                return false;
            }
            let parts: Vec<&str> = if partitions.is_empty() {
                vec![""]
            } else {
                partitions.iter().map(String::as_str).collect()
            };
            if self.allow {
                // Every partition of the entity is allowed; no section
                // allows only the empty partition.
                let allowed = c.partitions.clone().unwrap_or_else(|| vec![String::new()]);
                parts.iter().all(|p| allowed.iter().any(|a| fnmatch(a, p)))
            } else {
                // Some partition is denied; no section denies all.
                let denied = c.partitions.clone().unwrap_or_else(|| vec!["*".into()]);
                parts.iter().any(|p| denied.iter().any(|d| fnmatch(d, p)))
            }
        })
    }
}

/// A grant of the permissions document (10.4.1.5.3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    /// The grant's name.
    pub name: String,
    /// subject_name, or subject_name_expression when `expression`.
    pub subject_name: String,
    /// Whether `subject_name` is an expression.
    pub expression: bool,
    /// validity: not_before and not_after, seconds since the Unix epoch.
    pub validity: (i64, i64),
    /// The allow and deny rules, in order.
    pub rules: Vec<Rule>,
    /// The default: allow (else deny).
    pub default_allow: bool,
}

/// The attribute-value pairs of a subject name (10.4.1.5.3.1).
fn subject_pairs(s: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = s
        .split([',', '/'])
        .filter_map(|a| {
            let (n, v) = a.split_once('=')?;
            Some((n.trim().to_ascii_uppercase(), v.trim().to_string()))
        })
        .filter(|(n, _)| !n.is_empty())
        .collect();
    v.sort();
    v
}

/// Whether two subject names match (10.4.1.5.3.1.1), or a name matches an
/// expression (10.4.1.5.3.2.1).
pub fn subject_matches(name: &str, pattern: &str, expression: bool) -> bool {
    let a = subject_pairs(name);
    let b = subject_pairs(pattern);
    a.len() == b.len()
        && a.iter().zip(&b).all(|((na, va), (nb, vb))| {
            na == nb
                && if expression {
                    fnmatch(vb, va)
                } else {
                    va == vb
                }
        })
}

impl Grant {
    /// Whether the grant is valid at `now` (seconds since the epoch).
    pub fn is_valid_at(&self, now: i64) -> bool {
        self.validity.0 <= now && now <= self.validity.1
    }

    /// Whether `action` on `topic` with `partitions` in `domain` is
    /// allowed (10.4.1.5.3.4).
    pub fn allows(&self, domain: u32, action: Action, topic: &str, partitions: &[String]) -> bool {
        self.rules
            .iter()
            .find(|r| r.applies(domain, action, topic, partitions))
            .map_or(self.default_allow, |r| r.allow)
    }

    /// Whether joining `domain` is allowed: some allow rule names it, or
    /// the default allows.
    pub fn allows_join(&self, domain: u32) -> bool {
        self.default_allow
            || self
                .rules
                .iter()
                .any(|r| r.allow && r.domains.contains(domain, ""))
    }
}

/// A permissions document (10.4.1.5).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Permissions {
    /// The grants.
    pub grants: Vec<Grant>,
}

impl Permissions {
    /// The grant of `subject`, valid at `now`.
    pub fn grant(&self, subject: &str, now: i64) -> Result<&Grant, Error> {
        self.grants
            .iter()
            .find(|g| subject_matches(subject, &g.subject_name, g.expression) && g.is_valid_at(now))
            .ok_or_else(|| Error::new(format!("no valid grant for {subject}")))
    }
}

/// The PermissionsToken (10.4.2.2).
pub fn permissions_token(ca: &crate::pki::Cert) -> crate::token::DataHolder {
    crate::token::DataHolder::new("DDS:Access:Permissions:1.0")
        .with_property("dds.perm_ca.sn", &ca.subject_slashed())
        .with_property("dds.perm_ca.algo", ca.public_key().algorithm().name())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fnmatch_matches_as_posix_does() {
        assert!(fnmatch("*", ""));
        assert!(fnmatch("Square*", "Square"));
        assert!(fnmatch("Square*", "SquareTopic"));
        assert!(!fnmatch("Square*", "Circle"));
        assert!(fnmatch("B*", "Box"));
        assert!(fnmatch("?ox", "Box"));
        assert!(fnmatch("[A-C]ox", "Box"));
        assert!(!fnmatch("[!A-C]ox", "Box"));
        assert!(fnmatch("rt/*", "rt/chatter"));
        assert!(fnmatch("a\\*", "a*"));
        assert!(!fnmatch("a\\*", "ab"));
    }

    #[test]
    fn subject_names_match_in_any_order() {
        assert!(subject_matches(
            "C=JP, O=rustorb, CN=participant1",
            "CN=participant1,O=rustorb,C=JP",
            false
        ));
        assert!(!subject_matches("C=JP, CN=a", "C=JP, CN=b", false));
        assert!(subject_matches("C=JP, CN=robot7", "C=JP, CN=robot*", true));
        // RFC 4514 text of a certificate subject uses commas too.
        assert!(subject_matches(
            "CN=p,O=rustorb,C=JP",
            "C=JP, O=rustorb, CN=p",
            false
        ));
    }

    fn rule(allow: bool, action: Action, topics: &[&str], partitions: Option<&[&str]>) -> Rule {
        Rule {
            allow,
            domains: Domains {
                ids: vec![(0, Some(10))],
                tags: vec![],
            },
            actions: vec![(
                action,
                Criteria {
                    topics: topics.iter().map(|s| s.to_string()).collect(),
                    partitions: partitions.map(|p| p.iter().map(|s| s.to_string()).collect()),
                },
            )],
        }
    }

    #[test]
    fn rules_apply_in_order_then_the_default() {
        let g = Grant {
            name: "g".into(),
            subject_name: "CN=x".into(),
            expression: false,
            validity: (0, i64::MAX),
            rules: vec![
                rule(false, Action::Publish, &["Secret*"], None),
                rule(true, Action::Publish, &["*"], Some(&["A", "B"])),
                rule(true, Action::Subscribe, &["Square"], None),
            ],
            default_allow: false,
        };
        let p = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(!g.allows(0, Action::Publish, "SecretPlans", &p(&["A"])));
        assert!(g.allows(0, Action::Publish, "Square", &p(&["A", "B"])));
        assert!(!g.allows(0, Action::Publish, "Square", &p(&["A", "C"])));
        assert!(!g.allows(0, Action::Publish, "Square", &[]));
        assert!(g.allows(0, Action::Subscribe, "Square", &[]));
        assert!(!g.allows(0, Action::Subscribe, "Circle", &[]));
        assert!(!g.allows(11, Action::Subscribe, "Square", &[]));
        assert!(g.allows_join(5));
        assert!(!g.allows_join(11));
    }
}
