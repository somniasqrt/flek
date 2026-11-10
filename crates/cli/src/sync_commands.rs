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

    let session = auth::resolve_session(creds, g.team.as_deref()).await?;
    let db = open_db(g)?;
    let me = session.identity.clone();

    let only = match only {
        Some(q) => {
            // The conversation may not be stored yet: fetch the directory first.
            flek_core::sync::directory::sync_directory(&session.client()?, &db, &me).await?;
            let team = me.team_id.clone();
            let found = db
                .with_read(|c| references::resolve(c, &team, q, Match::Exact))?
                .required(q)?;
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

    let (stop_tx, stop_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            let _ = stop_tx.send(true);
        }
    });

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
