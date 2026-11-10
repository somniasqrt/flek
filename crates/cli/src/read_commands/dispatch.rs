//! Offline read dispatch.
use super::*;

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
