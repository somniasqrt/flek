//! Deterministic, credential-free, file-backed fixture shared by budget runners.
use flek_core::store::Store;
use flek_core::Result;
use rusqlite::params;

pub const TEAM: &str = "TBENCH";
pub const CHANNEL: &str = "CBENCH000";
pub const MESSAGES: usize = 100_000;
pub const SEED: u64 = 42;

pub struct Fixture {
    _temp: tempfile::TempDir,
    pub path: std::path::PathBuf,
}

impl Fixture {
    pub fn new() -> Result<Self> {
        let temp = tempfile::tempdir()?;
        let path = temp.path().join("synthetic.db");
        let db = Store::open(&path)?;
        db.with_conn(|c| {
            let tx = c.transaction()?;
            tx.execute(
                "INSERT INTO teams (id, name, self_user_id, url)
                 VALUES (?1, 'Synthetic team', 'UBENCH000', 'https://synthetic.invalid')",
                [TEAM],
            )?;
            for i in 0..650 {
                tx.execute(
                    "INSERT INTO users (id, team_id, name, display_name)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![format!("UBENCH{i:03}"), TEAM, format!("person{i:03}"), format!("Person {i:03}")],
                )?;
            }
            for i in 0..100 {
                let kind = if i < 75 { "public_channel" } else if i < 95 { "im" } else { "mpim" };
                tx.execute(
                    "INSERT INTO conversations
                     (id, team_id, kind, name, label, dm_user_id, last_read, raw_json)
                     VALUES (?1, ?2, ?3, ?4, ?4, ?5, '1700000000.000000', ?6)",
                    params![format!("CBENCH{i:03}"), TEAM, kind, format!("channel{i:03}"),
                        (kind == "im").then(|| format!("UBENCH{i:03}")),
                        r#"{"members":["UBENCH000","UBENCH001","UBENCH002"]}"#],
                )?;
            }
            let words = ["оцінки", "таблиця", "звіт", "дедлайн", "grades", "sheet", "report", "deadline",
                "meeting", "lunch", "release", "bug", "fix", "deploy", "review", "design",
                "проєкт", "зустріч", "питання", "відповідь"];
            let mut seed = SEED;
            {
                let mut insert = tx.prepare(
                    "INSERT INTO messages
                     (conversation_id, ts, user_id, text, thread_ts, reply_count, raw_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                )?;
                for i in 0..MESSAGES {
                    // One 10k top-level timeline; the other 90k span 99 conversations.
                    let channel = if i < 10_000 { 0 } else { 1 + (i - 10_000) / 910 };
                    let id = format!("CBENCH{channel:03}");
                    let ts = timestamp(i);
                    let user = if i % 10 == 0 { 0 } else { 1 + i % 649 };
                    let mut text = String::new();
                    for _ in 0..12 {
                        seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
                        text.push_str(words[(seed >> 33) as usize % words.len()]);
                        text.push(' ');
                    }
                    if i % 10_000 == 0 { text.push_str("needle "); }
                    if i % 1000 == 0 { text.push_str("<@UBENCH000> <https://example.invalid/fixture> "); }
                    let reply = i >= 10_000 && i % 10 >= 8;
                    let root = reply.then(|| timestamp(i / 10 * 10));
                    let replies = if i >= 10_000 && i % 10 == 0 { 2 } else { 0 };
                    let raw = serde_json::json!({"type":"message", "ts":ts, "user":format!("UBENCH{user:03}"),
                        "text":text, "synthetic_padding":"x".repeat(768)}).to_string();
                    insert.execute(params![id, ts, format!("UBENCH{user:03}"), text, root, replies, raw])?;
                    if i % 100 == 0 {
                        let file = format!("FBENCH{i:06}");
                        tx.execute("INSERT INTO files (id, name, mimetype, size) VALUES (?1, 'fixture.txt', 'text/plain', 1024)", [&file])?;
                        tx.execute("INSERT INTO message_files (conversation_id, ts, file_id) VALUES (?1, ?2, ?3)", params![id, ts, file])?;
                        tx.execute("INSERT INTO reactions (conversation_id, ts, name, user_id, event_ts) VALUES (?1, ?2, 'wave', 'UBENCH001', ?2)", params![id, ts])?;
                    }
                }
            }
            tx.commit()?;
            // Leave normal triggers/indexes in place; only flush the generated WAL.
            c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
            Ok(())
        })?;
        drop(db);
        Ok(Self { _temp: temp, path })
    }

    pub fn open(&self) -> Result<Store> {
        Store::open(&self.path)
    }
}

pub fn timestamp(i: usize) -> String {
    format!("{}.000000", 1_700_000_000 + i * 60)
}
