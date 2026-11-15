//! Thin channel/status adapters over offline preparation and one core mutation.
use crate::{
    args::Global,
    default_team, open_db,
    output::emit,
    write_commands::{config_path, guard_error, log_path},
};
use anyhow::Result;
use clap::{Args, Subcommand};
use flek_core::{
    agent::{ActionLog, AgentConfig},
    auth::{self, CredentialStore},
    store::Store,
    sync::{
        live,
        management::{self, Operation},
    },
    views,
    writes::WriteSession,
};
use serde_json::json;

#[derive(Args)]
pub struct ChannelTarget {
    /// Exact stored channel name, #name or id.
    channel: String,
}

#[derive(Args)]
pub struct MemberTarget {
    /// Exact stored channel name, #name or id.
    channel: String,
    /// Exact stored person id, handle, display name or email (one person).
    person: String,
}

#[derive(Args)]
pub struct ChannelText {
    /// Exact stored channel name, #name or id.
    channel: String,
    /// New value (pass an empty string to clear topic/purpose).
    text: String,
}

#[derive(Subcommand)]
pub enum ChannelMutation {
    /// Create a public channel, or a private channel with --private.
    Create {
        name: String,
        #[arg(long)]
        private: bool,
    },
    /// Invite one person to a channel.
    Invite(MemberTarget),
    /// Remove one person from a channel.
    Kick(MemberTarget),
    /// Join a stored channel and catch up its history.
    Join(ChannelTarget),
    /// Leave a channel, retaining its local history.
    Leave(ChannelTarget),
    /// Set or clear a channel's topic.
    Topic(ChannelText),
    /// Set or clear a channel's purpose.
    Purpose(ChannelText),
    /// Rename a channel (same name normalization as create).
    Rename(ChannelText),
    /// Archive a channel, retaining its local history.
    Archive(ChannelTarget),
}

#[derive(Subcommand)]
pub enum StatusCommand {
    /// Set your own custom status.
    Set {
        text: String,
        /// Slack emoji shortcode, for example :calendar: (empty by default).
        #[arg(long, default_value = "")]
        emoji: String,
        /// 0/never, future Unix seconds, RFC3339, date, or duration from now (30m/12h/7d).
        #[arg(long, alias = "expiration", default_value = "0", value_name = "TIME")]
        expires: String,
    },
    /// Clear your status text, emoji and expiration.
    Clear,
}

pub async fn channel(
    command: ChannelMutation,
    creds: &CredentialStore,
    g: &Global,
    db: &Store,
    team: &str,
) -> Result<()> {
    let operation = match command {
        ChannelMutation::Create { name, private } => Operation::Create { name, private },
        ChannelMutation::Invite(MemberTarget { channel, person }) => {
            Operation::Invite { channel, person }
        }
        ChannelMutation::Kick(MemberTarget { channel, person }) => {
            Operation::Kick { channel, person }
        }
        ChannelMutation::Join(ChannelTarget { channel }) => Operation::Join { channel },
        ChannelMutation::Leave(ChannelTarget { channel }) => Operation::Leave { channel },
        ChannelMutation::Archive(ChannelTarget { channel }) => Operation::Archive { channel },
        ChannelMutation::Topic(ChannelText { channel, text }) => Operation::Topic { channel, text },
        ChannelMutation::Purpose(ChannelText { channel, text }) => {
            Operation::Purpose { channel, text }
        }
        ChannelMutation::Rename(ChannelText {
            channel,
            text: name,
        }) => Operation::Rename { channel, name },
    };
    run(creds, g, db, team, operation).await
}

pub async fn status(command: StatusCommand, creds: &CredentialStore, g: &Global) -> Result<()> {
    let operation = match command {
        StatusCommand::Set {
            text,
            emoji,
            expires,
        } => Operation::SetStatus(flek_core::api::CustomStatus {
            status_text: text,
            status_emoji: emoji,
            status_expiration: management::status_expiration(&expires, chrono::Utc::now())?,
        }),
        StatusCommand::Clear => Operation::ClearStatus,
    };
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    run(creds, g, &db, &team, operation).await
}

async fn run(
    creds: &CredentialStore,
    g: &Global,
    db: &Store,
    team: &str,
    operation: Operation,
) -> Result<()> {
    let config = AgentConfig::load(&config_path(g)?)?;
    let plan = db.with_read(|c| management::prepare(c, team, operation))?;
    if g.dry_run {
        let report = plan.dry_run_report(&config, g.yes);
        if g.json {
            emit(report);
        } else {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        return Ok(());
    }
    let action = plan.action().clone();
    let outcome = plan
        .run_guarded(
            &ActionLog::new(log_path(g)?),
            &config,
            g.yes,
            db,
            &live::bus(),
            || async {
                let session = auth::resolve_session(creds, Some(&action.team_id)).await?;
                Ok(WriteSession {
                    client: session.client()?,
                    team_id: session.identity.team_id.clone(),
                    user_id: session.identity.user_id.clone(),
                })
            },
        )
        .await
        .map_err(guard_error)?;
    // Return stored views only. A read failure after acceptance is partial success.
    let conversation = outcome
        .conversation_id
        .as_deref()
        .and_then(|id| db.with_read(|c| views::conversation(c, id)).ok().flatten());
    let user = if outcome.conversation_id.is_none() {
        outcome
            .user_id
            .as_deref()
            .and_then(|id| db.with_read(|c| views::user(c, id)).ok().flatten())
    } else {
        None
    };
    let not_stored_locally = outcome.not_stored_locally
        || (outcome.conversation_id.is_some() && conversation.is_none())
        || (outcome.conversation_id.is_none() && user.is_none());
    let warning = if not_stored_locally {
        Some("not_stored_locally".to_owned())
    } else {
        outcome.warning
    };
    if g.json {
        emit(json!({
            "ok":true,"dry_run":false,"action":action.action,"targets":action.targets,
            "conversation_id":outcome.conversation_id,"user_id":outcome.user_id,
            "conversation":if not_stored_locally { None } else { conversation },
            "user":if not_stored_locally { None } else { user },
            "warning":warning,"not_stored_locally":not_stored_locally,
        }));
    } else {
        let target = outcome
            .conversation_id
            .or(outcome.user_id)
            .unwrap_or_default();
        println!("{}: {target}", action.action);
        if let Some(warning) = warning {
            eprintln!("warning: Slack accepted the action ({warning}); sync to refresh local data");
        }
    }
    Ok(())
}
