use crate::output;

use crate::{open_db, output::emit, Global};
use anyhow::{bail, Context, Result};
use flek_core::{
    auth::{self, CredentialStore, ImportedTeam, Session},
    views,
};
use serde_json::json;

pub(super) async fn whoami(store: &CredentialStore, g: &Global) -> Result<()> {
    let session = auth::resolve_session(store, g.team.as_deref()).await?;
    let Session {
        identity,
        credentials,
    } = &session;
    let profile = session.client()?.profile().clone();
    let issuer = credentials
        .origin
        .as_ref()
        .map_or_else(|| "unknown (generic Chrome)".to_owned(), |o| o.describe());
    if g.json {
        let mut v = serde_json::to_value(identity)?;
        let db = open_db(g)?;
        v["user"] = serde_json::to_value(db.with_read(|c| views::user(c, &identity.user_id))?)?;
        v["user_name"] = json!(identity.user);
        v["client"] = json!({
            "issuer": issuer,
            "user_agent": profile.user_agent,
            "client_hints": profile.hint_headers().into_iter().collect::<std::collections::BTreeMap<_, _>>(),
        });
        emit(v);
    } else {
        println!("team    {} ({})", identity.team, identity.team_id);
        println!("user    {} ({})", identity.user, identity.user_id);
        println!("url     {}", identity.url);
        println!("client  {issuer}");
        println!("ua      {}", profile.user_agent);
        for (name, value) in profile.hint_headers() {
            println!("        {name}: {value}");
        }
    }
    Ok(())
}

pub(super) async fn import(store: &CredentialStore, g: &Global) -> Result<()> {
    let settings_path = flek_core::settings::default_path()?;
    let teams = auth::import_explicit_and_save(store, g.team.as_deref(), &settings_path)
        .await
        .context("importing from Slack desktop")?;
    let failed = teams.iter().filter(|t| t.result.is_err()).count();

    if g.json {
        if failed == teams.len() {
            let error = teams
                .into_iter()
                .find_map(|t| t.result.err())
                .unwrap_or(flek_core::Error::NoCredentials);
            return Err(error.into());
        }
        let rows: Vec<_> = teams.iter().map(import_row).collect();
        emit(json!({"teams":rows}));
    } else {
        for t in &teams {
            match &t.result {
                Ok(s) => println!("saved   {} ({}) as {}", t.name, t.team_id, s.identity.user),
                Err(e) => println!(
                    "failed  {} ({}): {}",
                    t.name,
                    t.team_id,
                    output::safe_message(&e.to_string())
                ),
            }
        }
    }
    if failed == teams.len() {
        bail!("no team could be validated");
    }
    Ok(())
}

pub(super) fn import_row(t: &ImportedTeam) -> serde_json::Value {
    match &t.result {
        Ok(s) => {
            json!({ "team_id": t.team_id, "name": t.name, "ok": true, "user": s.identity.user, "user_id": s.identity.user_id })
        }
        Err(e) => {
            json!({ "team_id": t.team_id, "name": t.name, "ok": false, "error": output::safe_message(&e.to_string()) })
        }
    }
}

pub(super) fn list(store: &CredentialStore, g: &Global) -> Result<()> {
    let ids = store.list()?;
    if g.json {
        emit(json!({"teams":ids}));
    } else if ids.is_empty() {
        println!("no stored teams; run `flek auth import`");
    } else {
        for id in ids {
            println!("{id}");
        }
    }
    Ok(())
}

pub(super) fn logout(store: &CredentialStore, g: &Global, all: bool) -> Result<()> {
    let stored = store.list()?;
    let targets: Vec<String> = match (&g.team, all) {
        (_, true) => stored,
        (Some(t), false) => vec![t.clone()],
        (None, false) if stored.len() <= 1 => stored,
        (None, false) => {
            return Err(flek_core::Error::TeamSelection(format!(
                "several teams are stored ({}); pass --team or --all",
                stored.join(", ")
            ))
            .into())
        }
    };
    let mut removed = Vec::new();
    for id in &targets {
        if store.delete(id)? {
            removed.push(id.clone());
        }
    }
    if g.json {
        emit(json!({"removed":removed}));
    } else if removed.is_empty() {
        println!("nothing to remove");
    } else {
        for id in removed {
            println!("removed {id}");
        }
    }
    Ok(())
}
