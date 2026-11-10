//! Database setup and schema v1.

use flek_core::store::{migrations, Store};
use rusqlite::params;

#[test]
fn in_memory_store_is_migrated() {
    let store = Store::open_in_memory().unwrap();
    assert_eq!(
        store.schema_version().unwrap(),
        migrations::latest_version()
    );
    let tables: Vec<String> = store
        .with_conn(|c| {
            let mut stmt =
                c.prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")?;
            let rows = stmt
                .query_map([], |r| r.get(0))?
                .collect::<Result<_, _>>()?;
            Ok(rows)
        })
        .unwrap();
    for t in [
        "conversations",
        "files",
        "message_files",
        "messages",
        "reactions",
        "sync_state",
        "teams",
        "users",
    ] {
        assert!(tables.iter().any(|x| x == t), "missing table {t}");
    }
}

#[test]
fn file_store_uses_wal_and_reopens_without_remigrating() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nested/flek.db");
    {
        let store = Store::open(&path).unwrap();
        let mode: String = store
            .with_conn(|c| Ok(c.query_row("PRAGMA journal_mode", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(mode, "wal");
    }
    let store = Store::open(&path).unwrap();
    assert_eq!(store.path(), Some(path.as_path()));
    let mut conn = rusqlite::Connection::open(&path).unwrap();
    assert!(migrations::migrate(&mut conn).unwrap().is_empty());
    assert_eq!(
        store.schema_version().unwrap(),
        migrations::latest_version()
    );
}

#[test]
fn foreign_keys_are_enforced() {
    let store = Store::open_in_memory().unwrap();
    let err = store.with_conn(|c| {
        c.execute(
            "INSERT INTO conversations (id, team_id, kind) VALUES ('C1', 'T_missing', 'im')",
            [],
        )?;
        Ok(())
    });
    assert!(err.is_err());
}

#[test]
fn reactions_cascade_with_messages() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "INSERT INTO teams (id, name) VALUES ('T1', 'Team');
                 INSERT INTO conversations (id, team_id, kind, name) VALUES ('C1', 'T1', 'public_channel', 'general');
                 INSERT INTO messages (conversation_id, ts, text) VALUES ('C1', '1700000000.000100', 'hi');
                 INSERT INTO reactions (conversation_id,ts,name,user_id) VALUES ('C1', '1700000000.000100', 'wave', 'U1');",
            )?;
            c.execute("DELETE FROM messages WHERE conversation_id = ?1", params!["C1"])?;
            let n: i64 = c.query_row("SELECT COUNT(*) FROM reactions", [], |r| r.get(0))?;
            assert_eq!(n, 0);
            Ok(())
        })
        .unwrap();
}

#[test]
fn fts5_is_available() {
    let store = Store::open_in_memory().unwrap();
    store
        .with_conn(|c| {
            c.execute_batch(
                "CREATE VIRTUAL TABLE t USING fts5(body); INSERT INTO t VALUES ('привіт світ');",
            )?;
            let n: i64 =
                c.query_row("SELECT COUNT(*) FROM t WHERE t MATCH 'світ'", [], |r| {
                    r.get(0)
                })?;
            assert_eq!(n, 1);
            Ok(())
        })
        .unwrap();
}

#[tokio::test]
async fn call_runs_on_blocking_pool() {
    let store = Store::open_in_memory().unwrap();
    let n: i64 = store
        .call(|c| Ok(c.query_row("SELECT COUNT(*) FROM messages", [], |r| r.get(0))?))
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[test]
fn refuses_newer_schema() {
    let mut conn = rusqlite::Connection::open_in_memory().unwrap();
    migrations::migrate(&mut conn).unwrap();
    conn.execute("INSERT INTO schema_version VALUES (999, 0)", [])
        .unwrap();
    assert!(migrations::migrate(&mut conn).is_err());
}

#[test]
fn reads_do_not_wait_for_the_writer() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("flek.db")).unwrap();
    let count = |c: &rusqlite::Connection| -> flek_core::Result<i64> {
        Ok(c.query_row("SELECT COUNT(*) FROM teams", [], |r| r.get(0))?)
    };
    store
        .with_conn(|c| {
            // The writer is locked and mid-transaction; a reader still answers
            // with the last committed state instead of deadlocking.
            let tx = c.transaction()?;
            tx.execute("INSERT INTO teams (id, name) VALUES ('T1', 'Team')", [])?;
            assert_eq!(store.with_read(count)?, 0);
            tx.commit()?;
            assert_eq!(store.with_read(count)?, 1);
            Ok(())
        })
        .unwrap();
    let err = store
        .with_read(|c| Ok(c.execute("DELETE FROM teams", [])?))
        .unwrap_err();
    assert!(err.to_string().contains("readonly"), "{err}");
}
