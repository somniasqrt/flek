//! `flek`: command-line client on top of `flek-core`.

use anyhow::{bail, Context, Result};
use clap::{Args, Parser, Subcommand};
use std::path::PathBuf;

use flek_core::auth::{self, CredentialStore, ImportedTeam, Session};
use flek_core::store::{self, Store};
use serde_json::json;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "flek", version, about = "Local-first Slack client")]
struct Cli {
    #[command(flatten)]
    global: Global,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args)]
struct Global {
    /// Machine-readable JSON output.
    #[arg(long, global = true)]
    json: bool,
    /// More logging (-v info, -vv debug, -vvv trace). RUST_LOG overrides.
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    verbose: u8,
    /// Team id to act on (defaults to the first stored team).
    #[arg(long, global = true, value_name = "TEAM_ID")]
    team: Option<String>,
    /// Database file (default: ~/Library/Application Support/flek/flek.db).
    #[arg(long, global = true, value_name = "PATH")]
    db: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Manage stored sessions.
    #[command(subcommand)]
    Auth(AuthCommand),
    /// Show the current team and user (imports from Slack desktop if needed).
    Whoami,
    /// Open (create, migrate) the local database and show where it lives.
    Db,
}

#[derive(Subcommand)]
enum AuthCommand {
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

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    init_tracing(cli.global.verbose);
    let store = CredentialStore::keychain();
    let g = &cli.global;

    match cli.command {
        Command::Whoami => whoami(&store, g).await,
        Command::Db => db(g),
        Command::Auth(AuthCommand::Import) => import(&store, g).await,
        Command::Auth(AuthCommand::List) => list(&store, g),
        Command::Auth(AuthCommand::Logout { all }) => logout(&store, g, all),
    }
}

fn init_tracing(verbose: u8) {
    let default = match verbose {
        0 => "warn",
        1 => "info",
        2 => "debug",
        _ => "trace",
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();
}

async fn whoami(store: &CredentialStore, g: &Global) -> Result<()> {
    let Session { identity, .. } = auth::resolve_session(store, g.team.as_deref()).await?;
    if g.json {
        println!("{}", serde_json::to_string_pretty(&identity)?);
    } else {
        println!("team  {} ({})", identity.team, identity.team_id);
        println!("user  {} ({})", identity.user, identity.user_id);
        println!("url   {}", identity.url);
    }
    Ok(())
}

fn open_db(g: &Global) -> Result<Store> {
    let path = match &g.db {
        Some(p) => p.clone(),
        None => store::default_path()?,
    };
    Store::open(&path).with_context(|| format!("opening database {}", path.display()))
}

fn db(g: &Global) -> Result<()> {
    let db = open_db(g)?;
    let path = db
        .path()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    let version = db.schema_version()?;
    if g.json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "path": path, "schema_version": version }))?
        );
    } else {
        println!("path    {path}");
        println!("schema  v{version}");
    }
    Ok(())
}

async fn import(store: &CredentialStore, g: &Global) -> Result<()> {
    let teams = auth::import_and_save(store, g.team.as_deref())
        .await
        .context("importing from Slack desktop")?;
    let failed = teams.iter().filter(|t| t.result.is_err()).count();

    if g.json {
        let rows: Vec<_> = teams.iter().map(import_row).collect();
        println!("{}", serde_json::to_string_pretty(&rows)?);
    } else {
        for t in &teams {
            match &t.result {
                Ok(s) => println!("saved   {} ({}) as {}", t.name, t.team_id, s.identity.user),
                Err(e) => println!("failed  {} ({}): {e}", t.name, t.team_id),
            }
        }
    }
    if failed == teams.len() {
        bail!("no team could be validated");
    }
    Ok(())
}

fn import_row(t: &ImportedTeam) -> serde_json::Value {
    match &t.result {
        Ok(s) => {
            json!({ "team_id": t.team_id, "name": t.name, "ok": true, "user": s.identity.user, "user_id": s.identity.user_id })
        }
        Err(e) => {
            json!({ "team_id": t.team_id, "name": t.name, "ok": false, "error": e.to_string() })
        }
    }
}

fn list(store: &CredentialStore, g: &Global) -> Result<()> {
    let ids = store.list()?;
    if g.json {
        println!("{}", serde_json::to_string_pretty(&ids)?);
    } else if ids.is_empty() {
        println!("no stored teams; run `flek auth import`");
    } else {
        for id in ids {
            println!("{id}");
        }
    }
    Ok(())
}

fn logout(store: &CredentialStore, g: &Global, all: bool) -> Result<()> {
    let stored = store.list()?;
    let targets: Vec<String> = match (&g.team, all) {
        (_, true) => stored,
        (Some(t), false) => vec![t.clone()],
        (None, false) if stored.len() <= 1 => stored,
        (None, false) => bail!(
            "several teams are stored ({}); pass --team or --all",
            stored.join(", ")
        ),
    };
    let mut removed = Vec::new();
    for id in &targets {
        if store.delete(id)? {
            removed.push(id.clone());
        }
    }
    if g.json {
        println!("{}", serde_json::to_string_pretty(&removed)?);
    } else if removed.is_empty() {
        println!("nothing to remove");
    } else {
        for id in removed {
            println!("removed {id}");
        }
    }
    Ok(())
}
