//! Users and conversations into the database.

use crate::api::{AuthTest, Client};
use crate::store::{directory, Store};
use crate::Result;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize)]
pub struct DirectoryStats {
    pub users: usize,
    pub conversations: usize,
}

/// Fetches the team's users and the user's conversations and stores them.
pub async fn sync_directory(
    client: &Client,
    store: &Store,
    me: &AuthTest,
) -> Result<DirectoryStats> {
    let (team_id, team, url, self_user) = (
        me.team_id.clone(),
        me.team.clone(),
        me.url.clone(),
        me.user_id.clone(),
    );
    let me_id = self_user.clone();
    store
        .call(move |c| directory::upsert_team(c, &team_id, &team, &url, &me_id))
        .await?;

    let users = client.users_list().await?;
    tracing::info!(count = users.len(), "fetched users");
    let convs = client.users_conversations().await?;
    tracing::info!(count = convs.len(), "fetched conversations");

    let stats = DirectoryStats {
        users: users.len(),
        conversations: convs.len(),
    };
    let team_id = me.team_id.clone();
    store
        .call(move |c| {
            directory::upsert_users(c, &team_id, &users)?;
            directory::upsert_conversations(c, &team_id, &self_user, &convs)
        })
        .await?;
    // Undocumented; unread counts just stay unknown if it fails.
    match client.client_counts().await {
        Ok(markers) => {
            let markers: Vec<_> = markers.into_iter().map(|m| (m.id, m.last_read)).collect();
            store
                .call(move |c| directory::set_read_markers(c, &markers))
                .await?;
        }
        Err(e) if e.is_auth_error() => return Err(e),
        Err(e) => tracing::warn!(error = %e, "client.counts failed; unread counts unavailable"),
    }
    Ok(stats)
}

/// How often the custom emoji list is refreshed while the app runs.
pub const EMOJI_MAX_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 60 * 60);

/// Fetches `emoji.list` unless it was stored less than `max_age` ago (zero
/// forces it, e.g. once per start). Returns whether it was fetched. Any error
/// but an auth one is logged: emoji then stay as `:name:` text.
pub async fn sync_custom_emoji(
    client: &Client,
    store: &Store,
    team_id: &str,
    max_age: std::time::Duration,
) -> Result<bool> {
    let team = team_id.to_owned();
    let last = store
        .call(move |c| crate::store::emoji::synced_at(c, &team))
        .await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    if !max_age.is_zero() && last.is_some_and(|t| now - t < max_age.as_secs() as i64) {
        return Ok(false);
    }
    match client.emoji_list().await {
        Ok(list) => {
            tracing::info!(count = list.len(), "fetched custom emoji");
            let team = team_id.to_owned();
            store
                .call(move |c| crate::store::emoji::replace(c, &team, &list))
                .await?;
            Ok(true)
        }
        Err(e) if e.is_auth_error() => Err(e),
        Err(e) => {
            tracing::warn!(error = %e, "emoji.list failed; custom emoji stay as text");
            Ok(false)
        }
    }
}
