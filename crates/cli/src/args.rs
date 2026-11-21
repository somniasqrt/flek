//! Clap definitions; flags and help text are part of the CLI contract.

use crate::{event_commands, read_commands, search_commands};
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
    /// Authorize this individual agent write (policy and rate limits still apply).
    #[arg(long, global = true)]
    pub(super) yes: bool,
    /// Preview the exact prepared form without credentials or network writes.
    #[arg(long, global = true)]
    pub(super) dry_run: bool,
    /// Agent policy file (default: ~/.config/flek/agent.toml).
    #[arg(long, global = true, value_name = "PATH")]
    pub(super) agent_config: Option<PathBuf>,
    /// Action log database (default: agent-actions.db in Flek's data directory).
    #[arg(long, global = true, value_name = "PATH")]
    pub(super) agent_log: Option<PathBuf>,
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
    /// Set or clear your own custom status.
    #[command(subcommand)]
    Status(crate::channel_commands::StatusCommand),
    /// Manage stored sessions.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Sign in with a browser session (last resort).
    ///
    /// In browser DevTools, find the xoxc token under Application → Local Storage →
    /// https://app.slack.com → localConfig_v2 → teams.<team>.token, and the d cookie
    /// under Application → Cookies → https://app.slack.com. Treat both as passwords.
    /// Terminal input is hidden. Piped stdin: token on line 1, cookie on line 2.
    Login {
        /// Read the token and d cookie securely; never pass credentials as arguments.
        #[arg(long, required = true)]
        manual: bool,
    },
    /// Show the current team and user (imports from Slack desktop if needed).
    Whoami,
    /// Open (create, migrate) the local database and show where it lives.
    Db,
    /// Describe commands, arguments, write guards and exit codes (use with --json).
    Schema {
        /// Only this command or namespace, e.g. `channel create`.
        path: Vec<String>,
    },
    /// Stream committed messages, mentions, edits, deletions and reactions as NDJSON.
    Events {
        #[command(flatten)]
        options: event_commands::EventOptions,
    },
    /// Print the first reply from someone else as JSON (timeout exits with code 9).
    Wait {
        /// A Slack permalink, or a conversation followed by a Slack timestamp.
        #[arg(long, required = true, num_args = 1..=2, value_names = ["REFERENCE", "TS"])]
        reply_to: Vec<String>,
        /// Maximum wait in seconds (default: unlimited).
        #[arg(long, value_name = "SECONDS")]
        timeout: Option<u64>,
    },
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
        /// Re-read the last N days for edits, reactions, deletions and thread replies (0 = off, max 90).
        #[arg(long, default_value_t = 7, value_name = "DAYS",
            value_parser = clap::value_parser!(u32).range(0..=i64::from(flek_core::sync::FREE_HISTORY_DAYS)))]
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
    /// Filters: from:<person|me> -from:<person|me> in:<channel> -in:<channel> with:<person> has:link has:file has:pin has::emoji: hasmy::emoji: is:thread is:saved before:YYYY-MM-DD after:YYYY-MM-DD on:YYYY-MM-DD during:YYYY[-MM].
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
    /// Export the local archive to an Obsidian vault (offline).
    Export(crate::export_commands::Options),
    /// Send one guarded message; reads piped stdin when text is omitted.
    Send {
        /// Exact conversation name, #name, person for an existing DM, or id.
        channel: String,
        #[command(flatten)]
        options: crate::send_commands::MessageOptions,
    },
    #[command(flatten)]
    Write(crate::write_commands::WriteCommand),
    /// Upload one file as you (guarded like send).
    Upload(crate::upload_commands::UploadOptions),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_days_rejects_values_above_archive_visibility_limit() {
        let error = Cli::try_parse_from(["flek", "sync", "--refresh-days", "91"])
            .err()
            .expect("oversized refresh window must be a usage error");
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
        assert!(error.to_string().contains("90"));
        for days in ["0", "7", "90"] {
            assert!(Cli::try_parse_from(["flek", "sync", "--refresh-days", days]).is_ok());
        }
    }
}
