//! Conversation cursors only. Slack's separate, undocumented thread read API is out of scope.
use crate::{
    api::Client,
    store::Store,
    sync::live::{Bus, Change},
    Result,
};

static MARK_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub async fn mark(
    client: &Client,
    store: &Store,
    bus: &Bus,
    conversation: &str,
    ts: &str,
) -> Result<()> {
    if !crate::references::valid_ts(ts) {
        return Err(crate::Error::Usage("invalid message timestamp".into()));
    }
    write(client, store, bus, conversation, ts, false).await
}

/// Explicitly move the cursor before this message, including the first local message.
pub async fn unread(
    client: &Client,
    store: &Store,
    bus: &Bus,
    conversation: &str,
    ts: &str,
) -> Result<String> {
    let (id, target) = (conversation.to_owned(), ts.to_owned());
    let previous = store
        .read(move |c| {
            crate::store::message_actions::require_message(c, &id, &target)?;
            let previous: Option<String> = c.query_row(
                "SELECT MAX(ts) FROM messages WHERE conversation_id=?1 AND ts<?2 AND deleted=0
             AND (thread_ts IS NULL OR thread_ts=ts OR subtype='thread_broadcast')",
                rusqlite::params![id, target],
                |r| r.get(0),
            )?;
            // No earlier local row: a numeric predecessor keeps the target unread.
            previous.map_or_else(|| previous_timestamp(&target), Ok)
        })
        .await?;
    write(client, store, bus, conversation, &previous, true).await?;
    Ok(previous)
}

async fn write(
    client: &Client,
    store: &Store,
    bus: &Bus,
    conversation: &str,
    ts: &str,
    rewind: bool,
) -> Result<()> {
    // Serialize the check + remote write too: concurrent callers must not send an older
    // cursor after a newer one. SQLite's guarded update alone cannot protect Slack.
    let _guard = MARK_LOCK.lock().await;
    let (id, target) = (conversation.to_owned(), ts.to_owned());
    let needed = store
        .read(move |c| {
            Ok(c.query_row(
                "
                    SELECT last_read IS NULL
                    OR last_read < ?2
                    FROM conversations
                    WHERE id = ?1
                ",
                rusqlite::params![id, target],
                |r| r.get::<_, bool>(0),
            )?)
        })
        .await?;
    if !rewind && !needed {
        return Ok(());
    }
    client.conversations_mark(conversation, ts).await?;
    let (id, target) = (conversation.to_owned(), ts.to_owned());
    store
        .call(move |c| {
            c.execute(
                "
                    UPDATE conversations
                    SET last_read = ?2
                    WHERE id = ?1
                    AND (last_read IS NULL
                        OR last_read < ?2 OR ?3)
                ",
                rusqlite::params![id, target, rewind],
            )?;
            Ok(())
        })
        .await?;
    let _ = bus.send(Change::ReadMarker {
        conversation_id: conversation.into(),
    });
    Ok(())
}

/// One microsecond before a Slack timestamp, without floating-point rounding.
pub fn previous_timestamp(ts: &str) -> Result<String> {
    let invalid = || crate::Error::Usage("invalid message timestamp".into());
    if !crate::references::valid_ts(ts) {
        return Err(invalid());
    }
    let (seconds, micros) = ts.split_once('.').ok_or_else(invalid)?;
    let seconds: u64 = seconds.parse().map_err(|_| invalid())?;
    let micros: u64 = micros.parse().map_err(|_| invalid())?;
    if micros > 0 {
        Ok(format!("{seconds}.{:06}", micros - 1))
    } else {
        Ok(format!(
            "{}.999999",
            seconds.checked_sub(1).ok_or_else(invalid)?
        ))
    }
}
