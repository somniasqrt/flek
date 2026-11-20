//! Thin adapters for guarded writes. Every command prepares offline before auth.
use crate::{
    default_team, open_db,
    output::emit,
    send_commands::{self, MessageOptions},
    Global,
};
use anyhow::Result;
use clap::{Args, Subcommand};
use flek_core::{
    agent::{self, ActionLog, AgentConfig},
    auth::{self, CredentialStore},
    store::Store,
    writes::{MessageAction, PreparedWrite, WriteSession},
    Error,
};
use serde_json::json;
use std::path::PathBuf;

#[derive(Args)]
pub(super) struct MessageTarget {
    /// Message permalink, or exact channel name/id followed by a timestamp.
    reference: String,
    /// Message timestamp; omit when the reference is a permalink.
    ts: Option<String>,
}
#[derive(Subcommand)]
pub(super) enum WriteCommand {
    /// Replace the text of one of your own messages (Slack mrkdwn).
    Edit {
        /// Exact channel name or id, or a message permalink.
        reference: String,
        /// Message timestamp; omit when the reference is a permalink.
        ts: Option<String>,
        /// Replacement text (omit to read piped stdin; '-' explicitly reads stdin).
        #[arg(long, conflicts_with_all = ["file", "stdin"])]
        text: Option<String>,
        /// Read UTF-8 replacement text from a file.
        #[arg(long, conflicts_with = "stdin")]
        file: Option<PathBuf>,
        /// Read UTF-8 replacement text from stdin.
        #[arg(long)]
        stdin: bool,
    },
    /// Delete one of your own messages (not undoable).
    Delete {
        #[command(flatten)]
        target: MessageTarget,
    },
    /// Add one emoji reaction to a message.
    React {
        #[command(flatten)]
        target: MessageTarget,
        /// Emoji name without colons, e.g. `thumbsup` or `+1`.
        #[arg(long, value_name = "NAME")]
        emoji: String,
    },
    /// Remove your emoji reaction from a message.
    Unreact {
        #[command(flatten)]
        target: MessageTarget,
        /// Emoji name without colons, e.g. `thumbsup` or `+1`.
        #[arg(long, value_name = "NAME")]
        emoji: String,
    },
    /// Advance the conversation's read marker to this message.
    MarkRead {
        #[command(flatten)]
        target: MessageTarget,
    },
    /// Pin one message to its conversation.
    Pin {
        #[command(flatten)]
        target: MessageTarget,
    },
    /// Unpin one message from its conversation.
    Unpin {
        #[command(flatten)]
        target: MessageTarget,
    },
    /// Add a legacy Slack star (not visible in modern Later; API limitation).
    Save {
        #[command(flatten)]
        target: MessageTarget,
    },
    /// Forward a message by posting its permalink and an optional comment.
    Forward {
        #[command(flatten)]
        target: MessageTarget,
        /// Exact destination conversation (channel, group or existing DM).
        #[arg(long)]
        to: String,
        /// Text posted above the forwarded permalink.
        #[arg(long, default_value = "")]
        comment: String,
    },
    /// Open one direct or group conversation; does not send a message.
    Dm {
        /// One to eight people (@handle, display name or user id).
        #[arg(required = true, num_args = 1..=8, value_name = "PERSON")]
        people: Vec<String>,
    },
    /// Schedule one message for a future time (at most 120 days).
    Schedule {
        /// Exact conversation name, #name, person for an existing DM, or id.
        channel: String,
        /// Unix seconds or RFC3339 including timezone.
        #[arg(long, required = true)]
        at: String,
        #[command(flatten)]
        options: MessageOptions,
    },
    /// Read the local, payload-free agent action log (no credentials needed).
    Log {
        /// Most recent entries to show (1 to 1000).
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..=1000))]
        limit: u32,
    },
}

pub(super) fn config_path(g: &Global) -> Result<PathBuf> {
    Ok(g.agent_config
        .clone()
        .map(Ok)
        .unwrap_or_else(agent::default_config_path)?)
}
pub(super) fn log_path(g: &Global) -> Result<PathBuf> {
    Ok(g.agent_log
        .clone()
        .map(Ok)
        .unwrap_or_else(agent::default_log_path)?)
}

pub(super) async fn run(command: WriteCommand, creds: &CredentialStore, g: &Global) -> Result<()> {
    let command = match command {
        WriteCommand::Schedule {
            channel,
            at,
            options,
        } => return send_commands::send(creds, g, &channel, options, Some(at)).await,
        WriteCommand::Log { limit } => {
            let entries = ActionLog::new(log_path(g)?).entries(limit as usize)?;
            if g.json {
                emit(json!({"entries":entries}));
            } else {
                for e in entries {
                    println!(
                        "{} {} {} {} {} {}",
                        e.id,
                        e.started_at,
                        e.team_id,
                        e.action,
                        e.targets.join(","),
                        e.outcome
                    );
                }
            }
            return Ok(());
        }
        other => other,
    };
    let config = AgentConfig::load(&config_path(g)?)?;
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    let prepared = match command {
        WriteCommand::Edit {
            reference,
            ts,
            text,
            file,
            stdin,
        } => {
            // A permalink carries its own ts, so a lone positional is the reference.
            let text = send_commands::read_text(text, file, stdin, false)?;
            db.with_read(|c| {
                PreparedWrite::own_message(c, &team, false, &reference, ts.as_deref(), Some(&text))
            })?
        }
        WriteCommand::Delete { target } => db.with_read(|c| {
            PreparedWrite::own_message(
                c,
                &team,
                true,
                &target.reference,
                target.ts.as_deref(),
                None,
            )
        })?,
        WriteCommand::Dm { people } => db.with_read(|c| PreparedWrite::dm(c, &team, &people))?,
        WriteCommand::Forward {
            target,
            to,
            comment,
        } => db.with_read(|c| {
            PreparedWrite::forward(
                c,
                &team,
                &target.reference,
                target.ts.as_deref(),
                &to,
                &comment,
                &config,
            )
        })?,
        other => {
            let (action, target, emoji) = match other {
                WriteCommand::React { target, emoji } => {
                    (MessageAction::React, target, Some(emoji))
                }
                WriteCommand::Unreact { target, emoji } => {
                    (MessageAction::Unreact, target, Some(emoji))
                }
                WriteCommand::MarkRead { target } => (MessageAction::MarkRead, target, None),
                WriteCommand::Pin { target } => (MessageAction::Pin, target, None),
                WriteCommand::Unpin { target } => (MessageAction::Unpin, target, None),
                WriteCommand::Save { target } => (MessageAction::Save, target, None),
                _ => unreachable!(),
            };
            db.with_read(|c| {
                PreparedWrite::message_action(
                    c,
                    &team,
                    action,
                    &target.reference,
                    target.ts.as_deref(),
                    emoji.as_deref(),
                )
            })?
        }
    };
    execute(creds, g, &db, &config, prepared).await
}

/// Denials explain how to authorize; every other error passes through unchanged.
pub(super) fn guard_error(e: Error) -> anyhow::Error {
    match e {
        Error::Forbidden => anyhow::Error::new(e).context(
            "write guard: pass --yes or add an exact team/channel/action rule to agent.toml",
        ),
        other => other.into(),
    }
}

pub(super) async fn execute(
    creds: &CredentialStore,
    g: &Global,
    db: &Store,
    config: &AgentConfig,
    prepared: PreparedWrite,
) -> Result<()> {
    if g.dry_run {
        let report = prepared.dry_run_report(config, g.yes);
        if g.json {
            emit(report);
        } else {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        return Ok(());
    }
    let receipt = prepared
        .run_guarded(&ActionLog::new(log_path(g)?), config, g.yes, db, || async {
            let session = auth::resolve_session(creds, Some(&prepared.action.team_id)).await?;
            Ok(WriteSession {
                client: session.client()?,
                team_id: session.identity.team_id.clone(),
                user_id: session.identity.user_id.clone(),
            })
        })
        .await
        .map_err(guard_error)?;
    if g.json {
        emit(serde_json::to_value(&receipt)?);
    } else {
        println!(
            "{} {}{}",
            receipt.action,
            receipt.channel.as_deref().unwrap_or_default(),
            receipt
                .ts
                .as_deref()
                .map(|ts| format!(" (ts {ts})"))
                .unwrap_or_default()
        );
        if let Some(link) = receipt.permalink {
            println!("{link}");
        }
        if let Some(id) = receipt.scheduled_message_id {
            println!(
                "scheduled_message_id {id}, post_at {}",
                receipt.post_at.unwrap_or_default()
            );
        }
        if let Some(warning) = receipt.warning {
            eprintln!("warning: {warning}");
        }
        if receipt.not_stored_locally {
            eprintln!("warning: accepted remotely; local state unavailable, sync before retrying");
        }
    }
    Ok(())
}
