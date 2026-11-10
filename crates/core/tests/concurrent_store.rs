//! Separate Store handles model the app and CLI sharing one WAL database.
use flek_core::store::{directory, Store};
use std::{sync::mpsc, thread, time::Duration};

fn pragmas(c: &rusqlite::Connection) -> flek_core::Result<()> {
    assert_eq!(
        c.query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))?,
        "wal"
    );
    assert_eq!(
        c.query_row("PRAGMA busy_timeout", [], |r| r.get::<_, i64>(0))?,
        5000
    );
    Ok(())
}
#[test]
fn readers_see_committed_data_while_other_writer_waits() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("shared.db");
    let app = Store::open(&path).unwrap();
    let cli = Store::open(&path).unwrap();
    app.with_conn(|c| directory::upsert_team(c, "T1", "Before", "https://fake.slack.com/", "U1"))
        .unwrap();
    cli.with_conn(|c| pragmas(c)).unwrap();
    cli.with_read(pragmas).unwrap();
    let (ready_tx, ready_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let writer = thread::spawn(move || {
        app.with_conn(|c| {
            let tx = c.transaction()?;
            tx.execute("UPDATE teams SET name='Uncommitted'", [])?;
            ready_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            tx.commit()?;
            Ok(())
        })
        .unwrap()
    });
    ready_rx.recv_timeout(Duration::from_secs(3)).unwrap();
    // This uses a distinct read connection, without waiting for app's writer.
    let reader = cli.clone();
    let (read_tx, read_rx) = mpsc::channel();
    let read = thread::spawn(move || {
        read_tx
            .send(reader.with_read(|c| {
                Ok(c.query_row("SELECT name FROM teams", [], |r| r.get::<_, String>(0))?)
            }))
            .unwrap();
    });
    let name = read_rx
        .recv_timeout(Duration::from_secs(1))
        .expect("reader blocked by writer")
        .unwrap();
    assert_eq!(name, "Before");
    read.join().unwrap();
    let (attempt_tx, attempt_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let waiting = thread::spawn(move || {
        attempt_tx.send(()).unwrap();
        let result = cli.with_conn(|c| {
            c.execute("UPDATE teams SET name='After'", [])?;
            Ok(())
        });
        done_tx.send(result).unwrap();
    });
    attempt_rx.recv().unwrap();
    assert!(
        done_rx.recv_timeout(Duration::from_millis(100)).is_err(),
        "write did not wait"
    );
    release_tx.send(()).unwrap();
    writer.join().unwrap();
    done_rx
        .recv_timeout(Duration::from_secs(3))
        .unwrap()
        .unwrap();
    waiting.join().unwrap();
    let check = Store::open(&path).unwrap();
    assert_eq!(
        check
            .with_read(
                |c| Ok(c.query_row("SELECT name FROM teams", [], |r| r.get::<_, String>(0))?)
            )
            .unwrap(),
        "After"
    );
}

#[test]
fn opening_and_migrating_waits_for_active_writer() {
    use flek_core::store::migrations;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old.db");
    let mut app = rusqlite::Connection::open(&path).unwrap();
    app.pragma_update(None, "journal_mode", "WAL").unwrap();
    migrations::migrate_to(&mut app, migrations::latest_version() - 1).unwrap();
    let tx = app
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .unwrap();
    let (done_tx, done_rx) = mpsc::channel();
    let opener = thread::spawn(move || {
        done_tx
            .send(Store::open(&path).map(|s| s.schema_version().unwrap()))
            .unwrap();
    });
    assert!(done_rx.recv_timeout(Duration::from_millis(100)).is_err());
    tx.commit().unwrap();
    assert_eq!(
        done_rx
            .recv_timeout(Duration::from_secs(3))
            .unwrap()
            .unwrap(),
        migrations::latest_version()
    );
    opener.join().unwrap();
}

#[test]
fn fully_migrated_store_opens_without_waiting_for_writer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("current.db");
    let app = Store::open(&path).unwrap();
    app.with_conn(|c| {
        let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        tx.execute(
            "INSERT INTO teams (id,name) VALUES ('T1','Uncommitted')",
            [],
        )?;
        // Keep the writer locked through the whole open/read, with a wait
        // budget short enough to fail any attempt at a migration write lock.
        let cli = Store::open_with_busy_timeout(&path, Duration::from_millis(25))?;
        let count = cli.with_read(|c| {
            Ok(c.query_row("SELECT COUNT(*) FROM teams", [], |r| r.get::<_, i64>(0))?)
        })?;
        assert_eq!(count, 0);
        assert_eq!(
            cli.schema_version()?,
            flek_core::store::migrations::latest_version()
        );
        cli.with_conn(|c| {
            assert_eq!(
                c.query_row("PRAGMA busy_timeout", [], |r| r.get::<_, i64>(0))?,
                25
            );
            Ok(())
        })?;
        tx.commit()?;
        Ok(())
    })
    .unwrap();
}

#[test]
fn concurrent_first_open_applies_migrations_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new.db");
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = (0..2)
        .map(|_| {
            let path = path.clone();
            let barrier = barrier.clone();
            thread::spawn(move || {
                barrier.wait();
                Store::open(&path).unwrap().schema_version().unwrap()
            })
        })
        .collect();
    for h in handles {
        assert_eq!(
            h.join().unwrap(),
            flek_core::store::migrations::latest_version()
        );
    }
}
