//! `flek`: command-line client on top of `flek-core`.

mod output;

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use flek_core::{
    references::{self, Match, Resolution},
    views,
};
use output::emit;
use std::path::PathBuf;

use flek_core::auth::{self, CredentialStore, ImportedTeam, Session};
use flek_core::store::{self, Store};
use serde_json::json;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "flek", version, about = "Local-first Slack client")]
struct Cli {
    #[command(flatten)]
    global: Global,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Global {
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,
    /// More logging (-v info, -vv debug, -vvv trace). RUST_LOG overrides.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Team id (offline: the only team in the database; online: stored session).
    #[arg(long, global = true, value_name = "TEAM_ID")]
    team: Option<String>,
    /// Database file (default: ~/Library/Application Support/flek/flek.db).
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Manage stored sessions.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Show the current team and user (imports from Slack desktop if needed).
    Whoami,
    /// Open (create, migrate) the local database and show where it lives.
    Db,
    /// Fetch users, conversations and message history into the local database.
    Sync {
        /// Only this conversation (name, #name or id).
        #[arg(long)]
        channel: Option<String>,
        /// Skip downloading files into the local cache.
        #[arg(long)]
        no_files: bool,
        /// Largest file to download, in MB (0 = no limit).
        #[arg(long, default_value_t = 100, value_name = "MB")]
        max_file_size: u64,
        /// Re-read the last N days for edits, reactions, deletions and thread replies (0 = off).
        #[arg(long, default_value_t = 7, value_name = "DAYS")]
        refresh_days: u32,
        /// Re-read the whole history and every thread, merging without loss.
        #[arg(long)]
        full: bool,
        /// Stay connected and print new messages as they arrive (Ctrl+C to stop).
        #[arg(long)]
        follow: bool,
    },
    /// Show recent messages of a conversation from the local database.
    History {
        /// Conversation name, #name, person for a DM, or id.
        channel: String,
        /// How many messages.
        #[arg(long, default_value_t = 30)]
        limit: usize,
        /// Show one thread (root ts, as printed next to "replies").
        #[arg(long, value_name = "TS")]
        thread: Option<String>,
        /// Catch up this conversation before reading (requires a session).
        #[arg(long)]
        fresh: bool,
    },
    /// Search the local archive (works offline, includes messages Slack hides).
    ///
    /// Filters: from:<person> in:<channel> has:link has:file before:YYYY-MM-DD after:YYYY-MM-DD.
    /// Words: word, prefix*, "exact phrase", -excluded.
    Search {
        /// Query, e.g. `grades sheet in:#general after:2024-01-01`.
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        /// Maximum number of results.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Newest first instead of best match first.
        #[arg(long)]
        recent: bool,
    },
    /// Import an existing archive (simple JSON or Slack export / slackdump).
    Import {
        /// Archive file or directory.
        path: PathBuf,
        /// Put every message of a single-file archive into this conversation.
        #[arg(long)]
        channel: Option<String>,
    },
    /// Send a message as you (Slack mrkdwn; `&`, `<`, `>` are escaped).
    Send {
        /// Conversation name, #name, person for a DM, or id.
        channel: String,
        /// Message text.
        text: String,
        /// Reply in this thread (root ts, as `flek history` prints it).
        #[arg(long, value_name = "TS")]
        thread: Option<String>,
        /// With --thread: also show the reply in the channel.
        #[arg(long, requires = "thread")]
        broadcast: bool,
    },
    /// List conversations from the local database.
    Channels {
        /// Include archived channels.
        #[arg(long)]
        archived: bool,
    },
}

#[derive(Subcommand)]
enum AuthCommand {
    /// Import the session from the Slack desktop app into the Keychain.
    Import,
    /// List teams with stored credentials.
    List,
    /// Remove stored credentials from the Keychain.
    Logout {
        /// Remove every stored team.
        #[arg(long)]
        all: bool,
    },
}

#[tokio::main]
async fn main() {
    let json_output = std::env::args().any(|a| a == "--json");
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if json_output && e.exit_code() != 0 => {
            let error = anyhow::Error::new(flek_core::Error::Usage(
                e.render()
                    .to_string()
                    .lines()
                    .next()
                    .unwrap_or("invalid arguments")
                    .to_owned(),
            ));
            std::process::exit(output::report(&error, true));
        }
        Err(e) => e.exit(),
    };
    let json_output = cli.global.json;
    if let Err(error) = run(cli).await {
        std::process::exit(output::report(&error, json_output));
    }
}

async fn run(cli: Cli) -> Result<()> {
    init_tracing(cli.global.verbose, cli.global.json);
    let store = CredentialStore::keychain();
    let g = &cli.global;

    match cli.command {
        Command::Whoami => whoami(&store, g).await,
        Command::Db => db(g),
        Command::Sync {
            channel,
            no_files,
            max_file_size,
            refresh_days,
            full,
            follow,
        } => {
            let max = (!no_files).then_some(max_file_size);
            sync(
                &store,
                g,
                channel.as_deref(),
                max,
                refresh_days,
                full,
                follow,
            )
            .await
        }
        Command::History {
            channel,
            limit,
            thread,
            fresh,
        } => history(&store, g, &channel, limit, thread.as_deref(), fresh).await,
        Command::Search {
            query,
            limit,
            recent,
        } => search(g, &query.join(" "), limit, recent),
        Command::Import { path, channel } => import_archive(g, &path, channel.as_deref()),
        Command::Send {
            channel,
            text,
            thread,
            broadcast,
        } => send(&store, g, &channel, text, thread, broadcast).await,
        Command::Channels { archived } => channels(g, archived),
        Command::Auth(AuthCommand::Import) => import(&store, g).await,
        Command::Auth(AuthCommand::List) => list(&store, g),
        Command::Auth(AuthCommand::Logout { all }) => logout(&store, g, all),
    }
}

fn init_tracing(verbose: u8, json: bool) {
    let default = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = if json {
        EnvFilter::new("off")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default))
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
}

async fn whoami(store: &CredentialStore, g: &Global) -> Result<()> {
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

fn open_db(g: &Global) -> Result<Store> {
    let path = match &g.db {
        Some(p) => p.clone(),
        None => store::default_path()?,
    };
    Store::open(&path).with_context(|| format!("opening database {}", path.display()))
}

fn db(g: &Global) -> Result<()> {
    let db = open_db(g)?;
    let path = db
        .path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let version = db.schema_version()?;
    if g.json {
        emit(json!({"path":path,"db_schema_version":version}));
    } else {
        println!("path    {path}");
        println!("schema  v{version}");
    }
    Ok(())
}

/// Runs the sync engine once, or with `follow` until Ctrl+C.
/// `files`: `None` skips downloads, `Some(mb)` caps the size (0 = no cap).
async fn sync(
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

/// Posts one message and stores Slack's copy of it.
async fn send(
    creds: &CredentialStore,
    g: &Global,
    query: &str,
    text: String,
    thread: Option<String>,
    broadcast: bool,
) -> Result<()> {
    use flek_core::sync::send::{send, Outgoing};

    let session = auth::resolve_session(creds, g.team.as_deref()).await?;
    let db = open_db(g)?;
    let me = session.identity.clone();
    let client = session.client()?;
    let find =
        |db: &Store| db.with_read(|c| references::resolve(c, &me.team_id, query, Match::Exact));
    let conv = match find(&db)? {
        Resolution::Found(c) => c,
        Resolution::NotFound => {
            // Not stored yet (new DM, fresh database): fetch the directory.
            flek_core::sync::directory::sync_directory(&client, &db, &me).await?;
            find(&db)?.required(query)?
        }
        other => other.required(query)?,
    };
    let sent = send(
        &client,
        &db,
        &me.user_id,
        Outgoing {
            conversation_id: conv.id.clone(),
            blocks: None,
            text,
            thread_ts: thread,
            broadcast,
            client_msg_id: None,
        },
    )
    .await
    .map_err(|e| {
        let description = flek_core::api::chat::describe_send_error(&e);
        anyhow::Error::new(e).context(description)
    })
    .with_context(|| format!("sending to {}", conv.label))?;
    if g.json {
        let message = db
            .with_read(|c| views::message(c, &conv.id, &sent.ts))?
            .ok_or_else(|| {
                flek_core::Error::NotFound(format!(
                    "no stored message {} in conversation {}",
                    sent.ts, conv.id
                ))
            })?;
        emit(json!({"message":message}));
    } else {
        println!("sent to {} (ts {})", conv.label, sent.ts);
    }
    Ok(())
}

/// `#channel 14:03 Author: text` for a stored message.
fn live_line(
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
            "SELECT COALESCE(label, name, id) FROM conversations WHERE id = ?1",
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
fn progress_bar(len: u64, template: &str) -> indicatif::ProgressBar {
    let bar = indicatif::ProgressBar::new(len);
    if let Ok(style) = indicatif::ProgressStyle::with_template(template) {
        bar.set_style(style.progress_chars("=> "));
    }
    bar
}

/// A log line that does not tear the progress bar.
fn say(bar: &indicatif::ProgressBar, line: String) {
    if bar.is_hidden() {
        eprintln!("{line}");
    } else {
        bar.println(line);
    }
}

fn search(g: &Global, query: &str, limit: usize, recent: bool) -> Result<()> {
    use flek_core::store::search::{self, Sort};
    use std::io::IsTerminal;

    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    let q = search::parse(query).map_err(|e| flek_core::Error::Usage(e.to_string()))?;
    let sort = if recent {
        Sort::Recent
    } else {
        Sort::Relevance
    };
    let markers = if g.json {
        ("**", "**")
    } else if std::io::stdout().is_terminal() {
        ("\x1b[1;33m", "\x1b[0m")
    } else {
        ("", "")
    };
    let hits = db.with_read(|c| search::search(c, &team, &q, sort, limit, markers))?;
    if g.json {
        let results = db.with_read(|c| views::search_results(c, &hits))?;
        emit(json!({"results":results}));
        return Ok(());
    }
    if hits.is_empty() {
        println!("nothing found");
    }
    for h in hits {
        let (day, time) = format_ts(&h.ts);
        let author = h.author.unwrap_or_else(|| "?".into());
        let hidden = if h.hidden_by_limit {
            " [hidden in Slack]"
        } else {
            ""
        };
        let snippet = h.snippet.replace('\n', " ");
        println!(
            "{} {day} {time}  {author}: {snippet}{hidden}",
            h.conversation
        );
        if let Some(t) = h.thread_ts.filter(|t| *t != h.ts) {
            println!(
                "    in thread: flek history {} --thread {t}",
                h.conversation_id
            );
        }
    }
    Ok(())
}

fn import_archive(g: &Global, path: &std::path::Path, channel: Option<&str>) -> Result<()> {
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    let stats = db
        .with_conn(|c| flek_core::sync::import::import_path(c, &team, path, channel))
        .with_context(|| format!("importing {}", path.display()))?;
    if g.json {
        emit(json!({"summary":stats}));
        return Ok(());
    }
    println!(
        "{} archive: {} messages read, {} added or recovered",
        stats.format, stats.read, stats.written
    );
    if !stats.unknown_channels.is_empty() {
        println!(
            "skipped (no such conversation, run `flek sync` or pass --channel): {}",
            stats.unknown_channels.join(", ")
        );
    }
    Ok(())
}

fn default_team(db: &Store, g: &Global) -> Result<String> {
    if let Some(team) = &g.team {
        return Ok(team.clone());
    }
    let teams = db.with_read(|c| {
        let mut stmt = c.prepare("SELECT id FROM teams ORDER BY id")?;
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

async fn history(
    creds: &CredentialStore,
    g: &Global,
    query: &str,
    limit: usize,
    thread: Option<&str>,
    fresh: bool,
) -> Result<()> {
    use flek_core::store::messages;

    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    let conv = db
        .with_read(|c| references::resolve(c, &team, query, Match::AllowPrefix))?
        .required(query)?;
    if fresh {
        let session = auth::resolve_session(creds, Some(&team)).await?;
        flek_core::sync::fresh_conversation(&session.client()?, &db, &conv.id, thread).await?;
    }
    if g.json {
        let (conversation, messages) = db.with_read(|c| {
            let conversation = views::conversation(c, &conv.id)?
                .ok_or_else(|| references::conversation_not_found(query))?;
            let messages = views::history(c, &conv.id, limit, thread)?;
            Ok((conversation, messages))
        })?;
        emit(json!({"conversation":conversation,"messages":messages}));
        return Ok(());
    }
    let rows = match thread {
        Some(ts) => db.with_read(|c| messages::thread(c, &conv.id, ts))?,
        None => db.with_read(|c| messages::recent(c, &conv.id, limit))?,
    };
    let mut last_day = String::new();
    for r in rows {
        let (day, time) = format_ts(&r.ts);
        if day != last_day {
            println!("── {day} ──");
            last_day = day;
        }
        let author = r.author.or(r.user_id).unwrap_or_else(|| "?".into());
        let text = if r.hidden_by_limit {
            "[hidden by Slack Free limit]".to_owned()
        } else {
            r.text
        };
        let mut extra = String::new();
        if r.edited {
            extra.push_str(" (edited)");
        }
        if let Some(reactions) = &r.reactions {
            extra.push_str(&format!("  {reactions}"));
        }
        if r.reply_count > 0 && thread.is_none() {
            extra.push_str(&format!(" [{} replies: --thread {}]", r.reply_count, r.ts));
        }
        println!("{time} {author}: {text}{extra}");
    }
    Ok(())
}

/// Slack ts → local ("2026-11-08", "14:03").
fn format_ts(ts: &str) -> (String, String) {
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

fn channels(g: &Global, archived: bool) -> Result<()> {
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    let rows: Vec<_> = db
        .with_read(|c| flek_core::store::directory::list_conversations(c, &team))?
        .into_iter()
        .filter(|r| archived || !r.is_archived)
        .collect();
    if g.json {
        let conversations = db.with_read(|c| views::conversations(c, &team, archived))?;
        emit(json!({"conversations":conversations}));
        return Ok(());
    }
    if rows.is_empty() {
        println!("no conversations stored; run `flek sync`");
    }
    for r in rows {
        let prefix = match r.kind.as_str() {
            "public_channel" => "#",
            "private_channel" => "🔒",
            "mpim" => "👥",
            _ => "@",
        };
        let archived = if r.is_archived { "  (archived)" } else { "" };
        let unread = match r.unread {
            Some(0) | None => String::new(),
            Some(n) => format!("  ({n} unread)"),
        };
        println!(
            "{prefix} {}{archived}  {} msgs{unread}",
            r.label, r.messages
        );
    }
    Ok(())
}

async fn import(store: &CredentialStore, g: &Global) -> Result<()> {
    let teams = auth::import_and_save(store, g.team.as_deref())
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

fn import_row(t: &ImportedTeam) -> serde_json::Value {
    match &t.result {
        Ok(s) => {
            json!({ "team_id": t.team_id, "name": t.name, "ok": true, "user": s.identity.user, "user_id": s.identity.user_id })
        }
        Err(e) => {
            json!({ "team_id": t.team_id, "name": t.name, "ok": false, "error": output::safe_message(&e.to_string()) })
        }
    }
}

fn list(store: &CredentialStore, g: &Global) -> Result<()> {
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

fn logout(store: &CredentialStore, g: &Global, all: bool) -> Result<()> {
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
