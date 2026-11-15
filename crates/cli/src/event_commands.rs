//! JSON stream adapters. Matching, durable cursors and reply selection live in core.

use crate::{default_team, open_db, Global};
use anyhow::Result;
use clap::Args;
use flek_core::{
    auth::{self, CredentialStore},
    references::{self, Match},
    sync::{
        engine::{Engine, EngineOptions, Priority},
        events::{Filter, ReplyTarget, Stream},
        live,
    },
};
use serde_json::{json, Value};
use std::{io::Write, time::Duration};

#[derive(Args)]
pub(super) struct EventOptions {
    /// Only these conversations (repeatable; names, #names or ids).
    #[arg(long = "channel", value_name = "REFERENCE")]
    channels: Vec<String>,
    /// Only events on messages mentioning you directly (excludes your messages).
    #[arg(long)]
    mentions: bool,
    /// Only personal or group DMs.
    #[arg(long)]
    dm: bool,
    /// Replay stored events after this Slack timestamp, then continue live.
    #[arg(long, value_name = "TS")]
    since: Option<String>,
}

fn emit(mut value: Value) -> Result<()> {
    value["schema_version"] = json!(flek_core::views::SCHEMA_VERSION);
    let mut stdout = std::io::stdout().lock();
    writeln!(stdout, "{value}")?;
    stdout.flush()?;
    Ok(())
}

fn pipe_closed(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<std::io::Error>()
        .is_some_and(|e| e.kind() == std::io::ErrorKind::BrokenPipe)
}

pub(super) async fn events(creds: &CredentialStore, g: &Global, o: EventOptions) -> Result<()> {
    let since = o
        .since
        .as_deref()
        .map(|ts| {
            if !references::valid_ts(ts) {
                return Err(flek_core::Error::Usage(
                    "--since requires a Slack timestamp".into(),
                ));
            }
            flek_core::time::parse(ts)
        })
        .transpose()?;
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    let channels = db.with_read(|c| {
        o.channels
            .iter()
            .map(|q| {
                Ok(references::resolve(c, &team, q, Match::Exact)?
                    .required(q)?
                    .id)
            })
            .collect::<flek_core::Result<Vec<_>>>()
    })?;
    db.with_read(|c| flek_core::store::events::self_id(c, &team))?;
    let mut stream = Stream::open(
        db.clone(),
        team.clone(),
        Filter {
            channels: channels.clone(),
            mentions: o.mentions,
            dm: o.dm,
            since,
        },
    )
    .await?;
    // Keep the sender alive even if ctrl_c registration fails (M9.14.9). The
    // same channel stops the engine when the printer ends (broken pipe, error).
    let (stop, shutdown) = crate::sync_commands::shutdown_signal(tokio::signal::ctrl_c());
    let mut printer_stop = shutdown.clone();
    let result = async {
        while let Some(batch) = stream.replay().await? {
            for event in batch {
                emit(serde_json::to_value(event)?)?;
            }
        }
        let session = auth::resolve_session(creds, Some(&team)).await?;
        let engine = Engine {
            client: session.client()?,
            store: db,
            me: session.identity,
            bus: live::bus(),
            priority: Priority::default(),
            options: EngineOptions {
                only: (!channels.is_empty()).then_some(channels),
                follow: true,
                ..Default::default()
            },
        };
        let engine = engine.run(shutdown);
        tokio::pin!(engine);
        let mut engine_done = false;
        let printed = {
            let printer = async {
                loop {
                    let batch = tokio::select! {
                        _ = printer_stop.changed() => break,
                        batch = stream.next() => batch?,
                    };
                    for event in batch {
                        emit(serde_json::to_value(event)?)?;
                    }
                }
                Ok::<(), anyhow::Error>(())
            };
            tokio::pin!(printer);
            tokio::select! {
                r = &mut engine => { engine_done = true; r?; Ok(()) }
                r = &mut printer => r,
            }
        };
        let _ = stop.send(true);
        if !engine_done {
            engine.await?;
        }
        printed?;
        // Ctrl-C: flush what the engine committed before it stopped.
        while let batch @ [_, ..] = stream.poll().await?.as_slice() {
            for event in batch {
                emit(serde_json::to_value(event)?)?;
            }
        }
        Ok(())
    };
    match result.await {
        Err(e) if pipe_closed(&e) => Ok(()),
        r => r,
    }
}

pub(super) async fn wait(
    creds: &CredentialStore,
    g: &Global,
    reply_to: Vec<String>,
    timeout: Option<u64>,
) -> Result<()> {
    let deadline_at = timeout
        .map(|seconds| {
            tokio::time::Instant::now()
                .checked_add(Duration::from_secs(seconds))
                .ok_or_else(|| flek_core::Error::Usage("--timeout is too large".into()))
        })
        .transpose()?;
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    // Preserve conversation labels containing spaces; also accept "channel ts".
    let (reference, ts) = match reply_to.as_slice() {
        [reference] if references::parse_permalink(reference).is_some() => {
            (reference.as_str(), None)
        }
        [reference] => reference
            .trim()
            .rsplit_once(char::is_whitespace)
            .map_or((reference.as_str(), None), |(channel, ts)| {
                (channel.trim(), Some(ts))
            }),
        [reference, ts] => (reference.as_str(), Some(ts.as_str())),
        _ => {
            return Err(flek_core::Error::Usage(
                "--reply-to needs a permalink or conversation and timestamp".into(),
            )
            .into())
        }
    };
    let target = db.with_read(|c| ReplyTarget::resolve(c, &team, reference, ts))?;
    let (_signal_keepalive, mut shutdown) =
        crate::sync_commands::shutdown_signal(tokio::signal::ctrl_c());
    let wait_shutdown = shutdown.clone();
    let work = async {
        // An already stored answer needs neither Keychain nor network.
        let message = match target.stored_reply(&db).await? {
            Some(message) => Some(message),
            None => {
                let session = auth::resolve_session(creds, Some(&team)).await?;
                target
                    .wait(
                        &session.client()?,
                        &db,
                        &session.identity,
                        &Default::default(),
                        deadline_at,
                        wait_shutdown,
                    )
                    .await?
            }
        };
        message.map_or(Ok(()), |message| emit(json!({"message":message})))
    };
    // The core wait owns the deadline once the network is involved; this outer
    // one also covers a slow Keychain/session lookup.
    let deadline = async {
        match deadline_at {
            Some(deadline) => tokio::time::sleep_until(deadline).await,
            None => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        biased;
        _ = shutdown.changed() => Ok(()),
        r = work => match r { Err(e) if pipe_closed(&e) => Ok(()), r => r },
        _ = deadline => Err(flek_core::Error::Timeout.into()),
    }
}
