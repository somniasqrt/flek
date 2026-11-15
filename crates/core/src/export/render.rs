//! Markdown formatting and local reference resolution.
use super::read::{Conversation, Record};
use crate::{references, store::unfurls::url_hash, views, Result};
use rusqlite::Connection;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub(super) struct Context<'a> {
    pub conn: &'a Connection,
    pub conversations: &'a [Conversation],
    pub selected: &'a BTreeSet<String>,
    pub directory: &'a Path,
    pub vault: &'a Path,
    pub aliases: &'a BTreeMap<String, String>,
    pub since: Option<&'a str>,
    pub until: Option<&'a str>,
}

impl Context<'_> {
    pub fn note(&self, path: &str, label: &str) -> String {
        let root = super::disk::vault_relative(self.directory.strip_prefix(self.vault).unwrap());
        format!("[[{root}/{path}|{}]]", literal(label))
    }

    pub fn file_link(&self, path: &str, label: &str) -> String {
        self.note(path, label)
    }

    pub fn message_link(&self, conversation: &Conversation, ts: &str) -> String {
        self.note(
            &format!("{}#^{}", conversation.note(), anchor(ts)),
            &format!("{} · {ts}", conversation.label),
        )
    }

    pub fn url(&self, url: &str, label: &str) -> String {
        if let Some(reference) = references::parse_permalink(url) {
            if self.since.is_none_or(|s| reference.ts.as_str() >= s)
                && self.until.is_none_or(|u| reference.ts.as_str() < u)
            {
                if let Some(c) = self
                    .conversations
                    .iter()
                    .find(|c| c.id == reference.conversation_id && self.selected.contains(&c.id))
                {
                    let stored = self
                        .conn
                        .query_row(
                            "
                        SELECT EXISTS(SELECT 1 FROM messages
                        WHERE conversation_id = ?1 AND ts = ?2 AND deleted = 0)
                    ",
                            rusqlite::params![c.id, reference.ts],
                            |r| r.get::<_, bool>(0),
                        )
                        .unwrap_or(false);
                    if stored {
                        return self
                            .note(&format!("{}#^{}", c.note(), anchor(&reference.ts)), label);
                    }
                }
            }
        }
        if let Ok(parsed) = reqwest::Url::parse(url) {
            if matches!(parsed.scheme(), "https" | "http" | "mailto")
                && parsed.username().is_empty()
                && parsed.password().is_none()
            {
                // Encode Markdown/table delimiters, preserving a clickable URL.
                let target: String = parsed
                    .as_str()
                    .chars()
                    .map(|c| match c {
                        '(' => "%28".into(),
                        ')' => "%29".into(),
                        '|' => "%7C".into(),
                        '<' => "%3C".into(),
                        '>' => "%3E".into(),
                        _ => c.to_string(),
                    })
                    .collect();
                return format!("[{}](<{target}>)", literal(label));
            }
        }
        literal(label)
    }

    fn emoji(&self, name: &str) -> String {
        let mut current = name;
        let mut seen = BTreeSet::new();
        while let Some(next) = self.aliases.get(current) {
            if !seen.insert(current) || seen.len() > 8 {
                return format!(":{}:", literal(name));
            }
            current = next;
        }
        format!(":{}:", literal(current))
    }

    fn plain(&self, text: &str) -> Result<String> {
        let decoded = views::resolve_text(self.conn, text)?;
        let mut out = String::new();
        let mut rest = decoded.as_str();
        while let Some(start) = rest.find(':') {
            out.push_str(&body_literal(&rest[..start]));
            rest = &rest[start..];
            if let Some(end) = rest[1..].find(':').map(|i| i + 1) {
                let name = &rest[1..end];
                if !name.is_empty()
                    && name
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+'))
                {
                    out.push_str(&self.emoji(name));
                    rest = &rest[end + 1..];
                    continue;
                }
            }
            out.push(':');
            rest = &rest[1..];
        }
        out.push_str(&body_literal(rest));
        Ok(out)
    }

    pub fn text(&self, text: &str) -> Result<String> {
        let mut out = String::new();
        let mut rest = text;
        while let Some(start) = rest.find('<') {
            out.push_str(&self.plain(&rest[..start])?);
            let Some(end) = rest[start..].find('>').map(|i| start + i) else {
                out.push_str(&self.plain(&rest[start..])?);
                rest = "";
                break;
            };
            let inner = &rest[start + 1..end];
            let (target, label) = inner.split_once('|').unwrap_or((inner, inner));
            if let Some(id) = target.strip_prefix('#') {
                if let Some(c) = self
                    .conversations
                    .iter()
                    .find(|c| c.id == id && self.selected.contains(id))
                {
                    out.push_str(&self.note(&c.note(), &format!("#{}", c.label)));
                } else {
                    out.push_str(&self.plain(&rest[start..=end])?);
                }
            } else if ["http://", "https://", "mailto:"]
                .iter()
                .any(|s| target.starts_with(s))
            {
                out.push_str(&self.url(
                    &views::resolve_text(self.conn, target)?,
                    &views::resolve_text(self.conn, label)?,
                ));
            } else {
                out.push_str(&self.plain(&rest[start..=end])?);
            }
            rest = &rest[end + 1..];
        }
        out.push_str(&self.plain(rest)?);
        Ok(out)
    }

    pub fn message(
        &self,
        out: &mut String,
        record: &Record,
        present: &BTreeSet<&str>,
    ) -> Result<()> {
        let message = &record.message;
        let time = message.datetime.get(11..19).unwrap_or(&message.ts);
        let author = message
            .user
            .as_deref()
            .filter(|s| !s.is_empty())
            .or(message.user_id.as_deref())
            .unwrap_or("Unknown author");
        out.push_str(&format!(
            "**{} — {}**{}{} ^{}\n\n",
            literal(time),
            literal(author),
            if message.edited { " (edited)" } else { "" },
            if message.hidden_by_limit {
                " (hidden by limit)"
            } else {
                ""
            },
            anchor(&message.ts)
        ));
        if let Some(root) = &message.thread_ts {
            if present.contains(root.as_str()) {
                let c = self
                    .conversations
                    .iter()
                    .find(|c| c.id == message.conversation_id)
                    .unwrap();
                out.push_str(&format!("Reply to {}\n\n", self.message_link(c, root)));
            } else {
                out.push_str(&format!(
                    "Reply to {} (root not in this export)\n\n",
                    literal(root)
                ));
            }
        }
        let body = if message.hidden_by_limit && message.text.trim().is_empty() {
            "_Text hidden by the workspace history limit and not stored locally._".to_owned()
        } else {
            self.text(&message.text)?
        };
        if message.thread_ts.is_some() {
            for line in body.lines() {
                out.push_str(&format!("> {line}\n"));
            }
            out.push('\n');
        } else {
            out.push_str(&format!("{body}\n\n"));
        }
        if message.reply_count > 0 {
            out.push_str(&format!(
                "Thread: {} replies reported in the archive.\n\n",
                message.reply_count
            ));
        }
        if !message.reactions.is_empty() {
            let reactions = message
                .reactions
                .iter()
                .map(|r| {
                    let users = r
                        .users
                        .iter()
                        .map(|id| {
                            crate::store::directory::user_label(self.conn, id)
                                .map(|u| literal(u.as_deref().unwrap_or(id)))
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Ok(format!(
                        "{} ×{} ({})",
                        self.emoji(&r.name),
                        r.count,
                        users.join(", ")
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            out.push_str(&format!("Reactions: {}\n\n", reactions.join("; ")));
        }
        Ok(())
    }
}

fn anchor(ts: &str) -> String {
    format!("m-{}", url_hash(ts))
}

/// Escape authored link/HTML syntax while retaining ordinary emphasis and code.
pub(super) fn literal(text: &str) -> String {
    body_literal(text).replace(['\r', '\n'], " ")
}

fn body_literal(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\\', "\\\\")
        .replace('[', "&#91;")
        .replace(']', "&#93;")
        .replace('|', "&#124;")
}

pub(super) fn links_in(text: &str) -> BTreeSet<String> {
    let mut links = BTreeSet::new();
    for (start, _) in text.char_indices().filter(|(i, _)| {
        ["https://", "http://", "mailto:"]
            .iter()
            .any(|p| text[*i..].starts_with(p))
    }) {
        let rest = &text[start..];
        let end = rest
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '<' | '>' | '|' | '"' | '\'' | '`' | '[' | ']')
            })
            .unwrap_or(rest.len());
        let candidate = rest[..end].trim_end_matches(['.', ',', ';', '!', '?']);
        // Trim unmatched closing parentheses, preserving balanced URL path pairs.
        let mut candidate = candidate;
        while candidate.ends_with(')')
            && candidate.matches(')').count() > candidate.matches('(').count()
        {
            candidate = &candidate[..candidate.len() - 1];
        }
        if reqwest::Url::parse(candidate).is_ok() {
            links.insert(candidate.replace("&amp;", "&"));
        }
    }
    links
}

/// Archive text can itself contain pasted credentials. Never export their values.
pub(super) fn redact(text: &str) -> String {
    let mut out = text.to_owned();
    for prefix in ["xoxc-", "xoxd-", "xoxp-", "xoxb-"] {
        while let Some(start) = out.find(prefix) {
            let end = out[start..]
                .find(|c: char| {
                    c.is_whitespace()
                        || matches!(
                            c,
                            '"' | '\'' | '<' | '>' | ')' | ',' | ';' | '&' | '[' | ']'
                        )
                })
                .map_or(out.len(), |i| start + i);
            out.replace_range(start..end, "[redacted]");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_escapes_markdown_and_html_delimiters() {
        assert_eq!(
            literal("a|b [c] <d> & \\ \n e"),
            "a&#124;b &#91;c&#93; &lt;d&gt; &amp; \\\\   e"
        );
    }

    #[test]
    fn links_keep_balanced_parentheses_and_drop_punctuation() {
        let links =
            links_in("see https://x.test/a_(b), and <https://y.test/p|label>. mailto:a@x.test!");
        assert_eq!(
            links.into_iter().collect::<Vec<_>>(),
            [
                "https://x.test/a_(b)",
                "https://y.test/p",
                "mailto:a@x.test"
            ]
        );
        assert!(links_in("no urls here, not even ftp://x").is_empty());
    }

    #[test]
    fn credentials_are_redacted() {
        assert_eq!(
            redact("a xoxc-1-2 b xoxd-9%2B, c (xoxp-1)"),
            "a [redacted] b [redacted], c ([redacted])"
        );
    }
}
