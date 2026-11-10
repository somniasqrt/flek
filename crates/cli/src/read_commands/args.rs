//! Read command arguments.
use clap::{Args, Subcommand};
use std::path::PathBuf;

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
    pub(super) reference: String,
    pub(super) ts: Option<String>,
    #[arg(long)]
    pub(super) fresh: bool,
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
