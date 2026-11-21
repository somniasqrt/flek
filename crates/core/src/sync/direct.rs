//! Opening 1:1 direct messages and group DMs (mpim) with workspace people.
use super::{
    channel_browser::ChannelAction,
    live::{Bus, Change},
};
use crate::{
    api::Client,
    store::{directory, Store},
    Error, Result,
};
use rusqlite::{params, OptionalExtension};

/// Open (or reuse) a direct message with 1 to 8 people in `team`.
///
/// Reuses an existing open 1:1 DM locally without calling Slack. Otherwise
/// calls `conversations.open`, stores the conversation, announces the
/// directory update, and catches up recent history.
pub async fn open(
    client: &Client,
    store: &Store,
    bus: &Bus,
    team: &str,
    me: &str,
    people: &[String],
) -> Result<ChannelAction> {
    let mut ids = Vec::with_capacity(people.len());
    for id in people {
        if !ids.contains(id) {
            ids.push(id.clone());
        }
    }
    if ids.len() > 1 {
        ids.retain(|id| id != me);
    }
    if ids.is_empty() || ids.len() > 8 {
        return Err(Error::Usage("a direct message takes 1 to 8 people".into()));
    }

    let team_id = team.to_owned();
    let check_ids = ids.clone();
    let existing = store
        .read(move |c| {
            let mut stmt = c.prepare("SELECT deleted FROM users WHERE id = ?1 AND team_id = ?2")?;
            for id in &check_ids {
                let deleted: Option<bool> = stmt
                    .query_row(params![id, &team_id], |r| r.get(0))
                    .optional()?;
                match deleted {
                    None => {
                        return Err(Error::NotFound("person is not in this workspace".into()));
                    }
                    Some(true) => {
                        return Err(Error::Usage("cannot open a DM with a deleted user".into()));
                    }
                    Some(false) => {}
                }
            }
            if check_ids.len() == 1 {
                let id = c
                    .query_row(
                        "
                            SELECT id
                            FROM conversations
                            WHERE team_id = ?1
                            AND dm_user_id = ?2
                            AND kind = 'im'
                            AND is_member = 1
                            AND is_archived = 0
                            AND closed_at IS NULL
                            LIMIT 1
                        ",
                        params![&team_id, &check_ids[0]],
                        |r| r.get::<_, String>(0),
                    )
                    .optional()?;
                return Ok(id);
            }
            Ok(None)
        })
        .await?;

    if let Some(id) = existing {
        return Ok(ChannelAction { id, warning: None });
    }

    let conversation = client.open_conversation(&ids).await?;
    let id = conversation.id.clone();
    let (team_id, self_id) = (team.to_owned(), me.to_owned());
    let stored = store
        .call(move |c| directory::save_opened_dm(c, &team_id, &self_id, conversation))
        .await
        .is_ok();
    let _ = bus.send(Change::Directory);
    if !stored {
        return Ok(ChannelAction {
            id,
            warning: Some("not_stored_locally".into()),
        });
    }
    let warning = super::fresh_conversation(client, store, &id, None)
        .await
        .err()
        .map(|_| "catch_up_failed".to_owned());
    let _ = bus.send(Change::History {
        conversation_id: id.clone(),
    });
    Ok(ChannelAction { id, warning })
}
