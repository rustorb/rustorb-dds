//! Reading the governance (10.4.1.2) and permissions (10.4.1.5) XML
//! documents into the access control model.

use roxmltree::{Document, Node};

use crate::Error;
use crate::access::{
    Action, Criteria, DomainRule, Domains, Governance, Grant, Permissions, ProtectionKind, Rule,
    TopicRule,
};

fn children<'a, 'i>(n: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    n.children()
        .filter(move |c| c.is_element() && c.tag_name().name() == name)
}

fn child<'a, 'i>(n: Node<'a, 'i>, name: &'a str) -> Option<Node<'a, 'i>> {
    children(n, name).next()
}

fn required<'a, 'i>(n: Node<'a, 'i>, name: &str) -> Result<Node<'a, 'i>, Error> {
    n.children()
        .find(|c| c.is_element() && c.tag_name().name() == name)
        .ok_or_else(|| Error::new(format!("<{}> has no <{name}>", n.tag_name().name())))
}

fn text<'a>(n: Node<'a, '_>) -> &'a str {
    n.text().unwrap_or("").trim()
}

fn required_text<'a>(n: Node<'a, '_>, name: &str) -> Result<&'a str, Error> {
    Ok(text(required(n, name)?))
}

fn boolean(n: Node<'_, '_>, name: &str) -> Result<bool, Error> {
    match required_text(n, name)? {
        "true" | "1" | "TRUE" => Ok(true),
        "false" | "0" | "FALSE" => Ok(false),
        v => Err(Error::new(format!("<{name}>: {v} is not a boolean"))),
    }
}

fn number(s: &str) -> Result<u32, Error> {
    s.parse()
        .map_err(|_| Error::new(format!("{s} is not a domain id")))
}

fn protection(n: Node<'_, '_>, name: &str) -> Result<ProtectionKind, Error> {
    ProtectionKind::parse(required_text(n, name)?)
}

/// A DomainSet: `<id>` and `<id_range>` elements, then tags.
fn domains(n: Node<'_, '_>) -> Result<Domains, Error> {
    let mut d = Domains::default();
    for c in n.children().filter(|c| c.is_element()) {
        match c.tag_name().name() {
            "id" => {
                let id = number(text(c))?;
                d.ids.push((id, Some(id)));
            }
            "id_range" => {
                let min = child(c, "min").map(|m| number(text(m))).transpose()?;
                let max = child(c, "max").map(|m| number(text(m))).transpose()?;
                if min.is_none() && max.is_none() {
                    return Err(Error::new("<id_range> has neither <min> nor <max>"));
                }
                d.ids.push((min.unwrap_or(0), max));
            }
            "tag" | "tag_expression" => d.tags.push(text(c).to_string()),
            _ => {}
        }
    }
    if d.ids.is_empty() {
        return Err(Error::new("<domains> has no domain id"));
    }
    Ok(d)
}

fn document(xml: &[u8]) -> Result<Document<'_>, Error> {
    let s = std::str::from_utf8(xml.strip_suffix(&[0]).unwrap_or(xml))
        .map_err(|_| Error::new("the XML document is not UTF-8"))?;
    Document::parse(s).map_err(|e| Error::new(format!("XML: {e}")))
}

/// Reads a governance document (the signed content, without its S/MIME
/// envelope).
pub fn governance(xml: &[u8]) -> Result<Governance, Error> {
    let doc = document(xml)?;
    let rules = required(doc.root_element(), "domain_access_rules")?;
    let mut g = Governance::default();
    for r in children(rules, "domain_rule") {
        let mut topic_rules = Vec::new();
        for t in children(required(r, "topic_access_rules")?, "topic_rule") {
            topic_rules.push(TopicRule {
                topic_expression: required_text(t, "topic_expression")?.to_string(),
                enable_discovery_protection: boolean(t, "enable_discovery_protection")?,
                enable_liveliness_protection: boolean(t, "enable_liveliness_protection")?,
                enable_read_access_control: boolean(t, "enable_read_access_control")?,
                enable_write_access_control: boolean(t, "enable_write_access_control")?,
                metadata_protection_kind: protection(t, "metadata_protection_kind")?,
                data_protection_kind: protection(t, "data_protection_kind")?,
            });
        }
        g.domain_rules.push(DomainRule {
            domains: domains(required(r, "domains")?)?,
            allow_unauthenticated_participants: boolean(r, "allow_unauthenticated_participants")?,
            enable_join_access_control: boolean(r, "enable_join_access_control")?,
            discovery_protection_kind: protection(r, "discovery_protection_kind")?,
            liveliness_protection_kind: protection(r, "liveliness_protection_kind")?,
            rtps_protection_kind: protection(r, "rtps_protection_kind")?,
            topic_rules,
        });
    }
    Ok(g)
}

fn criteria(n: Node<'_, '_>) -> Criteria {
    Criteria {
        topics: child(n, "topics")
            .map(|t| children(t, "topic").map(|e| text(e).to_string()).collect())
            .unwrap_or_default(),
        partitions: child(n, "partitions").map(|p| {
            children(p, "partition")
                .map(|e| text(e).to_string())
                .collect()
        }),
    }
}

fn rule(n: Node<'_, '_>, allow: bool) -> Result<Rule, Error> {
    let mut actions = Vec::new();
    for c in n.children().filter(|c| c.is_element()) {
        let a = match c.tag_name().name() {
            "publish" => Action::Publish,
            "subscribe" => Action::Subscribe,
            "relay" => Action::Relay,
            _ => continue,
        };
        actions.push((a, criteria(c)));
    }
    Ok(Rule {
        allow,
        domains: domains(required(n, "domains")?)?,
        actions,
    })
}

/// Seconds since the Unix epoch of an `xs:dateTime`
/// (`YYYY-MM-DDThh:mm:ss[.fff][Z|±hh:mm]`; UTC without a zone).
pub fn date_time(s: &str) -> Result<i64, Error> {
    let bad = || Error::new(format!("{s} is not a dateTime"));
    let (date, time) = s.split_once('T').ok_or_else(bad)?;
    let mut d = date.splitn(3, '-');
    let (y, m, day): (i64, i64, i64) = (
        d.next().and_then(|v| v.parse().ok()).ok_or_else(bad)?,
        d.next().and_then(|v| v.parse().ok()).ok_or_else(bad)?,
        d.next().and_then(|v| v.parse().ok()).ok_or_else(bad)?,
    );
    let (clock, offset) = if let Some(c) = time.strip_suffix('Z') {
        (c, 0)
    } else if let Some(i) = time.rfind(['+', '-']) {
        let (c, z) = time.split_at(i);
        let sign = if z.starts_with('-') { -1 } else { 1 };
        let (h, mi) = z[1..].split_once(':').ok_or_else(bad)?;
        let h: i64 = h.parse().map_err(|_| bad())?;
        let mi: i64 = mi.parse().map_err(|_| bad())?;
        (c, sign * (h * 3600 + mi * 60))
    } else {
        (time, 0)
    };
    let mut c = clock.splitn(3, ':');
    let h: i64 = c.next().and_then(|v| v.parse().ok()).ok_or_else(bad)?;
    let mi: i64 = c.next().and_then(|v| v.parse().ok()).ok_or_else(bad)?;
    let sec: i64 = c
        .next()
        .and_then(|v| v.split('.').next())
        .and_then(|v| v.parse().ok())
        .ok_or_else(bad)?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&day) || h > 24 || mi > 59 || sec > 60 {
        return Err(bad());
    }
    // Days from the civil date (proleptic Gregorian calendar).
    let yy = if m <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Ok(days * 86_400 + h * 3600 + mi * 60 + sec - offset)
}

/// Reads a permissions document (the signed content, without its S/MIME
/// envelope).
pub fn permissions(xml: &[u8]) -> Result<Permissions, Error> {
    let doc = document(xml)?;
    let perms = required(doc.root_element(), "permissions")?;
    let mut p = Permissions::default();
    for g in children(perms, "grant") {
        let (subject_name, expression) = match child(g, "subject_name") {
            Some(s) => (text(s).to_string(), false),
            None => (
                required_text(g, "subject_name_expression")?.to_string(),
                true,
            ),
        };
        let validity = required(g, "validity")?;
        let mut rules = Vec::new();
        for c in g.children().filter(|c| c.is_element()) {
            match c.tag_name().name() {
                "allow_rule" => rules.push(rule(c, true)?),
                "deny_rule" => rules.push(rule(c, false)?),
                _ => {}
            }
        }
        let default_allow = match child(g, "default").map(text) {
            Some("ALLOW") => true,
            Some("DENY") | None => false,
            Some(v) => return Err(Error::new(format!("<default>: {v}"))),
        };
        p.grants.push(Grant {
            name: g.attribute("name").unwrap_or("").to_string(),
            subject_name,
            expression,
            validity: (
                date_time(required_text(validity, "not_before")?)?,
                date_time(required_text(validity, "not_after")?)?,
            ),
            rules,
            default_allow,
        });
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_times() {
        assert_eq!(date_time("1970-01-01T00:00:00").unwrap(), 0);
        assert_eq!(date_time("2000-03-01T00:00:00Z").unwrap(), 951_868_800);
        assert_eq!(date_time("2000-03-01T09:00:00+09:00").unwrap(), 951_868_800);
        assert_eq!(
            date_time("2000-02-29T23:00:00.5-01:00").unwrap(),
            951_868_800
        );
        assert!(date_time("2000-13-01T00:00:00").is_err());
    }
}
