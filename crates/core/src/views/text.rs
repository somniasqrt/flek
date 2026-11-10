//! Text storage views.

use crate::Result;
use chrono::{DateTime, SecondsFormat};
use rusqlite::{Connection, OptionalExtension};

pub(super) fn datetime(ts: &str) -> Option<String> {
    let (secs, micros) = ts.split_once('.').unwrap_or((ts, "0"));
    let nanos = format!("{micros:0<9}").get(..9)?.parse().ok()?;
    Some(
        DateTime::from_timestamp(secs.parse().ok()?, nanos)?
            .to_rfc3339_opts(SecondsFormat::AutoSi, true),
    )
}

/// Resolve Slack angle-bracket references before decoding escaped text.
/// Unknown identities retain their id (or Slack's supplied label).
pub fn resolve_text(conn: &Connection, text: &str) -> Result<String> {
    resolve_with(text, |sigil, id| {
        if sigil == '@' {
            crate::store::directory::user_label(conn, id)
        } else {
            Ok(conn
                .query_row(
                    "
                        SELECT COALESCE(name, label, id)
                        FROM conversations
                        WHERE id = ?1
                    ",
                    [id],
                    |r| r.get(0),
                )
                .optional()?)
        }
    })
}

pub(super) fn resolve_with(
    text: &str,
    mut lookup: impl FnMut(char, &str) -> Result<Option<String>>,
) -> Result<String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('<') {
        out.push_str(&rest[..start]);
        let Some(end) = rest[start..].find('>').map(|i| start + i) else {
            out.push_str(&rest[start..]);
            rest = "";
            break;
        };
        let inner = &rest[start + 1..end];
        let (target, label) = inner
            .split_once('|')
            .map_or((inner, None), |(a, b)| (a, Some(b)));
        if let Some(id) = target.strip_prefix('@') {
            out.push('@');
            out.push_str(&lookup('@', id)?.unwrap_or_else(|| label.unwrap_or(id).into()));
        } else if let Some(id) = target.strip_prefix('#') {
            let name = lookup('#', id)?;
            out.push('#');
            out.push_str(&name.unwrap_or_else(|| label.unwrap_or(id).into()));
        } else if let Some(special) = target.strip_prefix('!') {
            // `<!here>`, `<!channel>`, `<!subteam^S…|@team>`, `<!date^…|fallback>`.
            match label {
                Some(label) => out.push_str(label),
                None => {
                    out.push('@');
                    out.push_str(special.split('^').next().unwrap_or(special));
                }
            }
        } else if ["https://", "http://", "mailto:"]
            .iter()
            .any(|p| target.starts_with(p))
        {
            if let Some(label) = label {
                out.push_str(label);
                out.push_str(" (");
                out.push_str(target);
                out.push(')');
            } else {
                out.push_str(target);
            }
        } else {
            out.push_str(&rest[start..=end]);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Ok(decode_entities(&out))
}

fn decode_entities(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let Some(end) = rest.find(';') else {
            break;
        };
        let entity = &rest[1..end];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            other => other
                .strip_prefix("#x")
                .and_then(|s| u32::from_str_radix(s, 16).ok())
                .or_else(|| other.strip_prefix('#').and_then(|s| s.parse().ok()))
                .and_then(char::from_u32),
        };
        if let Some(ch) = decoded {
            out.push(ch);
            rest = &rest[end + 1..];
        } else {
            out.push('&');
            rest = &rest[1..];
        }
    }
    out.push_str(rest);
    out
}
