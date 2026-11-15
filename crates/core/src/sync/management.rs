//! Explicit channel/status mutations: prepare offline, inspect, then execute once.
//!
//! A `Prepared` plan is a [`PreparedWrite`] (guard descriptor + exact request
//! form), so dry runs share the M8.4 report shape and `run_guarded` puts every
//! mutation behind the same policy, rate limit and action log. The plan never
//! contains session credentials.
//!
//! Guard targets: channel actions address the resolved channel id, `status-*`
//! the own user id, and `channel-create` the marker `new:<normalized-name>`
//! (no id exists before creation; allow rules may list that marker or `*`).
use crate::{
    agent::{ActionDescriptor, ActionLog, AgentConfig},
    api::{write::WriteRequest, Client, CustomStatus},
    references::{self, Match},
    store::{
        channel_management::{self, Update},
        Store,
    },
    sync::{
        channel_browser,
        live::{Bus, Change},
    },
    writes::{self, PreparedWrite, WriteSession},
    Error, Result,
};
use chrono::{DateTime, Utc};
use rusqlite::{Connection, OptionalExtension};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone)]
pub enum Operation {
    Create { name: String, private: bool },
    Invite { channel: String, person: String },
    Kick { channel: String, person: String },
    Join { channel: String },
    Leave { channel: String },
    Topic { channel: String, text: String },
    Purpose { channel: String, text: String },
    Rename { channel: String, name: String },
    Archive { channel: String },
    SetStatus(CustomStatus),
    ClearStatus,
}

/// Only `prepare` can construct a plan. All references are exact stored identities.
pub struct Prepared {
    write: PreparedWrite,
    conversation_id: Option<String>,
    user_id: Option<String>,
    self_user_id: String,
    operation: Operation,
}

impl Prepared {
    /// Guard descriptor (action name, team, target) used for policy and the log.
    pub fn action(&self) -> &ActionDescriptor {
        &self.write.action
    }
    pub fn write(&self) -> &PreparedWrite {
        &self.write
    }
    /// Same report as the M8.4 writes; needs no credentials, network or log.
    pub fn dry_run_report(&self, config: &AgentConfig, yes: bool) -> Value {
        self.write.dry_run_report(config, yes)
    }
    /// Policy and rate-limit reservation first, session afterwards, one remote
    /// mutation, then the log entry. See [`writes::guarded`].
    pub async fn run_guarded<F, Fut>(
        self,
        log: &ActionLog,
        config: &AgentConfig,
        yes: bool,
        store: &Store,
        bus: &Bus,
        session: F,
    ) -> Result<Outcome>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<WriteSession>>,
    {
        let action = self.write.action.clone();
        writes::guarded(log, config, yes, &action, session, |session| async move {
            execute(&session, store, bus, self).await
        })
        .await
    }
}

fn plan(
    team: &str,
    action: &str,
    method: &str,
    params: &[(&str, &str)],
    target: &str,
) -> Result<PreparedWrite> {
    let action = ActionDescriptor {
        action: action.into(),
        team_id: team.into(),
        targets: vec![target.into()],
    };
    action.validate()?;
    Ok(PreparedWrite {
        action,
        request: WriteRequest::new(method, params),
        no_op: false,
        warning: None,
    })
}

/// Resolve every target before session access; never silently fetch a directory.
pub fn prepare(c: &Connection, team: &str, mut operation: Operation) -> Result<Prepared> {
    let self_user_id: String = c
        .query_row(
            "SELECT self_user_id FROM teams WHERE id = ?1 AND self_user_id != ''",
            [team],
            |r| r.get(0),
        )
        .optional()?
        .ok_or_else(|| Error::NotFound("no stored identity; run `flek sync` first".into()))?;
    let mut conversation_id = None;
    let mut user_id = None;
    let invite = matches!(operation, Operation::Invite { .. });
    let channel_action = match operation {
        Operation::Join { .. } => ("channel-join", "conversations.join"),
        Operation::Leave { .. } => ("channel-leave", "conversations.leave"),
        _ => ("channel-archive", "conversations.archive"),
    };
    let write = match &mut operation {
        Operation::Create { name, private } => {
            *name = normalized_name(name)?;
            plan(
                team,
                "channel-create",
                "conversations.create",
                &[
                    ("name", name),
                    ("is_private", if *private { "true" } else { "false" }),
                ],
                &format!("new:{name}"),
            )?
        }
        Operation::Invite { channel, person } | Operation::Kick { channel, person } => {
            resolve_channel(c, team, channel)?;
            let person_view = references::resolve_user(c, team, person, Match::Exact)?;
            if person_view.deleted {
                return Err(Error::Usage(
                    "cannot change membership for a deleted user".into(),
                ));
            }
            *person = person_view.id;
            conversation_id = Some(channel.clone());
            user_id = Some(person.clone());
            if invite {
                plan(
                    team,
                    "channel-invite",
                    "conversations.invite",
                    &[("channel", channel), ("users", person)],
                    channel,
                )?
            } else {
                if person == &self_user_id {
                    return Err(Error::Usage(
                        "use `flek channel leave` to leave a channel".into(),
                    ));
                }
                plan(
                    team,
                    "channel-kick",
                    "conversations.kick",
                    &[("channel", channel), ("user", person)],
                    channel,
                )?
            }
        }
        Operation::Join { channel }
        | Operation::Leave { channel }
        | Operation::Archive { channel } => {
            resolve_channel(c, team, channel)?;
            conversation_id = Some(channel.clone());
            let (action, method) = channel_action;
            if action != "channel-join" {
                reject_general(c, channel)?;
            }
            plan(team, action, method, &[("channel", channel)], channel)?
        }
        Operation::Topic { channel, text } => {
            resolve_channel(c, team, channel)?;
            conversation_id = Some(channel.clone());
            plan(
                team,
                "channel-topic",
                "conversations.setTopic",
                &[("channel", channel), ("topic", text)],
                channel,
            )?
        }
        Operation::Purpose { channel, text } => {
            resolve_channel(c, team, channel)?;
            conversation_id = Some(channel.clone());
            plan(
                team,
                "channel-purpose",
                "conversations.setPurpose",
                &[("channel", channel), ("purpose", text)],
                channel,
            )?
        }
        Operation::Rename { channel, name } => {
            resolve_channel(c, team, channel)?;
            *name = normalized_name(name)?;
            conversation_id = Some(channel.clone());
            plan(
                team,
                "channel-rename",
                "conversations.rename",
                &[("channel", channel), ("name", name)],
                channel,
            )?
        }
        Operation::SetStatus(_) | Operation::ClearStatus => {
            let (action, status) = match &operation {
                Operation::SetStatus(status) => {
                    validate_status(status)?;
                    ("status-set", status.clone())
                }
                _ => ("status-clear", empty_status()),
            };
            if !crate::store::presence::user_exists(c, &self_user_id)? {
                return Err(Error::NotFound(
                    "own profile is not stored; run `flek sync` first".into(),
                ));
            }
            user_id = Some(self_user_id.clone());
            // The real form field is the JSON-serialized profile.
            let profile = serde_json::to_string(&status)?;
            plan(
                team,
                action,
                "users.profile.set",
                &[("profile", &profile)],
                &self_user_id,
            )?
        }
    };
    Ok(Prepared {
        write,
        conversation_id,
        user_id,
        self_user_id,
        operation,
    })
}

fn resolve_channel(c: &Connection, team: &str, channel: &mut String) -> Result<()> {
    let found = references::resolve(c, team, channel, Match::Exact)?.required(channel)?;
    if !matches!(found.kind.as_str(), "public_channel" | "private_channel") {
        return Err(Error::Usage(
            "this operation requires a public or private channel".into(),
        ));
    }
    *channel = found.id;
    Ok(())
}

fn reject_general(c: &Connection, channel: &str) -> Result<()> {
    let general: bool = c.query_row(
        "SELECT COALESCE(json_extract(raw_json, '$.is_general'), 0) FROM conversations WHERE id = ?1",
        [channel], |r| r.get(0),
    )?;
    if general {
        Err(Error::Forbidden)
    } else {
        Ok(())
    }
}

fn normalized_name(name: &str) -> Result<String> {
    let name = channel_browser::normalize_name(name);
    // The existing UI uses Slack's validation codes; the CLI needs a usage exit.
    channel_browser::validate_name(&name).map_err(|_| {
        Error::Usage(
            "channel name must contain 1–80 letters, ASCII digits, hyphens or underscores".into(),
        )
    })?;
    Ok(name)
}

fn empty_status() -> CustomStatus {
    CustomStatus {
        status_text: String::new(),
        status_emoji: String::new(),
        status_expiration: 0,
    }
}

pub fn validate_status(status: &CustomStatus) -> Result<()> {
    if status.status_text.chars().count() > 100 || status.status_expiration < 0 {
        return Err(Error::Usage(
            "status must be at most 100 characters with a non-negative expiration".into(),
        ));
    }
    if !status.status_emoji.is_empty()
        && (!status.status_emoji.starts_with(':')
            || !status.status_emoji.ends_with(':')
            || status.status_emoji.len() < 3
            || !status
                .status_emoji
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_+-:".contains(&b)))
    {
        return Err(Error::Usage(
            "status emoji must be a Slack shortcode such as :calendar:".into(),
        ));
    }
    Ok(())
}

/// Status durations point forward; read-command TIME durations point backward.
pub fn status_expiration(input: &str, now: DateTime<Utc>) -> Result<i64> {
    let bad = || {
        Error::Usage("expiration must be 0/never, future Unix seconds, RFC3339, date or duration (30m/12h/7d)".into())
    };
    if input == "0" || input == "never" {
        return Ok(0);
    }
    let seconds = if let Ok(seconds) = input.parse::<i64>() {
        seconds
    } else if input.ends_with(['m', 'h', 'd']) {
        let multiplier = match input.as_bytes().last() {
            Some(b'm') => 60,
            Some(b'h') => 3600,
            _ => 86400,
        };
        let digits = &input[..input.len() - 1];
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        digits
            .parse::<i64>()
            .ok()
            .and_then(|n| n.checked_mul(multiplier))
            .and_then(|n| now.timestamp().checked_add(n))
            .ok_or_else(bad)?
    } else {
        crate::time::parse_at(input, now)?
            .split('.')
            .next()
            .unwrap()
            .parse::<i64>()
            .map_err(|_| bad())?
    };
    if seconds <= now.timestamp() || DateTime::from_timestamp(seconds, 0).is_none() {
        return Err(bad());
    }
    Ok(seconds)
}

#[derive(Debug, Serialize)]
pub struct Outcome {
    pub conversation_id: Option<String>,
    pub user_id: Option<String>,
    pub warning: Option<String>,
    pub not_stored_locally: bool,
}

/// Execute one prepared plan. Authorization, rate limiting and logging belong to
/// [`Prepared::run_guarded`]; the session identity must still match the offline
/// plan. Rejections never modify the DB.
async fn execute(
    session: &WriteSession,
    store: &Store,
    bus: &Bus,
    plan: Prepared,
) -> Result<Outcome> {
    execute_prepared(session, store, bus, plan)
        .await
        .map_err(classify_rejection)
}

// Keep the legacy Error::Slack exit contract intact for existing callers.
fn classify_rejection(error: Error) -> Error {
    use crate::error::ErrorKind;
    let Error::Slack { code, needed } = error else {
        return error;
    };
    let kind = match code.as_str() {
        "channel_not_found" | "user_not_found" => ErrorKind::NotFound,
        "restricted_action"
        | "restricted_action_read_only_channel"
        | "not_authorized"
        | "no_permission"
        | "missing_scope"
        | "not_in_channel"
        | "cant_kick_self"
        | "cant_kick_from_general"
        | "cant_leave_general"
        | "cant_archive_general"
        | "is_archived"
        | "access_denied"
        | "user_is_restricted"
        | "user_is_ultra_restricted" => ErrorKind::Forbidden,
        "name_taken" | "invalid_arguments" | "invalid_arg_name" | "invalid_arg_value"
        | "invalid_user" => ErrorKind::Usage,
        _ if code.starts_with("invalid_name") => ErrorKind::Usage,
        _ => return Error::Slack { code, needed },
    };
    Error::WriteRejected { code, needed, kind }
}

async fn execute_prepared(
    session: &WriteSession,
    store: &Store,
    bus: &Bus,
    plan: Prepared,
) -> Result<Outcome> {
    let client: &Client = &session.client;
    let identity = session;
    if identity.team_id != plan.write.action.team_id || identity.user_id != plan.self_user_id {
        return Err(Error::Forbidden);
    }
    let mut outcome = Outcome {
        conversation_id: plan.conversation_id.clone(),
        user_id: plan.user_id.clone(),
        warning: None,
        not_stored_locally: false,
    };
    let update = match plan.operation {
        Operation::Create { name, private } => {
            let result =
                channel_browser::create(client, store, bus, &identity.team_id, &name, private, "")
                    .await?;
            outcome.conversation_id = Some(result.id);
            outcome.not_stored_locally = result.warning.as_deref() == Some("not_stored_locally");
            outcome.warning = result.warning;
            if outcome.not_stored_locally {
                return Ok(outcome);
            }
            Update::Member {
                user: identity.user_id.clone(),
                member: true,
                own: true,
            }
        }
        Operation::Join { channel } => {
            let result =
                channel_browser::join(client, store, bus, &identity.team_id, &channel).await?;
            outcome.not_stored_locally = result.warning.as_deref() == Some("not_stored_locally");
            outcome.warning = result.warning;
            if outcome.not_stored_locally {
                return Ok(outcome);
            }
            Update::Member {
                user: identity.user_id.clone(),
                member: true,
                own: true,
            }
        }
        Operation::Invite { channel, person } => {
            client.conversations_invite(&channel, &person).await?;
            Update::Member {
                user: person,
                member: true,
                own: false,
            }
        }
        Operation::Kick { channel, person } => {
            client.conversations_kick(&channel, &person).await?;
            Update::Member {
                user: person,
                member: false,
                own: false,
            }
        }
        Operation::Leave { channel } => {
            client.conversations_leave(&channel).await?;
            Update::Member {
                user: identity.user_id.clone(),
                member: false,
                own: true,
            }
        }
        Operation::Topic { channel, text } => {
            Update::Topic(client.conversations_set_topic(&channel, &text).await?)
        }
        Operation::Purpose { channel, text } => {
            Update::Purpose(client.conversations_set_purpose(&channel, &text).await?)
        }
        Operation::Rename { channel, name } => {
            Update::Rename(client.conversations_rename(&channel, &name).await?)
        }
        Operation::Archive { channel } => {
            client.conversations_archive(&channel).await?;
            Update::Archive
        }
        Operation::SetStatus(status) => {
            outcome.not_stored_locally = !super::presence::set_status_confirmed(
                client,
                store,
                bus,
                &identity.user_id,
                status,
            )
            .await?;
            if outcome.not_stored_locally {
                outcome.warning = Some("not_stored_locally".into());
            }
            return Ok(outcome);
        }
        Operation::ClearStatus => {
            outcome.not_stored_locally = !super::presence::set_status_confirmed(
                client,
                store,
                bus,
                &identity.user_id,
                empty_status(),
            )
            .await?;
            if outcome.not_stored_locally {
                outcome.warning = Some("not_stored_locally".into());
            }
            return Ok(outcome);
        }
    };
    let id = outcome
        .conversation_id
        .clone()
        .expect("channel action has a channel id");
    // Remote acceptance is final; a failed local transaction must not invite a retry.
    outcome.not_stored_locally = store
        .call(move |c| {
            let tx = c.transaction()?;
            channel_management::apply(&tx, &id, update)?;
            tx.commit()?;
            Ok(())
        })
        .await
        .is_err();
    if outcome.not_stored_locally {
        outcome.warning = Some("not_stored_locally".into());
    }
    let _ = bus.send(Change::Directory);
    Ok(outcome)
}
