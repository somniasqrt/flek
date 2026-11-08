//! `flek`: command-line client on top of `flek-core`.

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
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
    /// Team id to act on (defaults to the first stored team).
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
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.global.verbose);
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
        } => {
            let max = (!no_files).then_some(max_file_size);
            let opts = flek_core::sync::SyncOptions {
                full,
                ..flek_core::sync::SyncOptions::refresh_days(refresh_days)
            };
            sync(&store, g, channel.as_deref(), max, &opts).await
        }
        Command::History {
            channel,
            limit,
            thread,
        } => history(&store, g, &channel, limit, thread.as_deref()),
        Command::Search {
            query,
            limit,
            recent,
        } => search(&store, g, &query.join(" "), limit, recent),
        Command::Import { path, channel } => import_archive(&store, g, &path, channel.as_deref()),
        Command::Channels { archived } => channels(&store, g, archived),
        Command::Auth(AuthCommand::Import) => import(&store, g).await,
        Command::Auth(AuthCommand::List) => list(&store, g),
        Command::Auth(AuthCommand::Logout { all }) => logout(&store, g, all),
    }
}

fn init_tracing(verbose: u8) {
    let default = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
}

async fn whoami(store: &CredentialStore, g: &Global) -> Result<()> {
    let Session { identity, .. } = auth::resolve_session(store, g.team.as_deref()).await?;
    if g.json {
        println!("{}", serde_json::to_string_pretty(&identity)?);
    } else {
        println!("team  {} ({})", identity.team, identity.team_id);
        println!("user  {} ({})", identity.user, identity.user_id);
        println!("url   {}", identity.url);
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
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "path": path, "schema_version": version }))?
        );
    } else {
        println!("path    {path}");
        println!("schema  v{version}");
    }
    Ok(())
}

/// `files`: `None` skips downloads, `Some(mb)` caps the size (0 = no cap).
async fn sync(
    creds: &CredentialStore,
    g: &Global,
    only: Option<&str>,
    files: Option<u64>,
    opts: &flek_core::sync::SyncOptions,
) -> Result<()> {
    use flek_core::store::directory;
    use flek_core::sync::backfill::{backfill_conversation, RateLimiter, HISTORY_INTERVAL};
    use flek_core::sync::threads::sync_threads;

    let session = auth::resolve_session(creds, g.team.as_deref()).await?;
    let db = open_db(g)?;
    let client = session.client()?;
    let me = &session.identity;

    let dir = flek_core::sync::directory::sync_directory(&client, &db, me).await?;
    eprintln!(
        "{}: {} users, {} conversations",
        me.team, dir.users, dir.conversations
    );

    let team = me.team_id.clone();
    let mut convs = db.with_conn(|c| directory::list_conversations(c, &team))?;
    if let Some(q) = only {
        let found = db
            .with_conn(|c| directory::find_conversation(c, &team, q))?
            .with_context(|| format!("no conversation matches {q:?}"))?;
        convs = vec![found];
    }

    let mut limiter = RateLimiter::new(HISTORY_INTERVAL);
    let (mut total, mut deleted, mut failed) = (0usize, 0usize, 0usize);
    let n = convs.len();
    let bar = progress_bar(n as u64, "{spinner} [{bar:30}] {pos}/{len} {wide_msg}");
    for conv in &convs {
        bar.set_message(conv.label.clone());
        let result = async {
            let b = backfill_conversation(&client, &db, &mut limiter, &conv.id, opts).await?;
            let t = sync_threads(&client, &db, &mut limiter, &conv.id, opts).await?;
            Ok::<_, flek_core::Error>((b, t))
        }
        .await;
        match result {
            Ok((b, t)) => {
                total += b.messages + t.replies;
                deleted += b.deleted + t.deleted;
                if b.messages + t.replies + b.deleted + t.deleted > 0 {
                    let mut line = format!("{}: +{} messages", conv.label, b.messages);
                    if t.threads > 0 {
                        line.push_str(&format!(", {} threads (+{} replies)", t.threads, t.replies));
                    }
                    if b.deleted + t.deleted > 0 {
                        line.push_str(&format!(", {} deleted", b.deleted + t.deleted));
                    }
                    say(&bar, line);
                }
            }
            Err(e) if e.is_auth_error() => {
                bar.abandon();
                return Err(e.into());
            }
            Err(e) => {
                failed += 1;
                say(&bar, format!("{}: {e}", conv.label));
            }
        }
        bar.inc(1);
    }
    bar.finish_and_clear();

    let mut file_stats = None;
    if let Some(mb) = files {
        let cache = flek_core::files::default_cache_dir()?;
        let max = (mb > 0).then(|| i64::try_from(mb * 1024 * 1024).unwrap_or(i64::MAX));
        let fbar = progress_bar(0, "files [{bar:30}] {pos}/{len}");
        let stats =
            flek_core::sync::files::download_files(&client, &db, &cache, max, |done, total| {
                fbar.set_length(total as u64);
                fbar.set_position(done as u64);
            })
            .await?;
        fbar.finish_and_clear();
        eprintln!(
            "files: {} downloaded, {} already cached, {} failed",
            stats.downloaded, stats.cached, stats.failed
        );
        file_stats = Some(stats);
    }

    if g.json {
        let out = json!({
            "users": dir.users, "conversations": dir.conversations, "new_messages": total,
            "deleted": deleted, "failed": failed, "files": file_stats
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else {
        println!(
            "done: {total} new messages, {deleted} deleted, {n} conversations, {failed} failed"
        );
    }
    Ok(())
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

fn search(
    creds: &CredentialStore,
    g: &Global,
    query: &str,
    limit: usize,
    recent: bool,
) -> Result<()> {
    use flek_core::store::search::{self, Sort};
    use std::io::IsTerminal;

    let team = default_team(creds, g)?;
    let db = open_db(g)?;
    let q = search::parse(query)?;
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
    let hits = db.with_conn(|c| search::search(c, &team, &q, sort, limit, markers))?;
    if g.json {
        println!("{}", serde_json::to_string_pretty(&hits)?);
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

fn import_archive(
    creds: &CredentialStore,
    g: &Global,
    path: &std::path::Path,
    channel: Option<&str>,
) -> Result<()> {
    let team = default_team(creds, g)?;
    let db = open_db(g)?;
    let stats = db
        .with_conn(|c| flek_core::sync::import::import_path(c, &team, path, channel))
        .with_context(|| format!("importing {}", path.display()))?;
    if g.json {
        println!("{}", serde_json::to_string_pretty(&stats)?);
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

fn default_team(creds: &CredentialStore, g: &Global) -> Result<String> {
    match &g.team {
        Some(t) => Ok(t.clone()),
        None => creds
            .list()?
            .into_iter()
            .next()
            .context("no stored team; run `flek auth import`"),
    }
}

fn history(
    creds: &CredentialStore,
    g: &Global,
    query: &str,
    limit: usize,
    thread: Option<&str>,
) -> Result<()> {
    use flek_core::store::{directory, messages};

    let team = default_team(creds, g)?;
    let db = open_db(g)?;
    let conv = db
        .with_conn(|c| directory::find_conversation(c, &team, query))?
        .with_context(|| format!("no conversation matches {query:?}; try `flek channels`"))?;
    let rows = match thread {
        Some(ts) => db.with_conn(|c| messages::thread(c, &conv.id, ts))?,
        None => db.with_conn(|c| messages::recent(c, &conv.id, limit))?,
    };
    if g.json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
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

fn channels(creds: &CredentialStore, g: &Global, archived: bool) -> Result<()> {
    let team = match &g.team {
        Some(t) => t.clone(),
        None => creds
            .list()?
            .into_iter()
            .next()
            .context("no stored team; run `flek auth import`")?,
    };
    let db = open_db(g)?;
    let rows: Vec<_> = db
        .with_conn(|c| flek_core::store::directory::list_conversations(c, &team))?
        .into_iter()
        .filter(|r| archived || !r.is_archived)
        .collect();
    if g.json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
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
        let rows: Vec<_> = teams.iter().map(import_row).collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        for t in &teams {
            match &t.result {
                Ok(s) => println!("saved   {} ({}) as {}", t.name, t.team_id, s.identity.user),
                Err(e) => println!("failed  {} ({}): {e}", t.name, t.team_id),
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
            json!({ "team_id": t.team_id, "name": t.name, "ok": false, "error": e.to_string() })
        }
    }
}

fn list(store: &CredentialStore, g: &Global) -> Result<()> {
    let ids = store.list()?;
    if g.json {
        println!("{}", serde_json::to_string_pretty(&ids)?);
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
        (None, false) => bail!(
            "several teams are stored ({}); pass --team or --all",
            stored.join(", ")
        ),
    };
    let mut removed = Vec::new();
    for id in &targets {
        if store.delete(id)? {
            removed.push(id.clone());
        }
    }
    if g.json {
        println!("{}", serde_json::to_string_pretty(&removed)?);
    } else if removed.is_empty() {
        println!("nothing to remove");
    } else {
        for id in removed {
            println!("removed {id}");
        }
    }
    Ok(())
}
