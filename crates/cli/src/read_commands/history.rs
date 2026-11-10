//! Offline read history.
use super::*;

pub(super) fn conv(db: &Store, team: &str, q: &str) -> Result<String> {
    Ok(db
        .with_read(|c| references::resolve(c, team, q, Match::AllowPrefix))?
        .required(q)?
        .id)
}

pub(super) fn times(
    since: Option<&str>,
    until: Option<&str>,
) -> Result<(Option<String>, Option<String>)> {
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

pub(super) fn print_user(u: &views::User) {
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

pub(super) async fn message_read(
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
