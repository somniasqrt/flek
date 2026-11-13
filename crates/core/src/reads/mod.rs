//! Offline reads shared by CLI and app, built exclusively from SQLite.

mod users;
pub use users::*;
mod history;
pub use history::*;
mod unread;
pub use unread::*;
mod mentions;
pub use mentions::*;
mod files;
pub use files::*;
mod channel;
pub use channel::*;
pub(crate) use mentions::mention_source;

/// Known workspace ids, including teams whose session cannot currently start.
pub fn team_ids(conn: &rusqlite::Connection) -> crate::Result<Vec<String>> {
    let mut statement = conn.prepare("SELECT id FROM teams ORDER BY id")?;
    let ids = statement
        .query_map([], |row| row.get(0))?
        .collect::<std::result::Result<_, _>>()?;
    Ok(ids)
}
