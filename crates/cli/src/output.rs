//! CLI framing and safe failure reporting.
use flek_core::{error::ErrorKind, views::SCHEMA_VERSION, Error};
use serde_json::{json, Value};

pub fn emit(mut value: Value) {
    value["schema_version"] = json!(SCHEMA_VERSION);
    println!("{value}");
}

/// Errors may include remote text or URL query parameters. Never expose
/// credential-shaped strings, including percent-encoded cookies.
pub fn safe_message(text: &str) -> String {
    let mut out = text.to_owned();
    for prefix in ["xoxc-", "xoxd-", "xoxp-", "xoxb-"] {
        let mut offset = 0;
        while let Some(pos) = out[offset..].find(prefix).map(|i| i + offset) {
            let end = out[pos..]
                .find(|c: char| {
                    c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>' | ')' | ',' | ';')
                })
                .map_or(out.len(), |i| pos + i);
            out.replace_range(pos..end, "[redacted]");
            offset = pos + "[redacted]".len();
        }
    }
    out
}

pub fn report(error: &anyhow::Error, json_output: bool) -> i32 {
    let core = error.chain().find_map(|e| e.downcast_ref::<Error>());
    let kind = core.map_or(ErrorKind::Internal, Error::kind);
    // reqwest errors can embed a URL; a stable description avoids exposing
    // credentials in query parameters or remote response bodies.
    let message = if core.is_some_and(|e| matches!(e, Error::Http(_) | Error::WebSocket(_))) {
        "could not communicate with Slack".into()
    } else {
        safe_message(&format!("{error:#}"))
    };
    let mut detail = json!({"code":kind,"message":message});
    if let Some(Error::Ambiguous { candidates }) = core {
        detail["candidates"] = json!(candidates);
    }
    if json_output {
        eprintln!(
            "{}",
            json!({"schema_version":SCHEMA_VERSION,"ok":false,"error":detail})
        );
    } else {
        eprintln!("{message}");
    }
    kind.exit_code()
}

/// Frame a change notification. Message bodies are always re-read from SQLite.
pub fn event(
    store: &flek_core::store::Store,
    change: &flek_core::sync::live::Change,
) -> anyhow::Result<Option<Value>> {
    use flek_core::sync::live::Change;
    match change {
        Change::Message {
            conversation_id,
            ts,
        } => Ok(store
            .with_read(|c| flek_core::views::message(c, conversation_id, ts))?
            .map(|message| json!({"type":"message","message":message}))),
        _ => Ok(Some(serde_json::to_value(change)?)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flek_core::{
        store::{
            directory,
            messages::{self, MessageRow},
            Store,
        },
        sync::live::{Change, ConnectionState, Progress},
    };
    #[test]
    fn events_use_stored_messages_and_typed_frames() {
        let store = Store::open_in_memory().unwrap();
        store
            .with_conn(|c| {
                directory::upsert_team(c, "T1", "Fake", "https://fake.slack.com/", "U1")?;
                let conv =
                    serde_json::from_value(json!({"id":"C1","name":"general","is_channel":true}))?;
                directory::upsert_conversations(c, "T1", "U1", &[conv])?;
                messages::upsert_messages(
                    c,
                    "C1",
                    &[MessageRow {
                        ts: "1700000000.000001".into(),
                        text: "stored".into(),
                        raw_json: "{}".into(),
                        ..Default::default()
                    }],
                )
            })
            .unwrap();
        let value = event(
            &store,
            &Change::Message {
                conversation_id: "C1".into(),
                ts: "1700000000.000001".into(),
            },
        )
        .unwrap()
        .unwrap();
        assert_eq!(value["type"], "message");
        assert_eq!(value["message"]["text"], "stored");
        assert_eq!(
            value["message"],
            serde_json::to_value(
                store
                    .with_read(|c| flek_core::views::message(c, "C1", "1700000000.000001"))
                    .unwrap()
                    .unwrap()
            )
            .unwrap()
        );
        for (change, expected) in [
            (
                Change::Progress(Progress::Idle),
                json!({"type":"progress","data":{"stage":"idle"}}),
            ),
            (
                Change::Connection(ConnectionState::Connected),
                json!({"type":"connection","data":{"state":"connected"}}),
            ),
            (Change::Directory, json!({"type":"directory"})),
        ] {
            assert_eq!(event(&store, &change).unwrap().unwrap(), expected);
        }
    }
    #[test]
    fn credential_shaped_error_text_is_redacted() {
        let result = safe_message("bad xoxc-fake; cookie=xoxd-fake%2Fabc token=xoxp-fake");
        assert_eq!(result, "bad [redacted]; cookie=[redacted] token=[redacted]");
    }
}
