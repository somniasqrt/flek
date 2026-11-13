//! Message mod operations.

use crate::Result;
use rusqlite::Connection;

mod write;
pub use write::*;
mod progress;
pub use progress::*;
mod reactions;
pub use reactions::*;
mod display;
pub use display::*;
mod cache;
pub use cache::*;
pub(crate) use write::import_messages_in_transaction;

pub fn conversation_exists(conn: &Connection, conversation_id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "
            SELECT EXISTS (
                SELECT 1
                FROM conversations
                WHERE id = ?1)
        ",
        [conversation_id],
        |r| r.get(0),
    )?)
}
