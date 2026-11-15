//! Offline preparation and remote-first persistence for individual agent writes.
//! The caller must reserve through agent::ActionLog before executing.
use crate::{
    agent::{ActionDescriptor, ActionLog, AgentConfig},
    api::{self, write::WriteRequest, Client},
    references::{self, Match},
    rtm::Event,
    store::Store,
    sync::live,
    views, Error, Result,
};
use rusqlite::Connection;
use serde::Serialize;
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::sync::watch;

#[derive(Clone, Copy)]
pub enum MessageAction {
    React,
    Unreact,
    MarkRead,
    Pin,
    Unpin,
    Save,
}
impl MessageAction {
    pub fn name(self) -> &'static str {
        match self {
            Self::React => "react",
            Self::Unreact => "unreact",
            Self::MarkRead => "mark-read",
            Self::Pin => "pin",
            Self::Unpin => "unpin",
            Self::Save => "save",
        }
    }
}

#[derive(Default)]
pub struct MessageInput {
    pub text: String,
    pub blocks: Option<Vec<Value>>,
    pub thread_ts: Option<String>,
    pub broadcast: bool,
    pub post_at: Option<i64>,
}

#[derive(Serialize)]
pub struct PreparedWrite {
    pub action: ActionDescriptor,
    pub request: WriteRequest,
    pub no_op: bool,
    pub warning: Option<&'static str>,
}

/// Receipt fields are remote identity plus locally read authoritative views.
#[derive(Default, Serialize)]
pub struct WriteReceipt {
    pub action: String,
    pub channel: Option<String>,
    pub ts: Option<String>,
    pub permalink: Option<String>,
    pub scheduled_message_id: Option<String>,
    pub post_at: Option<i64>,
    pub not_stored_locally: bool,
    pub warning: Option<&'static str>,
    pub message: Option<views::Message>,
    pub conversation: Option<views::Conversation>,
}

fn exact_channel(c: &Connection, team: &str, query: &str) -> Result<String> {
    Ok(references::resolve(c, team, query, Match::Exact)?
        .required(query)?
        .id)
}

/// Convert only explicit, exact @handles. Escape all user markup first; generated
/// mention tokens alone bypass escaping. Email addresses, code and existing <...>
/// markup stay literal, preventing accidental mass mentions.
pub fn mention_text(c: &Connection, team: &str, text: &str) -> Result<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    let mut ticks = 0;
    while i < chars.len() {
        if chars[i] == '`' {
            let start = i;
            while i < chars.len() && chars[i] == '`' {
                i += 1;
            }
            let count = i - start;
            if ticks == 0 {
                ticks = count;
            } else if count == ticks {
                ticks = 0;
            }
            out.extend(chars[start..i].iter());
            continue;
        }
        if chars[i] == '@'
            && ticks == 0
            && (i == 0 || chars[i - 1].is_whitespace() || matches!(chars[i - 1], '(' | '[' | '{'))
        {
            let start = i + 1;
            let mut end = start;
            while end < chars.len()
                && (chars[end].is_alphanumeric() || matches!(chars[end], '_' | '-' | '.'))
            {
                end += 1;
            }
            // A sentence-ending period is punctuation, not part of a handle.
            while end > start && chars[end - 1] == '.' {
                end -= 1;
            }
            if end > start {
                let name: String = chars[start..end].iter().collect();
                let user = references::resolve_user(c, team, &name, Match::Exact)?;
                if user.deleted || !crate::agent::valid_id(&user.id, "UW") {
                    return Err(Error::Usage("mention must identify an active user".into()));
                }
                out.push_str(&format!("<@{}>", user.id));
                i = end;
                continue;
            }
        }
        out.push_str(&api::chat::escape_text(&chars[i].to_string()));
        i += 1;
    }
    Ok(out)
}

impl PreparedWrite {
    pub fn message(
        c: &Connection,
        team: &str,
        channel: &str,
        mut input: MessageInput,
        config: &AgentConfig,
    ) -> Result<Self> {
        config.validate()?;
        let channel = exact_channel(c, team, channel)?;
        if input
            .thread_ts
            .as_deref()
            .is_some_and(|t| !references::valid_ts(t))
            || (input.broadcast && input.thread_ts.is_none())
        {
            return Err(Error::Usage(
                "invalid thread timestamp or broadcast without thread".into(),
            ));
        }
        if let Some(at) = input.post_at {
            validate_schedule(at, chrono::Utc::now().timestamp())?;
        }
        if input.text.trim().is_empty() && input.blocks.as_ref().is_none_or(|b| b.is_empty()) {
            return Err(Error::Usage("message needs text or blocks".into()));
        }
        if let Some(signature) = &config.signature {
            input.text.push_str("\n\n");
            input.text.push_str(signature);
            if let Some(blocks) = &mut input.blocks {
                blocks.push(
                    json!({"type":"context","elements":[{"type":"plain_text","text":signature}]}),
                );
            }
        }
        if input
            .blocks
            .as_ref()
            .is_some_and(|b| b.is_empty() || b.len() > 50 || b.iter().any(|v| !v.is_object()))
        {
            return Err(Error::Usage(
                "blocks must be an array of 1..50 objects (including the optional signature)"
                    .into(),
            ));
        }
        let text = mention_text(c, team, &input.text)?;
        let name = if input.post_at.is_some() {
            "schedule"
        } else {
            "send"
        };
        let method = if input.post_at.is_some() {
            "chat.scheduleMessage"
        } else {
            "chat.postMessage"
        };
        let mut request = WriteRequest::new(method, &[("channel", &channel), ("text", &text)]);
        if let Some(blocks) = input.blocks {
            request
                .params
                .insert("blocks".into(), serde_json::to_string(&blocks)?);
        }
        if let Some(thread) = input.thread_ts {
            request.params.insert("thread_ts".into(), thread);
        }
        if input.broadcast {
            request
                .params
                .insert("reply_broadcast".into(), "true".into());
        }
        if let Some(at) = input.post_at {
            request.params.insert("post_at".into(), at.to_string());
        } else {
            request
                .params
                .insert("client_msg_id".into(), api::chat::new_client_msg_id());
        }
        Self::new(name, team, vec![channel], request, false, None)
    }

    pub fn message_action(
        c: &Connection,
        team: &str,
        action: MessageAction,
        reference: &str,
        ts: Option<&str>,
        emoji: Option<&str>,
    ) -> Result<Self> {
        let parsed = if let Some(ts) = ts {
            if !references::valid_ts(ts) {
                return Err(Error::Usage(
                    "expected seconds.microseconds timestamp".into(),
                ));
            }
            references::MessageReference {
                conversation_id: exact_channel(c, team, reference)?,
                ts: ts.into(),
                thread_ts: None,
            }
        } else {
            let parsed = references::parse_permalink(reference)
                .ok_or_else(|| Error::Usage("provide a message permalink or CHANNEL TS".into()))?;
            // A permalink must still belong to the selected, locally known team.
            exact_channel(c, team, &parsed.conversation_id)?;
            parsed
        };
        let channel = &parsed.conversation_id;
        let method = match action {
            MessageAction::React => "reactions.add",
            MessageAction::Unreact => "reactions.remove",
            MessageAction::MarkRead => "conversations.mark",
            MessageAction::Pin => "pins.add",
            MessageAction::Unpin => "pins.remove",
            MessageAction::Save => "stars.add",
        };
        let key = if matches!(action, MessageAction::MarkRead) {
            "ts"
        } else {
            "timestamp"
        };
        let mut request = WriteRequest::new(method, &[("channel", channel), (key, &parsed.ts)]);
        if matches!(action, MessageAction::React | MessageAction::Unreact) {
            let name = emoji.unwrap_or_default().trim_matches(':');
            if name.is_empty()
                || !name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+'))
            {
                return Err(Error::Usage("provide an emoji name, e.g. thumbsup".into()));
            }
            request.params.insert("name".into(), name.into());
        }
        let no_op = matches!(action, MessageAction::MarkRead)
            && c.query_row(
                "SELECT last_read >= ?2 FROM conversations WHERE id = ?1",
                rusqlite::params![channel, parsed.ts],
                |r| r.get::<_, Option<bool>>(0),
            )?
            .unwrap_or(false);
        let warning = matches!(action, MessageAction::Save)
            .then_some("legacy_star_only_later_api_unavailable");
        Self::new(
            action.name(),
            team,
            vec![channel.clone()],
            request,
            no_op,
            warning,
        )
    }

    pub fn dm(c: &Connection, team: &str, people: &[String]) -> Result<Self> {
        if people.is_empty() || people.len() > 8 {
            return Err(Error::Usage(
                "dm takes 1..8 people; mass actions are not supported".into(),
            ));
        }
        let mut ids = Vec::new();
        for person in people {
            let u = references::resolve_user(c, team, person, Match::Exact)?;
            if u.deleted {
                return Err(Error::Usage("cannot open a DM with a deleted user".into()));
            }
            if !ids.contains(&u.id) {
                ids.push(u.id);
            }
        }
        let users = ids.join(",");
        Self::new(
            "dm",
            team,
            ids,
            WriteRequest::new(
                "conversations.open",
                &[("users", &users), ("return_im", "true")],
            ),
            false,
            None,
        )
    }

    fn new(
        name: &str,
        team: &str,
        targets: Vec<String>,
        request: WriteRequest,
        no_op: bool,
        warning: Option<&'static str>,
    ) -> Result<Self> {
        let action = ActionDescriptor {
            action: name.into(),
            team_id: team.into(),
            targets,
        };
        action.validate()?;
        Ok(Self {
            action,
            request,
            no_op,
            warning,
        })
    }

    /// Execute exactly the prepared form; authentication fields are transport-owned.
    /// Remote acceptance remains success when local persistence/permalink lookup fails.
    pub async fn execute(
        &self,
        client: &Client,
        store: &Store,
        user: &str,
    ) -> Result<WriteReceipt> {
        if self.action.action == "schedule" {
            let at = self.request.params["post_at"]
                .parse()
                .map_err(|_| Error::Usage("invalid post_at".into()))?;
            validate_schedule(at, chrono::Utc::now().timestamp())?;
        }
        if self.action.action == "mark-read" {
            crate::sync::read::mark(
                client,
                store,
                &live::bus(),
                &self.action.targets[0],
                &self.request.params["ts"],
            )
            .await?;
            return Ok(WriteReceipt {
                action: self.action.action.clone(),
                channel: Some(self.action.targets[0].clone()),
                ts: Some(self.request.params["ts"].clone()),
                ..Default::default()
            });
        }
        let raw = self.request.execute(client).await?;
        let mut receipt = WriteReceipt {
            action: self.action.action.clone(),
            channel: self.request.params.get("channel").cloned(),
            warning: self.warning,
            ..Default::default()
        };
        match self.action.action.as_str() {
            "send" => {
                self.persist_sent(client, store, user, raw, &mut receipt)
                    .await?
            }
            "schedule" => {
                receipt.scheduled_message_id = raw
                    .get("scheduled_message_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                receipt.post_at = self.request.params["post_at"].parse().ok();
            }
            "dm" => self.persist_dm(store, user, raw, &mut receipt).await?,
            "react" | "unreact" => {
                let channel = self.action.targets[0].clone();
                let ts = self.request.params["timestamp"].clone();
                let name = self.request.params["name"].clone();
                let event = if self.action.action == "react" {
                    Event::ReactionAdded {
                        channel,
                        ts,
                        name,
                        user: user.into(),
                        event_ts: None,
                    }
                } else {
                    Event::ReactionRemoved {
                        channel,
                        ts,
                        name,
                        user: user.into(),
                    }
                };
                receipt.ts = Some(self.request.params["timestamp"].clone());
                receipt.not_stored_locally = !matches!(
                    live::apply(store, event).await,
                    Ok(live::Applied::Changed(_))
                );
            }
            "pin" | "unpin" | "save" => {
                let channel = self.action.targets[0].clone();
                let ts = self.request.params["timestamp"].clone();
                receipt.ts = Some(ts.clone());
                let action = self.action.action.clone();
                let user = user.to_owned();
                receipt.not_stored_locally = store
                    .call(move |c| {
                        crate::store::message_actions::confirmed(c, &action, &channel, &ts, &user)
                    })
                    .await
                    .is_err();
            }
            _ => return Err(Error::Forbidden),
        }
        Ok(receipt)
    }

    async fn persist_sent(
        &self,
        client: &Client,
        store: &Store,
        user: &str,
        raw: Value,
        receipt: &mut WriteReceipt,
    ) -> Result<()> {
        let ts = raw
            .get("ts")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                Error::Store(
                    "send accepted without a timestamp; check history before retrying".into(),
                )
            })?
            .to_owned();
        let channel = raw
            .get("channel")
            .and_then(Value::as_str)
            .unwrap_or(&self.action.targets[0])
            .to_owned();
        receipt.ts = Some(ts.clone());
        receipt.channel = Some(channel.clone());
        let mut message = raw
            .get("message")
            .filter(|v| v.is_object())
            .cloned()
            .unwrap_or_else(|| json!({}));
        let obj = message.as_object_mut().unwrap();
        for (key, value) in [
            ("ts", ts.as_str()),
            ("type", "message"),
            ("user", user),
            ("text", self.request.params["text"].as_str()),
        ] {
            obj.entry(key).or_insert_with(|| value.into());
        }
        for key in ["client_msg_id", "thread_ts"] {
            if let Some(v) = self.request.params.get(key) {
                obj.entry(key).or_insert_with(|| v.clone().into());
            }
        }
        if let Some(blocks) = self.request.params.get("blocks") {
            obj.entry("blocks").or_insert(serde_json::from_str(blocks)?);
        }
        if self.request.params.contains_key("reply_broadcast") {
            obj.entry("subtype")
                .or_insert_with(|| "thread_broadcast".into());
        }
        let applied = match serde_json::from_value::<api::Message>(message.clone()) {
            Ok(mut m) => {
                m.raw = message;
                live::apply(
                    store,
                    Event::Message {
                        channel: channel.clone(),
                        message: m,
                    },
                )
                .await
                .ok()
            }
            Err(_) => None,
        };
        receipt.not_stored_locally = !matches!(applied, Some(live::Applied::Changed(_)));
        receipt.message = store
            .with_read(|c| views::message(c, &channel, &ts))
            .ok()
            .flatten();
        receipt.not_stored_locally |= receipt.message.is_none();
        receipt.permalink = receipt.message.as_ref().and_then(|m| m.permalink.clone());
        if receipt.permalink.is_none() {
            receipt.permalink = client.chat_get_permalink(&channel, &ts).await.ok();
            if receipt.permalink.is_none() {
                receipt.warning = Some("permalink_unavailable_message_accepted");
            }
        }
        Ok(())
    }

    async fn persist_dm(
        &self,
        store: &Store,
        user: &str,
        raw: Value,
        receipt: &mut WriteReceipt,
    ) -> Result<()> {
        let mut channel = raw
            .get("channel")
            .filter(|v| v.is_object())
            .cloned()
            .ok_or_else(|| {
                Error::Store("DM accepted without a conversation; sync before retrying".into())
            })?;
        let obj = channel.as_object_mut().unwrap();
        let single = self.action.targets.len() == 1;
        obj.entry("is_im").or_insert(single.into());
        obj.entry("is_mpim").or_insert((!single).into());
        if single {
            obj.entry("user")
                .or_insert_with(|| self.action.targets[0].clone().into());
        }
        let mut conv: api::Conversation = serde_json::from_value(channel.clone())?;
        conv.raw = channel;
        let id = conv.id.clone();
        receipt.channel = Some(id.clone());
        let team = self.action.team_id.clone();
        let user = user.to_owned();
        receipt.not_stored_locally = store
            .call(move |c| {
                crate::store::directory::upsert_conversations(c, &team, &user, &[conv])?;
                c.execute(
                    "UPDATE conversations SET closed_at = NULL WHERE id = ?1",
                    [&id],
                )?;
                Ok(())
            })
            .await
            .is_err();
        receipt.conversation = store
            .with_read(|c| views::conversation(c, receipt.channel.as_deref().unwrap()))
            .ok()
            .flatten();
        receipt.not_stored_locally |= receipt.conversation.is_none();
        Ok(())
    }
}

/// Authenticated identity a guarded write runs as.
pub struct WriteSession {
    pub client: Client,
    pub team_id: String,
    pub user_id: String,
}

impl PreparedWrite {
    /// The exact prepared form plus whether the policy would authorize it. Needs no
    /// credentials, no network and touches neither the action log nor the rate limit.
    pub fn dry_run_report(&self, config: &AgentConfig, yes: bool) -> Value {
        json!({
            "dry_run": true,
            "authorized": config.authorize(&self.action, yes).is_ok(),
            "write": self,
        })
    }

    /// The only path to a real write: see [`guarded`].
    pub async fn run_guarded<F, Fut>(
        &self,
        log: &ActionLog,
        config: &AgentConfig,
        yes: bool,
        store: &Store,
        session: F,
    ) -> Result<WriteReceipt>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<WriteSession>>,
    {
        guarded(
            log,
            config,
            yes,
            &self.action,
            session,
            |session| async move { self.execute(&session.client, store, &session.user_id).await },
        )
        .await
    }
}

/// Policy check and atomic rate-limit reservation come first, the session is
/// resolved lazily afterwards and must belong to the descriptor's team, and the
/// outcome is logged as a stable category only (never request data or error
/// text). Every write, including channel management, runs through here.
pub async fn guarded<T, F, Fut, E, EFut>(
    log: &ActionLog,
    config: &AgentConfig,
    yes: bool,
    action: &ActionDescriptor,
    session: F,
    execute: E,
) -> Result<T>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<WriteSession>>,
    E: FnOnce(WriteSession) -> EFut,
    EFut: std::future::Future<Output = Result<T>>,
{
    let reservation = log.reserve(config, action, yes)?;
    let result = async {
        let session = session().await?;
        if session.team_id != action.team_id {
            return Err(Error::Forbidden);
        }
        execute(session).await
    }
    .await;
    let status = result.as_ref().map(|_| ()).map_err(Error::kind);
    // Remote acceptance must never become a retryable failure because the
    // audit completion failed; the entry then stays `pending`.
    if reservation.finish(status).is_err() {
        tracing::warn!("action-log completion unavailable; entry remains pending");
    }
    result
}

pub fn validate_schedule(at: i64, now: i64) -> Result<()> {
    if at <= now || at > now.saturating_add(120 * 86400) {
        return Err(Error::Usage(
            "post_at must be in the future and within 120 days".into(),
        ));
    }
    Ok(())
}

/// What an upload would share. File bytes never appear here, only metadata.
#[derive(Clone, Serialize)]
pub struct UploadPlan {
    pub filename: String,
    pub size: u64,
    pub channel: String,
    pub thread_ts: Option<String>,
    pub comment: String,
}

/// One file shared to one conversation (or one thread). Prepared offline from a
/// stat of the file and the local database; the file is only read after the policy
/// check and the rate-limit reservation succeeded.
#[derive(Serialize)]
pub struct PreparedUpload {
    pub action: ActionDescriptor,
    pub upload: UploadPlan,
    #[serde(skip)]
    path: PathBuf,
}

/// Upload receipt: the guard fields plus the file identity. `message` is always
/// read from SQLite and `ts` stays null until a real history/live message exists.
#[derive(Default, Serialize)]
pub struct UploadReceipt {
    pub action: String,
    pub channel: Option<String>,
    pub conversation_id: String,
    pub file_id: String,
    pub ts: Option<String>,
    pub permalink: Option<String>,
    pub not_stored_locally: bool,
    /// The final response was ambiguous and no stored message was found: an
    /// uncertain remote write, never a reason to upload again.
    pub completion_unknown: bool,
    pub warning: Option<&'static str>,
    pub message: Option<views::Message>,
}

const MAX_UPLOAD_COMMENT_CHARS: usize = 40_000;

impl PreparedUpload {
    pub const ACTION: &'static str = "upload";

    pub fn prepare(
        c: &Connection,
        team: &str,
        path: &std::path::Path,
        channel: &str,
        thread_ts: Option<String>,
        comment: &str,
        config: &AgentConfig,
    ) -> Result<Self> {
        config.validate()?;
        let channel = exact_channel(c, team, channel)?;
        if thread_ts
            .as_deref()
            .is_some_and(|t| !references::valid_ts(t))
        {
            return Err(Error::Usage("invalid thread timestamp".into()));
        }
        let metadata = std::fs::metadata(path)?;
        if !metadata.is_file() {
            return Err(Error::Usage("upload requires a regular file".into()));
        }
        let filename = path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| Error::Usage("upload filename must be UTF-8".into()))?
            .to_owned();
        crate::sync::upload::validate_file(&filename, metadata.len())?;
        let mut comment = comment.to_owned();
        if let Some(signature) = &config.signature {
            if !comment.trim().is_empty() {
                comment.push_str("\n\n");
            }
            comment.push_str(signature);
        }
        if comment.chars().count() > MAX_UPLOAD_COMMENT_CHARS {
            return Err(Error::Usage("upload comment is too long".into()));
        }
        let action = ActionDescriptor {
            action: Self::ACTION.into(),
            team_id: team.into(),
            targets: vec![channel.clone()],
        };
        action.validate()?;
        Ok(Self {
            action,
            upload: UploadPlan {
                filename,
                size: metadata.len(),
                channel,
                thread_ts,
                comment,
            },
            path: path.to_owned(),
        })
    }

    /// Same contract as [`PreparedWrite::dry_run_report`]: no credentials, network,
    /// file read, log entry or rate-limit slot.
    pub fn dry_run_report(&self, config: &AgentConfig, yes: bool) -> Value {
        json!({
            "dry_run": true,
            "authorized": config.authorize(&self.action, yes).is_ok(),
            "write": self,
        })
    }

    /// Policy and rate limit first, then the private local snapshot, then the
    /// session. An ambiguous completion leaves the log entry `pending` (the file
    /// may already be shared) instead of recording it as accepted.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_guarded<F, Fut>(
        &self,
        log: &ActionLog,
        config: &AgentConfig,
        yes: bool,
        store: &Store,
        cancel: watch::Receiver<bool>,
        progress: crate::sync::upload::Progress,
        session: F,
    ) -> Result<UploadReceipt>
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = Result<WriteSession>>,
    {
        let reservation = log.reserve(config, &self.action, yes)?;
        let result = async {
            let mut upload = crate::sync::upload::Upload::from_path(&self.path).await?;
            let session = session().await?;
            if session.team_id != self.action.team_id {
                return Err(Error::Forbidden);
            }
            let uploaded = upload
                .send(
                    &session.client,
                    store,
                    crate::sync::upload::Destination {
                        conversation_id: self.upload.channel.clone(),
                        thread_ts: self.upload.thread_ts.clone(),
                        comment: self.upload.comment.clone(),
                        blocks: None,
                    },
                    cancel,
                    progress,
                )
                .await?;
            let channel = uploaded.conversation_id.clone();
            let message = match &uploaded.ts {
                Some(ts) => store
                    .with_read(|c| views::message(c, &channel, ts))
                    .ok()
                    .flatten(),
                None => None,
            };
            let permalink = match (&uploaded.ts, &message) {
                (Some(_), Some(m)) if m.permalink.is_some() => m.permalink.clone(),
                (Some(ts), _) => session.client.chat_get_permalink(&channel, ts).await.ok(),
                _ => None,
            };
            Ok(UploadReceipt {
                action: Self::ACTION.into(),
                channel: Some(channel.clone()),
                conversation_id: channel,
                file_id: uploaded.file_id,
                ts: uploaded.ts,
                permalink,
                not_stored_locally: message.is_none(),
                completion_unknown: uploaded.completion_unknown,
                warning: uploaded
                    .completion_unknown
                    .then_some("completion_unconfirmed_check_history_before_retrying"),
                message,
            })
        }
        .await;
        match &result {
            Ok(receipt) if receipt.completion_unknown => drop(reservation),
            other => {
                let status = other.as_ref().map(|_| ()).map_err(Error::kind);
                if reservation.finish(status).is_err() {
                    tracing::warn!("action-log completion unavailable; entry remains pending");
                }
            }
        }
        result
    }
}
