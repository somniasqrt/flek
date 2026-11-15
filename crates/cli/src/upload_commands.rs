//! Guarded file upload: exact destination, cancellation and JSON framing.
use crate::{
    default_team, open_db,
    output::emit,
    write_commands::{config_path, log_path},
    Global,
};
use anyhow::Result;
use clap::Args;
use flek_core::{
    agent::{ActionLog, AgentConfig},
    auth::{self, CredentialStore},
    writes::{PreparedUpload, WriteSession},
    Error,
};
use std::{path::PathBuf, sync::Arc};

#[derive(Args)]
pub(super) struct UploadOptions {
    /// File to upload (1 byte to 100 MiB).
    file: PathBuf,
    /// Exact conversation name, #name, person for an existing DM, or id.
    #[arg(long)]
    channel: String,
    /// Root timestamp for a thread reply.
    #[arg(long, value_name = "TS")]
    thread: Option<String>,
    /// Text introducing the file.
    #[arg(long, default_value = "")]
    comment: String,
}

pub(super) async fn run(options: UploadOptions, creds: &CredentialStore, g: &Global) -> Result<()> {
    // Everything up to the policy check is offline: SQLite and a stat of the file.
    let config = AgentConfig::load(&config_path(g)?)?;
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    let prepared = db.with_read(|c| {
        PreparedUpload::prepare(
            c,
            &team,
            &options.file,
            &options.channel,
            options.thread,
            &options.comment,
            &config,
        )
    })?;
    if g.dry_run {
        let report = prepared.dry_run_report(&config, g.yes);
        if g.json {
            emit(report);
        } else {
            println!("{}", serde_json::to_string_pretty(&report)?);
        }
        return Ok(());
    }
    let (stop, cancel) = tokio::sync::watch::channel(false);
    let signal = tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            stop.send_replace(true);
        }
    });
    let human = !g.json;
    let result = prepared
        .run_guarded(
            &ActionLog::new(log_path(g)?),
            &config,
            g.yes,
            &db,
            cancel,
            Arc::new(move |p| {
                if human && p.stage != "uploading" {
                    eprintln!("upload: {}", p.stage);
                }
            }),
            || async {
                let session = auth::resolve_session(creds, Some(&prepared.action.team_id)).await?;
                Ok(WriteSession {
                    client: session.client()?,
                    team_id: session.identity.team_id.clone(),
                    user_id: session.identity.user_id.clone(),
                })
            },
        )
        .await;
    signal.abort();
    let receipt = result.map_err(|e| match e {
        Error::Forbidden => anyhow::Error::new(e).context(
            "write guard: pass --yes or add an exact team/channel/action rule to agent.toml",
        ),
        other => other.into(),
    })?;
    if g.json {
        emit(serde_json::to_value(&receipt)?);
    } else {
        println!(
            "{} {} to {}{}",
            if receipt.completion_unknown {
                "awaiting confirmation for"
            } else {
                "uploaded"
            },
            receipt.file_id,
            receipt.conversation_id,
            if receipt.not_stored_locally {
                " (waiting for local sync)"
            } else {
                ""
            }
        );
        if let Some(link) = &receipt.permalink {
            println!("{link}");
        }
        if let Some(warning) = receipt.warning {
            eprintln!("warning: {warning}");
        }
    }
    Ok(())
}
