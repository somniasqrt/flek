//! Message input is CLI-owned; resolution, preparation and writes live in core.
use crate::{default_team, open_db, write_commands, Global};
use anyhow::Result;
use clap::Args;
use flek_core::{
    agent::AgentConfig,
    auth::CredentialStore,
    writes::{MessageInput, PreparedWrite},
    Error,
};
use std::{
    io::{self, IsTerminal, Read},
    path::{Path, PathBuf},
};

#[derive(Args)]
pub(super) struct MessageOptions {
    /// Text (omit to read piped stdin; '-' explicitly reads stdin).
    #[arg(conflicts_with_all = ["file", "stdin"])]
    pub text: Option<String>,
    /// Read UTF-8 message text from a file.
    #[arg(long, conflicts_with = "stdin")]
    pub file: Option<PathBuf>,
    /// Read UTF-8 message text from stdin.
    #[arg(long)]
    pub stdin: bool,
    /// Block Kit JSON array, @path to a JSON file, or '-' for stdin.
    #[arg(long, value_name = "JSON")]
    pub blocks: Option<String>,
    /// Root timestamp for a thread reply.
    #[arg(long, value_name = "TS")]
    pub thread: Option<String>,
    /// Also show the thread reply in the channel.
    #[arg(long, requires = "thread")]
    pub broadcast: bool,
}

fn read_bounded(mut reader: impl Read) -> Result<String> {
    let mut value = String::new();
    reader.by_ref().take(1_048_577).read_to_string(&mut value)?;
    if value.len() > 1_048_576 {
        return Err(Error::Usage("message input exceeds 1 MiB".into()).into());
    }
    Ok(value)
}

/// Text from the argument, a file, or stdin (explicit, '-', or piped when omitted).
/// `blocks_stdin` marks another input already claiming stdin.
pub(super) fn read_text(
    text: Option<String>,
    file: Option<PathBuf>,
    stdin: bool,
    blocks_stdin: bool,
) -> Result<String> {
    let text_stdin = stdin
        || text.as_deref() == Some("-")
        || (text.is_none() && file.is_none() && !io::stdin().is_terminal() && !blocks_stdin);
    if blocks_stdin && text_stdin {
        return Err(Error::Usage("stdin can supply either text or blocks, not both".into()).into());
    }
    Ok(if let Some(path) = file {
        read_bounded(std::fs::File::open(path)?)?
    } else if text_stdin {
        read_bounded(io::stdin().lock())?
    } else {
        text.unwrap_or_default()
    })
}

impl MessageOptions {
    fn input(self, post_at: Option<i64>) -> Result<MessageInput> {
        let blocks_stdin = self.blocks.as_deref() == Some("-");
        let text = read_text(self.text, self.file, self.stdin, blocks_stdin)?;
        let blocks = self
            .blocks
            .map(|source| -> Result<_> {
                let raw = if source == "-" {
                    read_bounded(io::stdin().lock())?
                } else if let Some(path) = source.strip_prefix('@') {
                    read_bounded(std::fs::File::open(Path::new(path))?)?
                } else {
                    source
                };
                // Do not echo source lines in parser errors.
                serde_json::from_str::<Vec<serde_json::Value>>(&raw).map_err(|_| {
                    Error::Usage("--blocks requires a JSON array of block objects".into()).into()
                })
            })
            .transpose()?;
        Ok(MessageInput {
            text,
            blocks,
            thread_ts: self.thread,
            broadcast: self.broadcast,
            post_at,
        })
    }
}

pub(super) async fn send(
    creds: &CredentialStore,
    g: &Global,
    channel: &str,
    options: MessageOptions,
    at: Option<String>,
) -> Result<()> {
    let post_at = at
        .map(|v| {
            v.parse::<i64>()
                .ok()
                .or_else(|| {
                    chrono::DateTime::parse_from_rfc3339(&v)
                        .ok()
                        .map(|d| d.timestamp())
                })
                .ok_or_else(|| {
                    Error::Usage("--at requires Unix seconds or RFC3339 with a timezone".into())
                })
        })
        .transpose()?;
    let input = options.input(post_at)?;
    let config = AgentConfig::load(&write_commands::config_path(g)?)?;
    let db = open_db(g)?;
    let team = default_team(&db, g)?;
    let prepared = db.with_read(|c| PreparedWrite::message(c, &team, channel, input, &config))?;
    write_commands::execute(creds, g, &db, &config, prepared).await
}
