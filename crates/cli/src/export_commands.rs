//! Thin, credential-free offline export boundary.
use crate::{default_team, output::emit, Global};
use anyhow::Result;
use clap::Args;
use flek_core::{
    export, files,
    store::{self, Store},
};
use std::path::PathBuf;

#[derive(Args)]
pub(super) struct Options {
    /// Destination vault (created if missing).
    #[arg(long, value_name = "VAULT")]
    obsidian: PathBuf,
    /// Only this stored conversation (name, #name, person or id).
    #[arg(long)]
    channel: Option<String>,
    /// Inclusive lower bound: Slack ts, YYYY-MM-DD, RFC3339 or 30m/12h/7d.
    #[arg(long)]
    since: Option<String>,
    /// Exclusive upper bound, using the same formats as --since.
    #[arg(long)]
    until: Option<String>,
}

pub(super) async fn run(g: &Global, options: Options) -> Result<()> {
    let db = Store::open_read_only(&g.db.clone().map_or_else(store::default_path, Ok)?)?;
    let team = default_team(&db, g)?;
    let cache = files::default_cache_dir()?;
    let summary = tokio::task::spawn_blocking(move || {
        db.with_read(|conn| {
            export::obsidian(
                conn,
                &team,
                &options.obsidian,
                &cache,
                &export::Options {
                    channel: options.channel,
                    since: options.since,
                    until: options.until,
                },
            )
        })
    })
    .await??;
    if g.json {
        emit(serde_json::json!({"summary": summary}));
    } else {
        println!(
            "Exported {} conversations, {} messages, {} links and {} files to {}",
            summary.conversations,
            summary.messages,
            summary.links,
            summary.files,
            summary.directory.display()
        );
        println!(
            "{} notes written, {} unchanged; {} attachments copied, {} unchanged, {} unavailable",
            summary.notes_written,
            summary.notes_unchanged,
            summary.attachments_copied,
            summary.attachments_unchanged,
            summary.attachments_unavailable
        );
    }
    Ok(())
}
