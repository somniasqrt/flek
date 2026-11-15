//! Explicit own-message edits/deletes: authorize locally, confirm remotely,
//! commit before announcing. RTM echoes use the same merge/deletion rules.
use crate::{
    api::{chat::UpdateMessage, Client},
    rtm::Event,
    store::{messages, Store},
    sync::live::{self, Applied, Change},
    Error, Result,
};

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
