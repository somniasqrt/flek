//! Presence snapshots and self writes. Network responses commit before readers
//! are notified; no periodic polling or workspace-wide presence discovery.

use crate::api::{Client, CustomStatus, SetPresence, UserPresence};
use crate::store::{presence, Store};
use crate::sync::live::{Bus, Change};
use crate::Result;

pub fn notify(bus: &Bus) {
    let _ = bus.send(Change::Presence);
}

pub async fn stale(store: &Store, bus: &Bus) -> Result<()> {
    store.call(|c| presence::mark_stale(c)).await?;
    notify(bus);
    Ok(())
}

/// A bounded batch for precisely the UI subscription set. Failures keep the
/// unknown/grey fallback; reconnect or a changed set retries, never a timer.
pub async fn snapshot(
    client: &Client,
    store: &Store,
    bus: &Bus,
    me: &str,
    ids: Vec<String>,
) -> Result<()> {
    let users = ids.clone();
    let versions = store
        .read(move |c| {
            users
                .into_iter()
                .map(|id| Ok((id.clone(), presence::versions(c, &id)?)))
                .collect::<Result<std::collections::HashMap<_, _>>>()
        })
        .await?;
    let self_versions = versions.get(me).copied().unwrap_or_default();
    let (team_dnd, self_dnd, self_presence) = tokio::join!(
        client.dnd_team_info(&ids),
        client.dnd_info(),
        client.users_get_presence(me)
    );
    // A denied team batch must not prevent the self snapshot from working.
    let mut failure = None;
    let self_id = if self_dnd.is_ok() {
        me.to_owned()
    } else {
        String::new()
    };
    match team_dnd {
        Ok(rows) => {
            store
                .call(move |c| {
                    let tx = c.transaction()?;
                    for (id, dnd) in rows {
                        if id != self_id
                            && presence::versions(&tx, &id)?.1
                                == versions.get(&id).copied().unwrap_or_default().1
                        {
                            presence::set_dnd(&tx, &id, &dnd)?;
                        }
                    }
                    tx.commit()?;
                    Ok(())
                })
                .await?
        }
        Err(e) => failure = Some(e),
    }
    match self_dnd {
        Ok(dnd) => {
            let user = me.to_owned();
            store
                .call(move |c| {
                    if presence::versions(c, &user)?.1 == self_versions.1 {
                        presence::set_dnd(c, &user, &dnd)?;
                    }
                    Ok(())
                })
                .await?;
        }
        Err(e) => {
            if e.is_auth_error() || failure.is_none() {
                failure = Some(e);
            }
        }
    }
    match self_presence {
        Ok(p) => {
            let user = me.to_owned();
            store
                .call(move |c| {
                    if presence::versions(c, &user)?.0 == self_versions.0 {
                        presence::set_presence(c, &user, &p)?;
                    }
                    Ok(())
                })
                .await?;
        }
        Err(e) => {
            if e.is_auth_error() || failure.is_none() {
                failure = Some(e);
            }
        }
    }
    notify(bus);
    if let Some(e) = failure {
        return Err(e);
    }
    Ok(())
}

pub async fn set_status(
    client: &Client,
    store: &Store,
    bus: &Bus,
    me: &str,
    status: CustomStatus,
) -> Result<()> {
    let user = me.to_owned();
    if !store.read(move |c| presence::user_exists(c, &user)).await? {
        return Err(crate::Error::Store(
            "Your profile is still syncing. Try again in a moment.".into(),
        ));
    }
    let profile = client.users_profile_set(&status).await?;
    let user = me.to_owned();
    store
        .call(move |c| presence::set_profile(c, &user, &profile))
        .await?;
    let _ = bus.send(Change::Directory);
    Ok(())
}

pub async fn set_presence(
    client: &Client,
    store: &Store,
    bus: &Bus,
    me: &str,
    value: SetPresence,
) -> Result<()> {
    client.users_set_presence(value).await?;
    // The setter only returns ok. Refresh self details when possible; if it
    // fails, the confirmed manual override survives and auto stays unknown.
    let snapshot = client.users_get_presence(me).await;
    let user = me.to_owned();
    store
        .call(move |c| match snapshot {
            Ok(mut p) => {
                p.manual_away
                    .get_or_insert(matches!(value, SetPresence::Away));
                presence::set_presence(c, &user, &p)
            }
            Err(_) => {
                let away = matches!(value, SetPresence::Away);
                presence::set_presence(
                    c,
                    &user,
                    &UserPresence {
                        presence: "away".into(),
                        manual_away: Some(away),
                        ..Default::default()
                    },
                )?;
                if !away {
                    c.execute(
                        "
                            UPDATE presence
                            SET presence_fresh = 0
                            WHERE user_id = ?1
                        ",
                        [&user],
                    )?;
                }
                Ok(())
            }
        })
        .await?;
    notify(bus);
    Ok(())
}

pub async fn snooze(
    client: &Client,
    store: &Store,
    bus: &Bus,
    me: &str,
    minutes: Option<u32>,
) -> Result<()> {
    let dnd = match minutes {
        Some(n) => client.dnd_set_snooze(n).await?,
        None => client.dnd_end_snooze().await?,
    };
    let user = me.to_owned();
    store
        .call(move |c| {
            presence::set_snooze(
                c,
                &user,
                minutes.is_some() && dnd.snooze_enabled,
                dnd.snooze_endtime,
            )
        })
        .await?;
    notify(bus);
    Ok(())
}

/// RTM presence queries accept at most 500 ids. Keep self even at the cap.
pub fn subscription(mut ids: Vec<String>, me: &str) -> Vec<String> {
    ids.retain(|id| !id.is_empty() && id != me);
    ids.sort();
    ids.dedup();
    ids.insert(0, me.to_owned());
    ids.truncate(500);
    ids
}
