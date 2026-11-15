//! Explicit message writes shared by desktop and CLI: own-message edits/deletes
//! (authorize locally, confirm remotely, commit before announcing; RTM echoes use
//! the same merge/deletion rules) and menu actions (permalink, pin, save, forward).
//! Read authoritative state from SQLite.
use crate::{
    api::{chat::UpdateMessage, write::WriteRequest, Client},
    rtm::Event,
    store::{message_actions as cache, messages, Store},
    sync::{
        live::{self, Applied, Bus, Change},
        send::{self, Outgoing, Sent},
    },
    Error, Result,
};
use rusqlite::params;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChangedMessage {
    pub conversation_id: String,
    pub ts: String,
    /// Remote acceptance is final; sync must recover a failed local write.
    pub not_stored_locally: bool,
    #[serde(skip)]
    pub change: Option<Change>,
}

/// Available separately for thin CLI adapters to reject before credential access.
pub fn authorize(
    store: &Store,
    team: &str,
    user: &str,
    channel: &str,
    ts: &str,
) -> Result<messages::EditVersion> {
    if !crate::references::valid_ts(ts) {
        return Err(Error::Usage(
            "Expected a Slack message timestamp (seconds.microseconds)".into(),
        ));
    }
    store.with_read(|c| messages::require_own_message(c, team, user, channel, ts))
}

// Async writes keep even the ownership read off the runtime thread.
async fn authorize_async(
    store: &Store,
    team: &str,
    user: &str,
    channel: &str,
    ts: &str,
) -> Result<messages::EditVersion> {
    if !crate::references::valid_ts(ts) {
        return Err(Error::Usage(
            "Expected a Slack message timestamp (seconds.microseconds)".into(),
        ));
    }
    let (team, user, channel, ts) = (
        team.to_owned(),
        user.to_owned(),
        channel.to_owned(),
        ts.to_owned(),
    );
    store
        .read(move |c| messages::require_own_message(c, &team, &user, &channel, &ts))
        .await
}

pub async fn edit(
    client: &Client,
    store: &Store,
    team: &str,
    user: &str,
    update: UpdateMessage,
) -> Result<ChangedMessage> {
    let version = authorize_async(store, team, user, &update.channel, &update.ts).await?;
    let updated = client.chat_update(&update).await?;
    // Do not allow a malformed response to redirect a local write.
    let matches = updated.channel == update.channel && updated.ts == update.ts;
    let stored = if matches {
        store
            .call(move |c| messages::apply_confirmed_edit(c, &updated, &version))
            .await
            .is_ok()
    } else {
        false
    };
    Ok(ChangedMessage {
        conversation_id: update.channel.clone(),
        ts: update.ts.clone(),
        not_stored_locally: !stored,
        change: stored.then_some(Change::Message {
            conversation_id: update.channel,
            ts: update.ts,
        }),
    })
}

pub async fn delete(
    client: &Client,
    store: &Store,
    team: &str,
    user: &str,
    channel: &str,
    ts: &str,
) -> Result<ChangedMessage> {
    authorize_async(store, team, user, channel, ts).await?;
    let deleted = client.chat_delete(channel, ts).await?;
    let applied = if deleted.channel == channel && deleted.ts == ts {
        live::apply(
            store,
            Event::MessageDeleted {
                channel: channel.into(),
                ts: ts.into(),
            },
        )
        .await
        .ok()
    } else {
        None
    };
    let change = match applied {
        Some(Applied::Changed(change)) => Some(change),
        _ => None,
    };
    Ok(ChangedMessage {
        conversation_id: channel.into(),
        ts: ts.into(),
        not_stored_locally: change.is_none(),
        change,
    })
}

/// Cached/built HTTPS permalink first; only ask Slack when workspace metadata is absent.
/// A deep link is the final offline/unavailable fallback, never a fabricated web URL.
pub async fn permalink(
    client: Option<&Client>,
    store: &Store,
    id: &str,
    ts: &str,
) -> Result<String> {
    let (channel, stamp) = (id.to_owned(), ts.to_owned());
    if let Some(link) = store
        .read(move |c| cache::permalink(c, &channel, &stamp))
        .await?
    {
        return Ok(link);
    }
    if let Some(client) = client {
        match client.chat_get_permalink(id, ts).await {
            Ok(link) => {
                let parsed = crate::references::parse_permalink(&link)
                    .filter(|r| r.conversation_id == id && r.ts == ts);
                if parsed.is_none() {
                    return Err(Error::Store(
                        "invalid message permalink returned by Slack".into(),
                    ));
                }
                let (channel, stamp, value) = (id.to_owned(), ts.to_owned(), link.clone());
                store.call(move |c| {
                    c.execute("INSERT INTO message_permalinks VALUES (?1,?2,?3) ON CONFLICT(conversation_id,ts) DO UPDATE SET permalink=excluded.permalink",params![channel,stamp,value])?;
                    Ok(())
                }).await?;
                return Ok(link);
            }
            Err(e) if e.is_auth_error() => return Err(e),
            Err(_) => {} // Extant local history may no longer be accessible on Slack Free.
        }
    }
    let channel = id.to_owned();
    let team: String = store
        .read(move |c| {
            Ok(c.query_row(
                "SELECT team_id FROM conversations WHERE id=?1",
                [channel],
                |r| r.get(0),
            )?)
        })
        .await?;
    let mut link = reqwest::Url::parse("slack://channel").expect("constant URL");
    link.query_pairs_mut()
        .append_pair("team", &team)
        .append_pair("id", id)
        .append_pair("message", ts);
    Ok(link.into())
}

async fn require(store: &Store, id: &str, ts: &str) -> Result<()> {
    let (id, ts) = (id.to_owned(), ts.to_owned());
    store
        .read(move |c| cache::require_message(c, &id, &ts))
        .await
}

pub async fn pin(
    client: &Client,
    store: &Store,
    bus: &Bus,
    id: &str,
    ts: &str,
    user: &str,
    on: bool,
) -> Result<()> {
    require(store, id, ts).await?;
    let method = if on { "pins.add" } else { "pins.remove" };
    // Idempotent outcomes (`already_pinned`, `no_pin`) are folded into success there.
    WriteRequest::new(method, &[("channel", id), ("timestamp", ts)])
        .execute(client)
        .await?;
    let (action, channel, stamp, user) = (
        if on { "pin" } else { "unpin" },
        id.to_owned(),
        ts.to_owned(),
        user.to_owned(),
    );
    store
        .call(move |c| cache::confirmed(c, action, &channel, &stamp, &user))
        .await?;
    let _ = bus.send(Change::Directory);
    Ok(())
}

/// Attempt documented legacy stars; unsupported stars retain a local bookmark.
/// Even successful stars are not visible in modern Slack Later (see api-notes).
pub async fn save(
    client: Option<&Client>,
    store: &Store,
    bus: &Bus,
    id: &str,
    ts: &str,
    user: &str,
    on: bool,
) -> Result<()> {
    require(store, id, ts).await?;
    if let Some(client) = client {
        let method = if on { "stars.add" } else { "stars.remove" };
        match WriteRequest::new(method, &[("channel", id), ("timestamp", ts)])
            .execute(client)
            .await
        {
            Ok(_) => {}
            Err(Error::Slack { code, .. })
                if matches!(
                    code.as_str(),
                    "missing_scope"
                        | "not_allowed_token_type"
                        | "unknown_method"
                        | "method_not_supported"
                        | "feature_not_enabled"
                        | "deprecated_endpoint"
                        | "message_not_found"
                        | "channel_not_found"
                ) => {}
            Err(e) if e.is_unreachable() => {}
            Err(Error::HttpStatus(404 | 410)) => {}
            Err(e) => return Err(e),
        }
    }
    let (channel, stamp, user) = (id.to_owned(), ts.to_owned(), user.to_owned());
    // No supported Later API exists, so local_only stays true even on stars success.
    store
        .call(move |c| cache::save(c, &channel, &stamp, &user, on, true))
        .await?;
    let _ = bus.send(Change::Message {
        conversation_id: id.into(),
        ts: ts.into(),
    });
    Ok(())
}

/// Body of a forward: the optional comment, then the message permalink on its own line.
pub fn forward_text(link: &str, comment: &str) -> String {
    if comment.trim().is_empty() {
        link.to_owned()
    } else {
        format!("{}\n{}", comment.trim(), link)
    }
}

/// Forward a stored message by posting its link plus the optional comment.
/// Destination is an exact stored conversation id or a stored person's user id.
pub async fn forward(
    client: &Client,
    store: &Store,
    me: &crate::api::AuthTest,
    source: &str,
    ts: &str,
    destination: &str,
    comment: &str,
) -> Result<Sent> {
    require(store, source, ts).await?;
    let (src, dest, team) = (
        source.to_owned(),
        destination.to_owned(),
        me.team_id.clone(),
    );
    let person = store.read(move |c| {
        let source_team: String = c.query_row("SELECT team_id FROM conversations WHERE id=?1",[src],|r| r.get(0))?;
        if source_team != team { return Err(Error::Forbidden); }
        let valid: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM conversations WHERE id=?1 AND team_id=?2 AND is_member=1 AND is_archived=0)",params![dest,team],|r| r.get(0))?;
        if valid { return Ok(false); }
        let person: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM users WHERE id=?1 AND team_id=?2 AND deleted=0 AND is_bot=0)",params![dest,team],|r| r.get(0))?;
        if !person { return Err(Error::NotFound("forward destination is unavailable".into())); }
        Ok(true)
    }).await?;
    let link = permalink(Some(client), store, source, ts).await?;
    // Do not duplicate source text or attachments: permissions on the original still apply.
    let text = forward_text(&link, comment);
    let (user, team) = (destination.to_owned(), me.team_id.clone());
    let existing_dm = if person {
        store.read(move |c| {
            use rusqlite::OptionalExtension;
            Ok(c.query_row("SELECT id FROM conversations WHERE team_id=?1 AND dm_user_id=?2 AND kind='im' AND is_member=1 AND is_archived=0 LIMIT 1",
                params![team,user], |r| r.get::<_,String>(0)).optional()?)
        }).await?
    } else {
        None
    };
    let channel = if let Some(id) = existing_dm {
        id
    } else if person {
        let mut conversation = client.open_forward_dm(destination).await?;
        conversation.is_im = true;
        conversation.user = Some(destination.into());
        let channel = conversation.id.clone();
        let (team, user) = (me.team_id.clone(), me.user_id.clone());
        store
            .call(move |c| {
                crate::store::directory::upsert_conversations(c, &team, &user, &[conversation])
            })
            .await?;
        channel
    } else {
        destination.into()
    };
    send::send(
        client,
        store,
        &me.user_id,
        Outgoing {
            conversation_id: channel,
            text,
            ..Default::default()
        },
    )
    .await
}
