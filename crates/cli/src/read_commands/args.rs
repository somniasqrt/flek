//! Read command arguments.
use clap::{Args, Subcommand};
use std::path::PathBuf;

#[derive(Args, Default)]
pub struct HistoryOptions {
    /// Most messages to show.
    #[arg(long, default_value_t = 30)]
    pub limit: usize,
    /// Show this thread (root timestamp) instead of the channel timeline.
    #[arg(long, value_name = "TS")]
    pub thread: Option<String>,
    /// Fetch new messages from Slack first (needs a session).
    #[arg(long)]
    pub fresh: bool,
    /// Only messages at or after this time: Slack ts, YYYY-MM-DD (local midnight), RFC 3339 or a duration ago (30m, 12h, 7d).
    #[arg(long, value_name = "TIME")]
    pub since: Option<String>,
    /// Only messages before this time (same formats as --since).
    #[arg(long, value_name = "TIME")]
    pub until: Option<String>,
    /// Center the page on this message (timestamp or permalink); it is marked `target`.
    #[arg(long, conflicts_with = "cursor", value_name = "TS_OR_PERMALINK")]
    pub around: Option<String>,
    /// Continue from `next_cursor` (older) or `prev_cursor` (newer) of a previous page.
    #[arg(long)]
    pub cursor: Option<String>,
}
#[derive(Args)]
pub struct MessageRef {
    /// Slack permalink or conversation reference followed by a timestamp.
    pub(super) reference: String,
    /// Message timestamp; omit when the reference is a permalink.
    pub(super) ts: Option<String>,
    /// Fetch the thread from Slack first (needs a session).
    #[arg(long)]
    pub(super) fresh: bool,
}
#[derive(Subcommand)]
pub enum ReadCommand {
    /// List unread conversations (offline unless --fresh).
    Unread {
        /// Also include up to N unread messages per conversation.
        #[arg(long, default_value_t = 0, value_name = "N")]
        messages: usize,
        /// Also list conversations whose read marker is unknown.
        #[arg(long)]
        include_unknown: bool,
        /// Catch up conversations with new messages from Slack first (needs a session).
        #[arg(long)]
        fresh: bool,
    },
    /// Root and all stored replies; a reply reference resolves to its root.
    Thread(MessageRef),
    /// One message with adjacent messages from the same timeline.
    Message {
        #[command(flatten)]
        reference: MessageRef,
        /// Messages to show before and after it.
        #[arg(long, default_value_t = 0, value_name = "N")]
        context: usize,
    },
    /// List stored people.
    Users {
        /// Case-insensitive part of a handle, display name, real name or email.
        #[arg(long, value_name = "TEXT")]
        query: Option<String>,
        /// Include bots and apps.
        #[arg(long)]
        include_bots: bool,
        /// Include deactivated accounts.
        #[arg(long)]
        include_deleted: bool,
        /// Most people to list.
        #[arg(long, default_value_t = 100)]
        limit: usize,
    },
    /// Show one stored person's profile.
    User {
        /// @handle, display name or user id.
        reference: String,
    },
    /// Read channel details or explicitly manage channels.
    #[command(subcommand)]
    Channel(ChannelCommand),
    /// Locally stored mentions of me, newest first.
    Mentions {
        /// Start of the period: Slack ts, YYYY-MM-DD (local midnight), RFC 3339 or a duration ago (30m, 12h, 7d).
        #[arg(long, default_value = "7d", value_name = "TIME")]
        since: String,
        /// End of the period (same formats as --since).
        #[arg(long, value_name = "TIME")]
        until: Option<String>,
        /// Most messages to show.
        #[arg(long, default_value_t = 50)]
        limit: usize,
        /// Also count @here, @channel and @everyone.
        #[arg(long)]
        include_broadcasts: bool,
    },
    /// List stored files with message provenance.
    Files {
        /// Only files shared in this conversation.
        #[arg(long, value_name = "REFERENCE")]
        channel: Option<String>,
        /// Only files shared by this person.
        #[arg(long, value_name = "PERSON")]
        from: Option<String>,
        /// image, pdf, video, audio, doc, other, or a mimetype prefix such as `text/`.
        #[arg(long = "type", value_name = "KIND")]
        kind: Option<String>,
        /// Shared at or after this time: Slack ts, YYYY-MM-DD (local midnight), RFC 3339 or a duration ago (30m, 12h, 7d).
        #[arg(long, value_name = "TIME")]
        since: Option<String>,
        /// Shared before this time (same formats as --since).
        #[arg(long, value_name = "TIME")]
        until: Option<String>,
        /// Most files to list.
        #[arg(long, default_value_t = 50)]
        limit: usize,
    },
    /// Access cached files.
    #[command(subcommand)]
    File(FileCommand),
}
#[derive(Subcommand)]
pub enum ChannelCommand {
    #[command(flatten)]
    Manage(crate::channel_commands::ChannelMutation),
    /// Stored metadata; --fresh also fetches members and pins.
    Info {
        /// Channel name, #name or id.
        reference: String,
        /// Fetch members and pinned messages from Slack first (needs a session).
        #[arg(long)]
        fresh: bool,
    },
}
#[derive(Subcommand)]
pub enum FileCommand {
    /// Print the cache path, downloading only if missing.
    Get {
        /// Slack file id (F…), as listed by `flek files`.
        id: String,
        /// Also copy the file to this file path (replaced if it exists).
        #[arg(long, value_name = "PATH")]
        to: Option<PathBuf>,
    },
}
