//! Explicit sidebar actions; persist only after Slack confirms remote writes.
use crate::{
    api::Client,
    store::Store,
    sync::live::{Bus, Change},
    Error, Result,
};

pub async fn preference(
    store: &Store,
    bus: &Bus,
    id: &str,
    starred: Option<bool>,
    notification: Option<String>,
) -> Result<()> {
    if notification
        .as_ref()
        .is_some_and(|n| !matches!(n.as_str(), "all" | "mentions" | "muted"))
    {
        return Err(Error::Store("unknown notification preference".into()));
    }
    let id = id.to_owned();
    store
        .call(move |c| {
            c.execute(
                "
                    UPDATE conversations
                    SET starred = COALESCE(?2, starred), notification = COALESCE(?3, notification)
                    WHERE id = ?1
                ",
                rusqlite::params![id, starred, notification],
            )?;
            Ok(())
        })
        .await?;
    let _ = bus.send(Change::Directory);
    Ok(())
}

pub async fn dismiss(client: &Client, store: &Store, bus: &Bus, id: &str) -> Result<()> {
    let key = id.to_owned();
    let (kind, general) = store
        .read(move |c| {
            Ok(c.query_row(
                "
                    SELECT kind, COALESCE(json_extract(raw_json, '$.is_general'), 0)
                    FROM conversations
                    WHERE id = ?1
                ",
                [key],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?)),
            )?)
        })
        .await?;
    let dm = matches!(kind.as_str(), "im" | "mpim");
    if general {
        return Err(Error::Store(
            "Cannot leave the workspace's general channel".into(),
        ));
    }
    if dm {
        client.conversations_close(id).await?;
    } else {
        client.conversations_leave(id).await?;
    }
    let id = id.to_owned();
    store
        .call(move |c| {
            if dm {
                c.execute(
                    "
                        UPDATE conversations
                        SET closed_at = COALESCE(latest_ts, '0')
                        WHERE id = ?1
                    ",
                    [id],
                )?;
            } else {
                c.execute(
                    "
                        UPDATE conversations
                        SET is_member = 0
                        WHERE id = ?1
                    ",
                    [id],
                )?;
            }
            Ok(())
        })
        .await?;
    let _ = bus.send(Change::Directory);
    Ok(())
}
