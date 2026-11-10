//! `flek`: command-line boundary on top of flek-core.

mod account_commands;
mod archive_commands;
mod args;
mod common;
mod dispatch;
mod output;
mod read_commands;
mod search_commands;
mod send_commands;
mod sync_commands;

use args::{Cli, Global};
use clap::Parser;
use common::{default_team, format_ts, open_db};

#[tokio::main]
async fn main() {
    let json_output = std::env::args().any(|a| a == "--json");
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
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
    let json_output = cli.global.json;
    if let Err(error) = dispatch::run(cli).await {
        std::process::exit(output::report(&error, json_output));
    }
}
