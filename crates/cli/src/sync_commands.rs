//! Sync dispatch, progress and live terminal output.

use crate::format_ts;
use crate::output;
use crate::{open_db, output::emit, Global};
use anyhow::Result;
use flek_core::{
    auth::{self, CredentialStore},
    references::{self, Match},
};
use serde_json::json;

/// Runs the sync engine once, or with `follow` until Ctrl+C.
/// `files`: `None` skips downloads, `Some(mb)` caps the size (0 = no cap).
pub(super) async fn sync(
    creds: &CredentialStore,
    g: &Global,
    only: Option<&str>,
    files: Option<u64>,
    refresh_days: u32,
    full: bool,
    follow: bool,
) -> Result<()> {
    use flek_core::sync::engine::{Engine, EngineOptions, Priority};
    use flek_core::sync::live::{bus, Change, ConnectionState, Progress};

    let db = open_db(g)?;
    let mut settings = flek_core::settings::Settings::safe_defaults();
    settings.reload(&flek_core::settings::default_path()?);
    let session = auth::resume_session(
        creds,
        Some(&db),
        g.team.as_deref(),
        settings.desktop_import_on_start,
    )
    .await?;
    let me = session.identity.clone();

    let only = match only {
        Some(q) => {
            let team = me.team_id.clone();
            let mut found = db.with_read(|c| references::resolve(c, &team, q, Match::Exact))?;
            if !matches!(found, references::Resolution::Found(_)) {
                // Only a missing local identity needs directory discovery before startup.
                flek_core::sync::directory::sync_directory(&session.client()?, &db, &me).await?;
                found = db.with_read(|c| references::resolve(c, &team, q, Match::Exact))?;
            }
            let found = found.required(q)?;
            Some(vec![found.id])
        }
        None => None,
    };
    let files = match files {
        Some(mb) => Some((
            flek_core::files::default_cache_dir()?,
            (mb > 0).then(|| i64::try_from(mb * 1024 * 1024).unwrap_or(i64::MAX)),
        )),
        None => None,
    };

    let bus = bus();
    let mut events = bus.subscribe();
    let engine = Engine {
        client: session.client()?,
        store: db.clone(),
        me: me.clone(),
        bus: bus.clone(),
        priority: Priority::default(),
        options: EngineOptions {
            refresh_days,
            full,
            files,
            only,
            follow,
            ..Default::default()
        },
    };

    let (stop_tx, stop_rx) = shutdown_signal(tokio::signal::ctrl_c());

    let bar = if g.json {
        indicatif::ProgressBar::hidden()
    } else {
        progress_bar(0, "{spinner} [{bar:30}] {pos}/{len} {wide_msg}")
    };
    let printer = async {
        loop {
            let change = match events.recv().await {
                Ok(c) => c,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(_) => break,
            };
            if g.json {
                if follow {
                    if let Some(value) = output::event(&db, &change)? {
                        emit(value);
                    }
                }
                continue;
            }
            match change {
                Change::Progress(Progress::Directory) => bar.set_message("users and conversations"),
                Change::Progress(Progress::Conversations {
                    done,
                    total,
                    current,
                    deep,
                }) => {
                    bar.set_length(total as u64);
                    bar.set_position(done as u64);
                    bar.set_message(if deep {
                        format!("{current} (older history)")
                    } else {
                        current
                    });
                }
                Change::Progress(Progress::Files { done, total }) => {
                    bar.set_length(total as u64);
                    bar.set_position(done as u64);
                    bar.set_message("files");
                }
                Change::Progress(Progress::Idle) => {
                    bar.finish_and_clear();
                    if follow {
                        eprintln!("up to date; listening for new messages (Ctrl+C to stop)");
                    }
                }
                Change::Connection(ConnectionState::SyncIncomplete { in_secs }) => {
                    say(
                        &bar,
                        format!("connected; sync incomplete, retrying in {in_secs}s"),
                    );
                }
                Change::Connection(ConnectionState::Reconnecting { in_secs }) => {
                    say(&bar, format!("connection lost, reconnecting in {in_secs}s"));
                }
                Change::Message {
                    conversation_id,
                    ts,
                } => {
                    if let Ok(Some(line)) = db.with_read(|c| live_line(c, &conversation_id, &ts)) {
                        say(&bar, line);
                    }
                }
                _ => {}
            }
        }
        Ok::<(), anyhow::Error>(())
    };

    let stats = tokio::select! {
        r = engine.run(stop_rx) => r?,
        // Only if every sender is gone, which cannot happen while `bus` lives.
        result = printer => return result,
    };
    // Retain a sender through engine shutdown even if signal registration failed.
    drop(stop_tx);
    bar.finish_and_clear();
    if g.json && follow {
        // Flush queued notifications before the terminal summary on shutdown.
        while let Ok(change) = events.try_recv() {
            if let Some(value) = output::event(&db, &change)? {
                emit(value);
            }
        }
    }

    if g.json {
        if follow {
            emit(json!({"type":"summary","summary":stats}));
        } else {
            emit(json!({"summary":stats}));
        }
    } else {
        println!(
            "done: {} messages fetched, {} deleted, {} conversations, {} failed, {} files downloaded",
            stats.messages,
            stats.deleted,
            stats.conversations,
            stats.failed,
            stats.files_downloaded
        );
    }
    Ok(())
}

fn shutdown_signal(
    signal: impl std::future::Future<Output = std::io::Result<()>> + Send + 'static,
) -> (
    tokio::sync::watch::Sender<bool>,
    tokio::sync::watch::Receiver<bool>,
) {
    let (keepalive, receiver) = tokio::sync::watch::channel(false);
    let signal_stop = keepalive.clone();
    tokio::spawn(async move {
        match signal.await {
            Ok(()) => {
                let _ = signal_stop.send(true);
            }
            Err(error) => eprintln!("warning: could not listen for Ctrl+C: {error}"),
        }
    });
    (keepalive, receiver)
}

pub(super) fn live_line(
    c: &rusqlite::Connection,
    conversation_id: &str,
    ts: &str,
) -> flek_core::Result<Option<String>> {
    let Some(row) = flek_core::store::messages::thread(c, conversation_id, ts)?
        .into_iter()
        .find(|r| r.ts == ts)
    else {
        return Ok(None);
    };
    let label: String = c
        .query_row(
            "
                SELECT COALESCE(label, name, id)
                FROM conversations
                WHERE id = ?1
            ",
            [conversation_id],
            |r| r.get(0),
        )
        .unwrap_or_else(|_| conversation_id.to_owned());
    let (_, time) = format_ts(ts);
    let author = row.author.or(row.user_id).unwrap_or_else(|| "?".into());
    Ok(Some(format!(
        "{label} {time} {author}: {}",
        row.text.replace('\n', " ")
    )))
}

/// Progress on stderr; hidden automatically when stderr is not a terminal.
pub(super) fn progress_bar(len: u64, template: &str) -> indicatif::ProgressBar {
    let bar = indicatif::ProgressBar::new(len);
    if let Ok(style) = indicatif::ProgressStyle::with_template(template) {
        bar.set_style(style.progress_chars("=> "));
    }
    bar
}

/// A log line that does not tear the progress bar.
pub(super) fn say(bar: &indicatif::ProgressBar, line: String) {
    if bar.is_hidden() {
        eprintln!("{line}");
    } else {
        bar.println(line);
    }
}

#[cfg(test)]
mod tests {
    use super::shutdown_signal;
    use std::time::Duration;

    #[tokio::test]
    async fn a_failed_ctrl_c_listener_does_not_close_the_shutdown_channel() {
        // M9.14.9: a closed channel makes `changed()` return at once, forever.
        let (stop, mut stopped) =
            shutdown_signal(async { Err(std::io::Error::other("no signal handler")) });
        let waited = tokio::time::timeout(Duration::from_millis(100), stopped.changed()).await;
        assert!(waited.is_err(), "shutdown must stay pending");
        stop.send(true).unwrap();
        stopped.changed().await.unwrap();
        assert!(*stopped.borrow());
    }

    #[tokio::test]
    async fn ctrl_c_requests_shutdown() {
        let (_stop, mut stopped) = shutdown_signal(async { Ok(()) });
        stopped.changed().await.unwrap();
        assert!(*stopped.borrow());
    }
}
