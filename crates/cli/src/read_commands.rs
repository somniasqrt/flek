//! Thin CLI adapters for the core's database read operations.
use super::*;
use flek_core::{reads, time};

#[derive(Args, Default)]
pub struct HistoryOptions {
    #[arg(long, default_value_t = 30)]
    pub limit: usize,
    #[arg(long)]
    pub thread: Option<String>,
    #[arg(long)]
    pub fresh: bool,
    #[arg(long)]
    pub since: Option<String>,
    #[arg(long)]
    pub until: Option<String>,
    #[arg(long, conflicts_with = "cursor")]
    pub around: Option<String>,
    #[arg(long)]
    pub cursor: Option<String>,
}
#[derive(Args)]
pub struct MessageRef {
    /// Slack permalink or conversation reference followed by a timestamp.
    reference: String,
    ts: Option<String>,
    #[arg(long)]
    fresh: bool,
}
#[derive(Subcommand)]
pub enum ReadCommand {
    /// List unread conversations (offline unless --fresh).
    Unread {
        #[arg(long, default_value_t = 0)]
        messages: usize,
        #[arg(long)]
        include_unknown: bool,
        #[arg(long)]
        fresh: bool,
    },
    /// Root and all stored replies; a reply reference resolves to its root.
    Thread(MessageRef),
    /// One message with adjacent messages from the same timeline.
    Message {
        #[command(flatten)]
        reference: MessageRef,
        #[arg(long, default_value_t = 0)]
        context: usize,
    },
    /// List stored people.
    Users {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        include_bots: bool,
        #[arg(long)]
        include_deleted: bool,
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Show one stored person's profile.
    User { reference: String },
    /// Read channel details.
    #[command(subcommand)]
    Channel(ChannelCommand),
    /// Locally stored mentions of me, newest first.
    Mentions {
        #[arg(long, default_value = "7d")]
        since: String,
        #[arg(long)]
        until: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
        #[arg(long)]
        include_broadcasts: bool,
    },
    /// List stored files with message provenance.
    Files {
        #[arg(long)]
        channel: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long = "type")]
        kind: Option<String>,
        #[arg(long)]
        since: Option<String>,
        #[arg(long)]
        until: Option<String>,
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Access cached files.
    #[command(subcommand)]
    File(FileCommand),
}
#[derive(Subcommand)]
pub enum ChannelCommand {
    /// Stored metadata; --fresh also fetches members and pins.
    Info {
        reference: String,
        #[arg(long)]
        fresh: bool,
    },
}
#[derive(Subcommand)]
pub enum FileCommand {
    /// Print the cache path, downloading only if missing.
    Get {
        id: String,
        #[arg(long)]
        to: Option<PathBuf>,
    },
}

fn conv(db: &Store, team: &str, q: &str) -> Result<String> {
    Ok(db
        .with_read(|c| references::resolve(c, team, q, Match::AllowPrefix))?
        .required(q)?
        .id)
}
fn times(since: Option<&str>, until: Option<&str>) -> Result<(Option<String>, Option<String>)> {
    let since = since.map(time::parse).transpose()?;
    let until = until.map(time::parse).transpose()?;
    if since
        .as_ref()
        .zip(until.as_ref())
        .is_some_and(|(s, u)| s > u)
    {
        return Err(flek_core::Error::Usage("--since must not exceed --until".into()).into());
    }
    Ok((since, until))
}
pub fn print_messages(messages: &[views::Message], target: Option<&str>) {
    let mut last = String::new();
    for m in messages {
        let (day, time) = format_ts(&m.ts);
        if last != day {
            println!("── {day} ──");
            last = day;
        }
        let marker = if target == Some(m.ts.as_str()) {
            "> "
        } else {
            ""
        };
        let text = if m.hidden_by_limit {
            "[hidden by Slack Free limit]"
        } else {
            &m.text_resolved
        };
        let mut extra = if m.reply_count > 0 {
            format!(" [{} replies: --thread {}]", m.reply_count, m.ts)
        } else {
            String::new()
        };
        if m.edited {
            extra.push_str(" (edited)");
        }
        for r in &m.reactions {
            extra.push_str(&format!("  :{}: {}", r.name, r.count));
        }
        println!(
            "{marker}{time} {}: {text}{extra}",
            m.user.as_deref().or(m.user_id.as_deref()).unwrap_or("?")
        );
    }
}
pub async fn history(
    creds: &CredentialStore,
    g: &Global,
    q: &str,
    o: HistoryOptions,
) -> Result<()> {
    let (since, until) = times(o.since.as_deref(), o.until.as_deref())?;
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    let id = conv(&db, &team, q)?;
    let around = match o.around {
        Some(a) => Some(if let Some(r) = references::parse_permalink(&a) {
            if r.conversation_id != id {
                return Err(flek_core::Error::Usage(
                    "--around permalink belongs to another conversation".into(),
                )
                .into());
            }
            r.ts
        } else {
            if !references::valid_ts(&a) {
                return Err(flek_core::Error::Usage("invalid --around timestamp".into()).into());
            }
            a
        }),
        None => None,
    };
    let w = reads::Window {
        since,
        until,
        around: around.clone(),
        cursor: o.cursor,
        thread: o.thread.clone(),
        limit: o.limit,
    };
    // Validate cursors and windows before any session access.
    db.with_read(|c| reads::history(c, &id, &w))?;
    if o.fresh {
        let session = auth::resolve_session(creds, Some(&team)).await?;
        flek_core::sync::fresh_conversation(&session.client()?, &db, &id, o.thread.as_deref())
            .await?;
    }
    let page = db.with_read(|c| reads::history(c, &id, &w))?;
    if g.json {
        let mut messages = serde_json::to_value(&page.messages)?;
        if let Some(target) = &around {
            for m in messages.as_array_mut().unwrap() {
                if m["ts"] == *target {
                    m["target"] = json!(true);
                }
            }
        }
        let conversation = db.with_read(|c| views::conversation(c, &id))?;
        emit(json!({
            "conversation": conversation,
            "messages": messages,
            "next_cursor": page.next_cursor,
            "prev_cursor": page.prev_cursor,
        }));
    } else {
        print_messages(&page.messages, around.as_deref());
    }
    Ok(())
}
pub async fn run(cmd: ReadCommand, creds: &CredentialStore, g: &Global) -> Result<()> {
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    match cmd {
        ReadCommand::Unread {
            messages,
            include_unknown,
            fresh,
        } => {
            let failed = if fresh {
                let session = auth::resolve_session(creds, Some(&team)).await?;
                flek_core::sync::fresh_unread(&session.client()?, &db, &session.identity)
                    .await?
                    .failed
            } else {
                0
            };
            let (known, unknown) = db.with_read(|c| reads::unread(c, &team, messages))?;
            let hidden_unknown = if include_unknown { 0 } else { unknown.len() };
            let unknown = if include_unknown { unknown } else { Vec::new() };
            if g.json {
                emit(json!({"conversations":known,"unknown":unknown,"failed":failed}));
            } else {
                if failed > 0 {
                    eprintln!(
                        "warning: {failed} conversations failed to refresh; showing stored data"
                    );
                }
                if known.is_empty() {
                    println!(
                        "no unread messages{}",
                        if fresh {
                            ""
                        } else {
                            " in stored data (add --fresh to check Slack)"
                        }
                    );
                }
                if hidden_unknown > 0 {
                    println!(
                        "{hidden_unknown} conversations have no read marker; \
                         --include-unknown lists them"
                    );
                }
                for u in known {
                    println!(
                        "{}  {} unread  {} mentions",
                        u.conversation.label,
                        u.conversation.unread_count.unwrap_or(0),
                        u.mentions
                    );
                    print_messages(&u.messages, None);
                }
                if !unknown.is_empty() {
                    println!("── unknown read markers ──");
                    for c in unknown {
                        println!("{}", c.label);
                    }
                }
            }
        }
        ReadCommand::Thread(reference) => {
            message_read(creds, g, &db, &team, reference, None).await?
        }
        ReadCommand::Message { reference, context } => {
            message_read(creds, g, &db, &team, reference, Some(context)).await?
        }
        ReadCommand::Users {
            query,
            include_bots,
            include_deleted,
            limit,
        } => {
            let users = db.with_read(|c| {
                reads::users(
                    c,
                    &team,
                    query.as_deref(),
                    include_bots,
                    include_deleted,
                    limit,
                )
            })?;
            if g.json {
                emit(json!({"users":users}));
            } else {
                for u in users {
                    print_user(&u);
                }
            }
        }
        ReadCommand::User { reference } => {
            let user = db.with_read(|c| {
                references::resolve_user(c, &team, &reference, Match::AllowPrefix)
            })?;
            if g.json {
                emit(json!({"user":user}));
            } else {
                print_user(&user);
                for (label, value) in [
                    ("title", user.title),
                    ("email", user.email),
                    ("status emoji", user.status_emoji),
                    ("timezone", user.tz),
                    ("timezone label", user.tz_label),
                    ("DM", user.dm_conversation_id),
                ] {
                    if let Some(value) = value {
                        println!("{label}: {value}");
                    }
                }
                if let Some(value) = user.status_expiration {
                    println!("status expiration: {value}");
                }
                if let Some(value) = user.is_admin {
                    println!("admin: {value}");
                }
                if let Some(value) = user.is_owner {
                    println!("owner: {value}");
                }
            }
        }
        ReadCommand::Channel(ChannelCommand::Info { reference, fresh }) => {
            let id = conv(&db, &team, &reference)?;
            if fresh {
                let session = auth::resolve_session(creds, Some(&team)).await?;
                flek_core::sync::channel_details::fresh(&session.client()?, &db, &id).await?;
            }
            let info = db.with_read(|c| reads::channel_info(c, &id))?;
            if g.json {
                emit(serde_json::to_value(info)?);
            } else {
                println!(
                    "{} ({})\ntopic: {}\npurpose: {}",
                    info.conversation.label,
                    id,
                    info.conversation.topic.as_deref().unwrap_or(""),
                    info.conversation.purpose.as_deref().unwrap_or("")
                );
                if let Some(created) = info.created {
                    println!("created: {created}");
                }
                if let Some(creator) = info.creator {
                    println!("creator: {creator}");
                }
                if let Some(count) = info.num_members {
                    println!("member count: {count}");
                }
                if let Some(users) = info.members {
                    println!("members:");
                    for u in users {
                        print_user(&u);
                    }
                }
                if let Some(pins) = info.pins {
                    println!("pins:");
                    print_messages(&pins, None);
                }
            }
        }
        ReadCommand::Mentions {
            since,
            until,
            limit,
            include_broadcasts,
        } => {
            let (since, until) = times(Some(&since), until.as_deref())?;
            let messages = db.with_read(|c| {
                reads::mentions(
                    c,
                    &team,
                    since.as_deref().unwrap(),
                    until.as_deref(),
                    include_broadcasts,
                    limit,
                )
            })?;
            if g.json {
                emit(json!({"messages":messages}));
            } else {
                for m in messages {
                    println!(
                        "{} {} {}: {}",
                        m.conversation,
                        m.ts,
                        m.user.as_deref().unwrap_or("?"),
                        m.text
                    );
                }
            }
        }
        ReadCommand::Files {
            channel,
            from,
            kind,
            since,
            until,
            limit,
        } => {
            let (since, until) = times(since.as_deref(), until.as_deref())?;
            let channel = channel.map(|q| conv(&db, &team, &q)).transpose()?;
            let user = from
                .map(|q| {
                    db.with_read(|c| references::resolve_user(c, &team, &q, Match::AllowPrefix))
                        .map(|u| u.id)
                })
                .transpose()?;
            let filter = reads::FileFilter {
                channel,
                user,
                kind,
                since,
                until,
                limit,
                ..Default::default()
            };
            let files = db.with_read(|c| reads::files(c, &team, &filter))?;
            if g.json {
                emit(json!({"files":files}));
            } else {
                for f in files {
                    println!(
                        "{} {} {}",
                        f.file.id,
                        f.file.name.as_deref().unwrap_or("?"),
                        f.file.local_path.as_deref().unwrap_or("[not cached]")
                    );
                }
            }
        }
        ReadCommand::File(FileCommand::Get { id, to }) => {
            db.with_read(|c| reads::file(c, &team, &id))?;
            let cache = flek_core::files::default_cache_dir()?;
            let path =
                if let Some(path) = flek_core::files::stored_cached_path(&db, &cache, &id).await? {
                    path
                } else {
                    // Reject missing/hidden URLs before resolving any credentials.
                    db.with_read(|c| reads::pending_file(c, &id))?;
                    let session = auth::resolve_session(creds, Some(&team)).await?;
                    flek_core::files::fetch_stored(&session.client()?, &db, &cache, &id).await?
                };
            let output = if let Some(to) = to {
                if to != path {
                    std::fs::copy(&path, &to)?;
                }
                to
            } else {
                path
            };
            if g.json {
                let file = db.with_read(|c| reads::file(c, &team, &id))?;
                emit(json!({"file":file,"path":output}));
            } else {
                println!("{}", output.display());
            }
        }
    }
    Ok(())
}
fn print_user(u: &views::User) {
    println!(
        "{} @{}  {}{}",
        u.id,
        u.name,
        u.display_name
            .as_deref()
            .or(u.real_name.as_deref())
            .unwrap_or(""),
        u.status_text
            .as_ref()
            .map(|s| format!("  {s}"))
            .unwrap_or_default()
    );
}
async fn message_read(
    creds: &CredentialStore,
    g: &Global,
    db: &Store,
    team: &str,
    r: MessageRef,
    context: Option<usize>,
) -> Result<()> {
    let reference =
        db.with_read(|c| references::message_reference(c, team, &r.reference, r.ts.as_deref()))?;
    // Scope permalinks to the selected team as well.
    let id = conv(db, team, &reference.conversation_id)?;
    if r.fresh {
        let session = auth::resolve_session(creds, Some(team)).await?;
        flek_core::sync::fresh_message_reference(
            &session.client()?,
            db,
            &reference,
            context.is_none(),
        )
        .await?;
    }
    if let Some(n) = context {
        let result = db
            .with_read(|c| reads::message(c, &id, &reference.ts, n))
            .map_err(|e| {
                if matches!(e, flek_core::Error::NotFound(_)) {
                    flek_core::Error::NotFound(format!(
                        "no stored message {:?} {}",
                        r.reference, reference.ts
                    ))
                } else {
                    e
                }
            })?;
        if g.json {
            emit(serde_json::to_value(result)?);
        } else {
            print_messages(&result.before, None);
            print_messages(&[result.message], Some(&reference.ts));
            print_messages(&result.after, None);
        }
    } else {
        let root = db.with_read(|c| reads::thread_root(c, &id, &reference.ts))?;
        let messages = db
            .with_read(|c| reads::thread(c, &id, &reference.ts))
            .map_err(|e| {
                if matches!(e, flek_core::Error::NotFound(_)) {
                    flek_core::Error::NotFound(format!(
                        "no stored thread {:?} {}",
                        r.reference, reference.ts
                    ))
                } else {
                    e
                }
            })?;
        if g.json {
            emit(
                json!({"conversation":db.with_read(|c|views::conversation(c,&id))?,"root_ts":root,"messages":messages}),
            );
        } else {
            print_messages(&messages, None);
        }
    }
    Ok(())
}
