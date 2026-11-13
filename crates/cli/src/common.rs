//! Shared offline database selection and terminal formatting.

use crate::Global;
use anyhow::{Context, Result};
use flek_core::store::{self, Store};

pub(super) fn open_db(g: &Global) -> Result<Store> {
    let path = match &g.db {
        Some(p) => p.clone(),
        None => store::default_path()?,
    };
    Store::open(&path).with_context(|| format!("opening database {}", path.display()))
}

pub(super) fn default_team(db: &Store, g: &Global) -> Result<String> {
    if let Some(team) = &g.team {
        return Ok(team.clone());
    }
    let teams = db.with_read(|c| {
        let mut stmt = c.prepare(
            "
                SELECT id
                FROM teams
                WHERE self_user_id IS NOT NULL AND self_user_id != ''
                ORDER BY id
            ",
        )?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    })?;
    match teams.as_slice() {
        [team] => Ok(team.clone()),
        [] => Err(flek_core::Error::NotFound(
            "no stored team; run `flek auth import` and `flek sync`".into(),
        )
        .into()),
        _ => {
            Err(flek_core::Error::TeamSelection("several teams stored; pass --team".into()).into())
        }
    }
}

/// Slack ts → local ("2026-11-08", "14:03").
pub(super) fn format_ts(ts: &str) -> (String, String) {
    let secs = ts
        .split('.')
        .next()
        .and_then(|s| s.parse::<i64>().ok())
        .unwrap_or(0);
    match chrono::DateTime::from_timestamp(secs, 0) {
        Some(t) => {
            let t = t.with_timezone(&chrono::Local);
            (
                t.format("%Y-%m-%d").to_string(),
                t.format("%H:%M").to_string(),
            )
        }
        None => (String::new(), ts.to_owned()),
    }
}
