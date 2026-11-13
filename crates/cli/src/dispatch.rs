//! Command dispatch and logging setup.

use crate::{
    account_commands::*, archive_commands::*, args::*, read_commands, search_commands,
    send_commands::send, sync_commands::sync,
};
use anyhow::Result;
use flek_core::auth::CredentialStore;
use tracing_subscriber::prelude::*;

pub(super) async fn run(cli: Cli) -> Result<()> {
    init_tracing(cli.global.verbose, cli.global.json);
    let store = CredentialStore::keychain();
    let g = &cli.global;

    match cli.command {
        Command::Whoami => whoami(&store, g).await,
        Command::Db => db(g),
        Command::Sync {
            channel,
            no_files,
            max_file_size,
            refresh_days,
            full,
            follow,
        } => {
            let max = (!no_files).then_some(max_file_size);
            sync(
                &store,
                g,
                channel.as_deref(),
                max,
                refresh_days,
                full,
                follow,
            )
            .await
        }
        Command::Read(cmd) => read_commands::run(cmd, &store, g).await,
        Command::History { channel, options } => {
            read_commands::history(&store, g, &channel, options).await
        }
        Command::Search { query, options } => {
            search_commands::run(&store, g, &query.join(" "), options).await
        }
        Command::Import { path, channel } => import_archive(g, &path, channel.as_deref()),
        Command::Send {
            channel,
            text,
            thread,
            broadcast,
        } => send(&store, g, &channel, text, thread, broadcast).await,
        Command::Channels {
            archived,
            include_non_member,
        } => channels(g, archived, include_non_member),
        Command::Auth(AuthCommand::Import) => import(&store, g).await,
        Command::Auth(AuthCommand::List) => list(&store, g),
        Command::Auth(AuthCommand::Logout { all }) => logout(&store, g, all),
    }
}

fn init_tracing(verbose: u8, json: bool) {
    let directives = std::env::var("RUST_LOG").ok();
    let default = flek_core::logging::cli_default(verbose);
    let (default, directives) = if json {
        ("off", None)
    } else {
        (default, directives.as_deref())
    };
    tracing_subscriber::registry()
        .with(flek_core::logging::filter(default, directives))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(false),
        )
        .init();
}
