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
    store
        .call(move |c| directory::upsert_team(c, &team_id, &team, &url))
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
    Ok(stats)
}
