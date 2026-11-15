//! `flek`: command-line boundary on top of flek-core.

mod account_commands;
mod archive_commands;
mod args;
mod common;
mod dispatch;
mod export_commands;
mod manual_login;
mod output;
mod read_commands;
mod search_commands;
mod send_commands;
mod sync_commands;
mod write_commands;

use args::{Cli, Global};
use clap::Parser;
use common::{default_team, format_ts, open_db};

#[tokio::main]
async fn main() {
    let json_output = std::env::args().any(|a| a == "--json");
    let login_requested = std::env::args_os().any(|a| a == "login");
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(e) if login_requested && e.exit_code() != 0 => {
            // Clap's usual diagnostic quotes unexpected arguments. A mistaken
            // credential on argv must never be reflected back to the terminal.
            let error = anyhow::Error::new(flek_core::Error::Usage(
                "use `flek login --manual`; credentials are read only from stdin or hidden prompts, never arguments".into(),
            ));
            std::process::exit(output::report(&error, json_output));
        }
        Err(e) if json_output && e.exit_code() != 0 => {
            let error = anyhow::Error::new(flek_core::Error::Usage(
                e.render()
                    .to_string()
                    .lines()
                    .next()
                    .unwrap_or("invalid arguments")
                    .to_owned(),
            ));
            std::process::exit(output::report(&error, true));
        }
        Err(e) => e.exit(),
    };
    if let Err(error) = if matches!(&cli.command, args::Command::Export(_)) {
        Ok(())
    } else {
        flek_core::auth::chromium::cleanup_stale_copies()
    } {
        eprintln!(
            "warning: could not remove stale session copies: {}",
            output::safe_message(&error.to_string())
        );
    }
    let json_output = cli.global.json;
    if let Err(error) = dispatch::run(cli).await {
        std::process::exit(output::report(&error, json_output));
    }
}
