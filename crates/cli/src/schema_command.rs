//! `flek schema`: the command tree as data, so agents need not scrape `--help`.
//!
//! Built from the clap definitions at run time, so it cannot drift from the
//! parser. Offline: no database, no Keychain.
use crate::{args::Cli, output::emit, Global};
use anyhow::Result;
use clap::{Arg, ArgAction, Command, CommandFactory};
use flek_core::{error::ErrorKind, Error};
use serde_json::{json, Value};

/// Writer commands and their guard action names (`agent.toml`, `flek log`).
pub(crate) const GUARDED: &[(&str, &str)] = &[
    ("send", "send"),
    ("schedule", "schedule"),
    ("edit", "edit"),
    ("delete", "delete"),
    ("react", "react"),
    ("unreact", "unreact"),
    ("mark-read", "mark-read"),
    ("pin", "pin"),
    ("unpin", "unpin"),
    ("save", "save"),
    ("forward", "forward"),
    ("dm", "dm"),
    ("upload", "upload"),
    ("channel create", "channel-create"),
    ("channel invite", "channel-invite"),
    ("channel kick", "channel-kick"),
    ("channel join", "channel-join"),
    ("channel leave", "channel-leave"),
    ("channel topic", "channel-topic"),
    ("channel purpose", "channel-purpose"),
    ("channel rename", "channel-rename"),
    ("channel archive", "channel-archive"),
    ("status set", "status-set"),
    ("status clear", "status-clear"),
];

/// Commands whose JSON output is a stream of NDJSON lines, not one object.
const STREAMS: &[&str] = &["events"];

/// Commands that read stored credentials and may call Slack. Everything else
/// works from the local database alone (`--fresh`/`--remote` flags excepted).
const ONLINE: &[&str] = &[
    "whoami",
    "sync",
    "events",
    "wait",
    "login",
    "auth import",
    "auth list",
    "auth logout",
];

pub(crate) fn run(g: &Global, path: &[String]) -> Result<()> {
    let root = Cli::command();
    let mut node = &root;
    for word in path {
        node = node
            .get_subcommands()
            .find(|c| c.get_name() == word)
            .ok_or_else(|| Error::NotFound(format!("command `{}`", path.join(" "))))?;
    }
    let mut commands = Vec::new();
    collect(node, path.to_vec(), &mut commands);
    if g.json {
        emit(json!({
            "cli_version": env!("CARGO_PKG_VERSION"),
            "global_options": root
                .get_arguments()
                .filter(|a| a.is_global_set())
                .map(arg)
                .collect::<Vec<_>>(),
            "commands": commands,
            "exit_codes": exit_codes(),
        }));
    } else {
        for c in &commands {
            let marker = match c["guarded_action"].as_str() {
                Some(_) => "  [write]",
                None => "",
            };
            println!(
                "flek {:<22} {}{marker}",
                c["path"].as_str().unwrap_or_default(),
                c["about"].as_str().unwrap_or_default()
            );
        }
    }
    Ok(())
}

/// Depth-first leaves; a command with subcommands is a namespace, not a command.
fn collect(cmd: &Command, path: Vec<String>, out: &mut Vec<Value>) {
    let subcommands: Vec<_> = cmd.get_subcommands().filter(|c| !c.is_hide_set()).collect();
    if subcommands.is_empty() {
        if !path.is_empty() {
            out.push(leaf(cmd, &path.join(" ")));
        }
        return;
    }
    for sub in subcommands {
        let mut next = path.clone();
        next.push(sub.get_name().to_owned());
        collect(sub, next, out);
    }
}

fn leaf(cmd: &Command, path: &str) -> Value {
    let guarded = GUARDED.iter().find(|(p, _)| *p == path).map(|(_, a)| *a);
    json!({
        "path": path,
        "about": cmd.get_about().map(|s| s.to_string()),
        "long_about": cmd.get_long_about().map(|s| s.to_string()),
        "guarded_action": guarded,
        "needs_session": guarded.is_some() || ONLINE.contains(&path),
        "output": if STREAMS.contains(&path) { "ndjson" } else { "object" },
        "arguments": cmd
            .get_arguments()
            .filter(|a| !a.is_global_set() && !a.is_hide_set() && !is_builtin(a))
            .map(arg)
            .collect::<Vec<_>>(),
    })
}

fn is_builtin(a: &Arg) -> bool {
    matches!(
        a.get_action(),
        ArgAction::Help | ArgAction::HelpShort | ArgAction::HelpLong | ArgAction::Version
    )
}

fn arg(a: &Arg) -> Value {
    let takes_values = a.get_action().takes_values();
    let kind = if a.is_positional() {
        "positional"
    } else if takes_values {
        "option"
    } else {
        "flag"
    };
    let multiple = a.get_num_args().is_some_and(|n| n.max_values() > 1)
        || matches!(a.get_action(), ArgAction::Append | ArgAction::Count);
    // Clap gives flags a hidden "true"/"false" value; agents only need presence.
    let (value_names, default, possible_values): (Vec<_>, Vec<_>, Vec<_>) = if takes_values {
        (
            a.get_value_names()
                .unwrap_or_default()
                .iter()
                .map(|s| s.to_string())
                .collect(),
            a.get_default_values()
                .iter()
                .map(|v| v.to_string_lossy().into_owned())
                .collect(),
            a.get_possible_values()
                .iter()
                .filter(|v| !v.is_hide_set())
                .map(|v| v.get_name().to_owned())
                .collect(),
        )
    } else {
        Default::default()
    };
    json!({
        "name": a.get_id().as_str(),
        "kind": kind,
        "long": a.get_long().map(|l| format!("--{l}")),
        "short": a.get_short().map(|s| format!("-{s}")),
        "required": a.is_required_set(),
        "multiple": multiple,
        "value_names": value_names,
        "default": default,
        "possible_values": possible_values,
        "help": a.get_help().map(|s| s.to_string()),
    })
}

fn exit_codes() -> Vec<Value> {
    use ErrorKind::*;
    let mut codes = vec![json!({"code": 0, "kind": "ok"})];
    for kind in [
        Internal,
        Usage,
        NotFound,
        Ambiguous,
        Auth,
        RateLimited,
        Network,
        Forbidden,
        Timeout,
    ] {
        codes.push(json!({"code": kind.exit_code(), "kind": kind}));
    }
    codes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaves() -> Vec<String> {
        let mut out = Vec::new();
        collect(&Cli::command(), Vec::new(), &mut out);
        out.iter()
            .map(|c| c["path"].as_str().unwrap().to_owned())
            .collect()
    }

    #[test]
    fn every_listed_command_exists() {
        let leaves = leaves();
        for path in GUARDED
            .iter()
            .map(|(p, _)| *p)
            .chain(STREAMS.iter().copied())
            .chain(ONLINE.iter().copied())
        {
            assert!(leaves.iter().any(|l| l == path), "unknown command `{path}`");
        }
    }

    #[test]
    fn guard_action_names_are_unique_and_follow_the_command_path() {
        let mut names: Vec<_> = GUARDED.iter().map(|(_, a)| *a).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), GUARDED.len());
        for (path, action) in GUARDED {
            assert_eq!(path.replace(' ', "-"), *action);
        }
    }

    #[test]
    fn every_argument_is_described_for_agents() {
        let mut out = Vec::new();
        collect(&Cli::command(), Vec::new(), &mut out);
        let missing: Vec<_> = out
            .iter()
            .flat_map(|c| {
                c["arguments"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|a| a["help"].is_null())
                    .map(move |a| format!("{} {}", c["path"], a["name"]))
            })
            .collect();
        assert!(missing.is_empty(), "arguments without help: {missing:?}");
    }

    #[test]
    fn help_and_version_are_not_reported_as_arguments() {
        let root = Cli::command();
        let send = root.find_subcommand("send").unwrap();
        let value = leaf(send, "send");
        let names: Vec<_> = value["arguments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|a| a["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"channel"));
        assert!(!names.contains(&"help"));
    }
}
