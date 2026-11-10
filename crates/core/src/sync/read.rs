//! Conversation cursors only. Slack's separate, undocumented thread read API is out of scope.
use crate::{
    api::Client,
    store::Store,
    sync::live::{Bus, Change},
    Result,
};

pub async fn mark(
    client: &Client,
    store: &Store,
    bus: &Bus,
    conversation: &str,
    ts: &str,
) -> Result<()> {
    // Serialize the check + remote write too: concurrent callers must not send an older
    // cursor after a newer one. SQLite's guarded update alone cannot protect Slack.
    static MARK_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _guard = MARK_LOCK.lock().await;
    let (id, target) = (conversation.to_owned(), ts.to_owned());
    let needed = store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT last_read IS NULL OR last_read < ?2 FROM conversations WHERE id = ?1",
                rusqlite::params![id, target],
                |r| r.get::<_, bool>(0),
            )?)
        })
        .await?;
    if !needed {
        return Ok(());
    }
    client.conversations_mark(conversation, ts).await?;
    let (id, target) = (conversation.to_owned(), ts.to_owned());
    store
        .call(move |c| {
            c.execute(
                "UPDATE conversations SET last_read = ?2
             WHERE id = ?1 AND (last_read IS NULL OR last_read < ?2)",
                rusqlite::params![id, target],
            )?;
            Ok(())
        })
        .await?;
    let _ = bus.send(Change::ReadMarker {
        conversation_id: conversation.into(),
    });
    Ok(())
}
