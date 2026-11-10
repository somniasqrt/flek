//! Thin adapter for core agent search; credentials only for explicit --remote.
use super::*;
use flek_core::search;
use std::io::IsTerminal;

#[derive(Args)]
pub struct Options {
    #[arg(long, default_value_t = 20)]
    limit: usize,
    /// Newest first instead of relevance.
    #[arg(long)]
    recent: bool,
    #[arg(long)]
    since: Option<String>,
    #[arg(long)]
    until: Option<String>,
    #[arg(long, default_value_t = 0)]
    context: usize,
    #[arg(long, default_value_t = 0, conflicts_with = "cursor")]
    offset: usize,
    #[arg(long)]
    cursor: Option<String>,
    /// Search Slack too; persist matches before reading merged local results.
    #[arg(long)]
    remote: bool,
}
pub async fn run(creds: &CredentialStore, g: &Global, query: &str, options: Options) -> Result<()> {
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    let o = search::Options {
        limit: options.limit,
        recent: options.recent,
        since: options.since,
        until: options.until,
        context: options.context,
        offset: options.offset,
        cursor: options.cursor,
    };
    // Invalid references/cursors must fail before any session access.
    let prepared = db.with_read(|c| search::prepare(c, &team, query, &o))?;
    let client = if options.remote {
        Some(auth::resolve_session(creds, Some(&team)).await?.client()?)
    } else {
        None
    };
    let markers = if g.json {
        ("**", "**")
    } else if std::io::stdout().is_terminal() {
        ("\x1b[1;33m", "\x1b[0m")
    } else {
        ("", "")
    };
    let page = search::run(&db, &team, prepared, &o, client.as_ref(), markers).await?;
    if g.json {
        emit(serde_json::to_value(page)?);
    } else {
        if let Some(error) = page.remote.error {
            eprintln!("warning: {}", error.message);
        }
        if page.results.is_empty() {
            println!("nothing found");
        }
        for hit in page.results {
            read_commands::print_messages(&hit.before, None);
            // Markers are ANSI codes, so resolving Slack markup keeps them intact.
            let snippet = db.with_read(|c| views::resolve_text(c, &hit.result.snippet))?;
            let m = hit.result.message;
            let (day, time) = format_ts(&m.ts);
            println!(
                "{} {day} {time}  {}: {}{}",
                m.conversation,
                m.user.as_deref().or(m.user_id.as_deref()).unwrap_or("?"),
                snippet.replace('\n', " "),
                if m.hidden_by_limit {
                    " [hidden in Slack]"
                } else {
                    ""
                }
            );
            if let Some(root) = m.thread_ts {
                println!(
                    "    in thread: flek history {} --thread {root}",
                    m.conversation_id
                );
            }
            read_commands::print_messages(&hit.after, None);
        }
    }
    Ok(())
}
