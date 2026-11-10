//! Local rail views. No network or UI dependencies.
use crate::{reads, Result};
use rusqlite::{params, Connection};
use serde::Serialize;

#[derive(Serialize)]
pub struct DirectMessage {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub partner_id: Option<String>,
    pub members: Vec<String>,
    pub unread: Option<i64>,
    pub ts: Option<String>,
    pub user_id: Option<String>,
    pub text: String,
    pub file_name: Option<String>,
    pub hidden_by_limit: bool,
}

/// One indexed latest-message lookup per conversation, in one SQL query.
pub fn direct_messages(c: &Connection, team: &str) -> Result<Vec<DirectMessage>> {
    let mut stmt = c.prepare(
        "SELECT c.id,COALESCE(c.label,c.name,c.id),c.kind,c.dm_user_id,CASE WHEN c.notification='muted' THEN 0 ELSE c.unread_count END,
            m.ts,m.user_id,COALESCE(m.text,''),COALESCE(m.hidden_by_limit,0),
            (SELECT COALESCE(f.title,f.name) FROM message_files mf JOIN files f ON f.id=mf.file_id
             WHERE mf.conversation_id=c.id AND mf.ts=m.ts ORDER BY f.id LIMIT 1),
            COALESCE(NULLIF((SELECT json_group_array(user_id) FROM conversation_members cm
                      WHERE cm.conversation_id=c.id),'[]'),
                json_extract(c.raw_json,'$.members'),
                (SELECT json_group_array(u.id) FROM users u WHERE u.team_id=c.team_id
                 AND c.kind='mpim' AND (c.name GLOB 'mpdm-'||u.name||'--*'
                   OR c.name GLOB '*--'||u.name||'--*'
                   OR c.name GLOB '*--'||u.name||'-[0-9]*')),'[]')
         FROM conversations c LEFT JOIN users u ON u.id=c.dm_user_id
         LEFT JOIN messages m ON m.id=(SELECT id FROM messages
             WHERE conversation_id=c.id AND deleted=0 ORDER BY ts DESC LIMIT 1)
         WHERE c.team_id=?1 AND c.kind IN ('im','mpim') AND c.is_member=1 AND c.is_archived=0 AND c.closed_at IS NULL
         ORDER BY COALESCE(u.deleted,0),m.ts DESC,c.id",
    )?;
    let rows = stmt.query_map([team], |r| {
        Ok(DirectMessage {
            id: r.get(0)?,
            label: r.get(1)?,
            kind: r.get(2)?,
            partner_id: r.get(3)?,
            unread: r.get(4)?,
            ts: r.get(5)?,
            user_id: r.get(6)?,
            text: r.get(7)?,
            hidden_by_limit: r.get(8)?,
            file_name: r.get(9)?,
            members: serde_json::from_str(&r.get::<_, String>(10)?).unwrap_or_default(),
        })
    })?;
    Ok(rows.collect::<std::result::Result<_, _>>()?)
}

#[derive(Serialize)]
pub struct Activity {
    pub kind: String,
    pub conversation_id: String,
    pub ts: String,
    pub event_ts: String,
    pub thread_ts: Option<String>,
    pub user_id: Option<String>,
    pub text: String,
    pub file_name: Option<String>,
    pub hidden_by_limit: bool,
    pub reaction: Option<String>,
    pub unread: bool,
    pub unread_count: i64,
}

#[derive(Serialize)]
pub struct ActivityPage {
    pub items: Vec<Activity>,
    pub unread_count: i64,
    pub has_more: bool,
}

/// Thread read state is approximated by the root conversation's read marker.
/// Reactions without an observed event time are recent history, always read.
pub fn activity(
    c: &Connection,
    team: &str,
    tab: &str,
    unread: bool,
    limit: usize,
    offset: usize,
) -> Result<ActivityPage> {
    // CROSS JOIN keeps SQLite on the bounded candidate set; otherwise its
    // planner can choose a full conversation-message scan before the limit.
    let sql = format!(
        "WITH me AS (SELECT self_user_id AS id FROM teams WHERE id=?1),
         mine AS MATERIALIZED (
             SELECT DISTINCT conversation_id,COALESCE(thread_ts,ts) AS root
             FROM messages WHERE user_id=(SELECT id FROM me) AND deleted=0
               AND conversation_id IN (SELECT id FROM conversations WHERE team_id=?1)
         ), candidates AS (
             SELECT 'dm' AS kind,m.id,m.user_id AS actor,m.ts AS event_ts,NULL AS reaction,
                 1 AS timed,COALESCE(c.unread_count,0) AS unread_count,
                 m.conversation_id,m.ts
             FROM conversations c JOIN messages m ON m.id=(
                 SELECT id FROM messages WHERE conversation_id=c.id AND deleted=0
                 AND user_id IS NOT (SELECT id FROM me) ORDER BY ts DESC LIMIT 1)
             WHERE c.team_id=?1 AND c.kind IN ('im','mpim')
             UNION ALL
             SELECT 'mention',m.id,m.user_id,m.ts,NULL,1,1,m.conversation_id,m.ts
             {}
             UNION ALL
             SELECT 'thread',m.id,m.user_id,m.ts,NULL,1,1,m.conversation_id,m.ts FROM mine
             JOIN messages m ON m.id=(
                 SELECT id FROM messages WHERE conversation_id=mine.conversation_id
                   AND thread_ts=mine.root AND ts<>mine.root AND deleted=0
                   AND user_id IS NOT (SELECT id FROM me) ORDER BY ts DESC LIMIT 1)
             UNION ALL
             SELECT 'reaction',m.id,r.user_id,COALESCE(r.event_ts,m.ts),r.name,
                 r.event_ts IS NOT NULL,1,m.conversation_id,m.ts
             FROM reactions r CROSS JOIN messages m ON r.conversation_id=m.conversation_id AND r.ts=m.ts
             WHERE m.user_id=(SELECT id FROM me) AND r.user_id<>(SELECT id FROM me)
               AND m.deleted=0
         ), items AS MATERIALIZED (
             SELECT x.*,COALESCE(x.timed AND x.event_ts>c.last_read AND c.notification<>'muted',0) AS unread
             FROM candidates x CROSS JOIN conversations c ON c.id=x.conversation_id
             WHERE c.team_id=?1 AND c.is_member=1 AND c.is_archived=0 AND c.closed_at IS NULL
         ), filtered AS MATERIALIZED (
             SELECT * FROM items WHERE (?3='all' OR kind=?3) AND (NOT ?4 OR unread)
             ORDER BY event_ts DESC,conversation_id,ts,kind,COALESCE(actor,''),
                 COALESCE(reaction,'') LIMIT ?5 OFFSET ?6
         ) SELECT kind,filtered.conversation_id,filtered.ts,event_ts,m.thread_ts,actor,
             COALESCE(m.text,''),m.hidden_by_limit,
             reaction,unread,unread_count,
             (SELECT COALESCE(f.title,f.name) FROM message_files mf JOIN files f ON f.id=mf.file_id
              WHERE mf.conversation_id=filtered.conversation_id AND mf.ts=filtered.ts
              ORDER BY f.id LIMIT 1),
             (SELECT COUNT(*) FROM items WHERE unread)
         FROM filtered CROSS JOIN messages m ON m.id=filtered.id
         ORDER BY event_ts DESC,filtered.conversation_id,filtered.ts,kind,COALESCE(actor,''),
             COALESCE(reaction,'')",
        reads::mention_source("?2", "?1", "(SELECT id FROM me)", "0")
    );
    let me: String = c.query_row(
        "SELECT COALESCE(self_user_id,'') FROM teams WHERE id=?1",
        [team],
        |r| r.get(0),
    )?;
    let token = format!("\"{}\"", me.replace('"', "\"\""));
    let mut stmt = c.prepare(&sql)?;
    let mut count = 0;
    let rows = stmt.query_map(
        params![
            team,
            token,
            tab,
            unread,
            (limit.min(500) + 1) as i64,
            offset as i64
        ],
        |r| {
            count = r.get(12)?;
            Ok(Activity {
                kind: r.get(0)?,
                conversation_id: r.get(1)?,
                ts: r.get(2)?,
                event_ts: r.get(3)?,
                thread_ts: r.get(4)?,
                user_id: r.get(5)?,
                text: r.get(6)?,
                hidden_by_limit: r.get(7)?,
                reaction: r.get(8)?,
                unread: r.get(9)?,
                unread_count: r.get(10)?,
                file_name: r.get(11)?,
            })
        },
    )?;
    let mut items = rows.collect::<std::result::Result<Vec<_>, _>>()?;
    let has_more = items.len() > limit.min(500);
    items.truncate(limit.min(500));
    // An empty filtered tab still needs the global Activity badge.
    if items.is_empty() && (tab != "all" || offset > 0) {
        count = activity(c, team, "all", true, 1, 0)?.unread_count;
    }
    Ok(ActivityPage {
        items,
        unread_count: count,
        has_more,
    })
}
