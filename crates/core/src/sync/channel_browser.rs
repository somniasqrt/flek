//! User-triggered directory, preview, join and create. Never called by background sync.
use super::live::{Bus, Change};
use crate::{
    api::{Client, HistoryQuery},
    store::{channel_browser, Store},
    Error, Result,
};

/// Shared with the UI preview helper; server creation always validates again here.
pub fn normalize_name(input: &str) -> String {
    input
        .to_lowercase()
        .chars()
        .map(|c| if c.is_whitespace() { '-' } else { c })
        .collect()
}

pub fn validate_name(name: &str) -> Result<()> {
    let code = if name.is_empty() {
        Some("invalid_name_required")
    } else if name.chars().count() > 80 {
        Some("invalid_name_maxlength")
    } else if !name
        .chars()
        .all(|c| c.is_alphabetic() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        Some("invalid_name_specials")
    } else {
        None
    };
    match code {
        Some(code) => Err(Error::Slack {
            code: code.into(),
            needed: None,
        }),
        None => Ok(()),
    }
}

pub async fn refresh(client: &Client, store: &Store, team: &str, force: bool) -> Result<()> {
    let key = team.to_owned();
    let cached = store.read(move |c| channel_browser::read(c, &key)).await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    if !force
        && cached
            .fetched_at
            .is_some_and(|t| (0..3600).contains(&(now - t)))
    {
        return Ok(());
    }
    // Cache the inclusive directory once, so toggling Archived remains instant/offline.
    let directory = client.conversations_list(true).await?;
    let team = team.to_owned();
    store
        .call(move |c| {
            let tx = c.transaction()?;
            for channel in directory.channels {
                channel_browser::upsert(&tx, &team, &channel, false)?;
            }
            tx.execute(
                "INSERT INTO channel_directory_sync (team_id, fetched_at, truncated)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(team_id) DO UPDATE SET
                 fetched_at = excluded.fetched_at, truncated = excluded.truncated",
                rusqlite::params![team, now, directory.truncated],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
}

/// One recent page only; preview must not move the catch-up markers or start a full walk.
pub async fn preview(client: &Client, store: &Store, team: &str, id: &str) -> Result<()> {
    let key = id.to_owned();
    let team = team.to_owned();
    let allowed = store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT kind = 'public_channel' OR is_member = 1
         FROM conversations WHERE id = ?1 AND team_id = ?2",
                rusqlite::params![key, team],
                |r| r.get::<_, bool>(0),
            )?)
        })
        .await?;
    if !allowed {
        return Err(Error::Slack {
            code: "channel_not_found".into(),
            needed: None,
        });
    }
    let page = client
        .conversations_history(id, HistoryQuery::default())
        .await?;
    let rows = page
        .messages
        .iter()
        .map(super::backfill::to_row)
        .collect::<Vec<_>>();
    let id = id.to_owned();
    store
        .call(move |c| crate::store::messages::upsert_messages(c, &id, &rows))
        .await?;
    Ok(())
}

#[derive(Debug, serde::Serialize)]
pub struct ChannelAction {
    pub id: String,
    /// The remote write succeeded; secondary work failed. Never invite a duplicate create.
    pub warning: Option<String>,
}

pub async fn join(
    client: &Client,
    store: &Store,
    bus: &Bus,
    team: &str,
    id: &str,
) -> Result<ChannelAction> {
    let channel = client.conversations_join(id).await?;
    let key = channel.id.clone();
    let team = team.to_owned();
    store
        .call(move |c| channel_browser::upsert(c, &team, &channel, true))
        .await?;
    let _ = bus.send(Change::Directory);
    let warning = super::fresh_conversation(client, store, &key, None)
        .await
        .err()
        .map(|_| "catch_up_failed".to_owned());
    let _ = bus.send(Change::History {
        conversation_id: key.clone(),
    });
    Ok(ChannelAction { id: key, warning })
}

pub async fn create(
    client: &Client,
    store: &Store,
    bus: &Bus,
    team: &str,
    name: &str,
    private: bool,
    purpose: &str,
) -> Result<ChannelAction> {
    let name = normalize_name(name);
    validate_name(&name)?;
    let channel = client.conversations_create(&name, private).await?;
    let id = channel.id.clone();
    let team = team.to_owned();
    // Persist creation BEFORE the optional second write, including on purpose failure.
    store
        .call(move |c| channel_browser::upsert(c, &team, &channel, true))
        .await?;
    let _ = bus.send(Change::Directory);
    let mut warning = None;
    if !purpose.trim().is_empty() {
        match client.conversations_set_purpose(&id, purpose.trim()).await {
            Ok(value) => {
                let key = id.clone();
                store
                    .call(move |c| {
                        c.execute(
                            "UPDATE conversations SET purpose = ?2,
                         raw_json = json_set(raw_json, '$.purpose.value', ?2) WHERE id = ?1",
                            rusqlite::params![key, value],
                        )?;
                        Ok(())
                    })
                    .await?;
            }
            Err(_) => warning = Some("purpose_failed".into()),
        }
    }
    let _ = bus.send(Change::Directory);
    Ok(ChannelAction { id, warning })
}
