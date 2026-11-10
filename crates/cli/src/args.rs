//! Clap definitions; flags and help text are part of the CLI contract.

use crate::{read_commands, search_commands};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "flek", version, about = "Local-first Slack client")]
pub(super) struct Cli {
    #[command(flatten)]
    pub(super) global: Global,
    #[command(subcommand)]
    pub(super) command: Command,
}

#[derive(Args)]
pub(super) struct Global {
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    pub(super) json: bool,
    /// More logging (-v info, -vv debug, -vvv trace). RUST_LOG overrides.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub(super) verbose: u8,
    /// Team id (offline: the only team in the database; online: stored session).
    #[arg(long, global = true, value_name = "TEAM_ID")]
    pub(super) team: Option<String>,
    /// Database file (default: flek/flek.db in the OS data directory).
    #[arg(long, global = true, value_name = "PATH")]
    pub(super) db: Option<PathBuf>,
}

#[derive(Subcommand)]
pub(super) enum Command {
    #[command(flatten)]
    Read(read_commands::ReadCommand),
    /// Manage stored sessions.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Show the current team and user (imports from Slack desktop if needed).
    Whoami,
    /// Open (create, migrate) the local database and show where it lives.
    Db,
    /// Fetch users, conversations and message history into the local database.
    Sync {
        /// Only this conversation (name, #name or id).
        #[arg(long)]
        channel: Option<String>,
        /// Skip downloading files into the local cache.
        #[arg(long)]
        no_files: bool,
        /// Largest file to download, in MB (0 = no limit).
        #[arg(long, default_value_t = 100, value_name = "MB")]
        max_file_size: u64,
        /// Re-read the last N days for edits, reactions, deletions and thread replies (0 = off).
        #[arg(long, default_value_t = 7, value_name = "DAYS")]
        refresh_days: u32,
        /// Re-read the whole history and every thread, merging without loss.
        #[arg(long)]
        full: bool,
        /// Stay connected and print new messages as they arrive (Ctrl+C to stop).
        #[arg(long)]
        follow: bool,
    },
    /// Show recent messages of a conversation from the local database.
    History {
        /// Conversation name, #name, person for a DM, or id.
        channel: String,
        #[command(flatten)]
        options: read_commands::HistoryOptions,
    },
    /// Search the local archive (works offline, includes messages Slack hides).
    ///
    /// Filters: from:<person> in:<channel> has:link has:file before:YYYY-MM-DD after:YYYY-MM-DD.
    /// Words: word, prefix*, "exact phrase", -excluded.
    Search {
        /// Query, e.g. `grades sheet in:#general after:2024-01-01`.
        #[arg(required = true, num_args = 1..)]
        query: Vec<String>,
        #[command(flatten)]
        options: search_commands::Options,
    },
    /// Import an existing archive (simple JSON or Slack export / slackdump).
    Import {
        /// Archive file or directory.
        path: PathBuf,
        /// Put every message of a single-file archive into this conversation.
        #[arg(long)]
        channel: Option<String>,
    },
    /// Send a message as you (Slack mrkdwn; `&`, `<`, `>` are escaped).
    Send {
        /// Conversation name, #name, person for a DM, or id.
        channel: String,
        /// Message text.
        text: String,
        /// Reply in this thread (root ts, as `flek history` prints it).
        #[arg(long, value_name = "TS")]
        thread: Option<String>,
        /// With --thread: also show the reply in the channel.
        #[arg(long, requires = "thread")]
        broadcast: bool,
    },
    /// List conversations from the local database.
    Channels {
        /// Include archived channels.
        #[arg(long)]
        archived: bool,
        /// Include conversations discovered by search without joining them.
        #[arg(long)]
        include_non_member: bool,
    },
}

#[derive(Subcommand)]
pub(super) enum AuthCommand {
    /// Import the session from the Slack desktop app into the Keychain.
    Import,
    /// List teams with stored credentials.
    List,
    /// Remove stored credentials from the Keychain.
    Logout {
        /// Remove every stored team.
        #[arg(long)]
        all: bool,
    },
}
