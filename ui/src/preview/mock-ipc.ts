import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { defaultSettings, type Settings } from "../models/settings-model";
import { fileType } from "../models/rail-model";
import type { Conversation, Status } from "../api";
import { people, conversations, now, msg, ROOT, replies, messages, bigMessages } from "./messages";
import { hits, profiles, elsewhere, presence, closed, directRows, activityRows, storedFiles } from "./directory";
import { pdf, svgFixture } from "./assets";
import { spreadsheetFixture, wordFixture } from "./documents";

const w = window as unknown as { __sendDelay: number };
w.__sendDelay = 700;

/// send_message: like the app, stores the message, announces it, then
/// answers. Text containing "fail" is refused like Slack would.
async function sendMessage(a: Record<string, unknown>) {
  await new Promise((r) => setTimeout(r, w.__sendDelay));
  const text = String(a.text);
  if (text.includes("fail")) throw "You are not a member of this channel";
  const esc = text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  const ts = `${Math.floor(Date.now() / 1000)}.${String(Date.now() % 1000).padStart(3, "0")}100`;
  const conv = String(a.conversationId);
  const threadTs = (a.threadTs as string | null) ?? null;
  const m = msg(0, "U3", esc, {
    ts,
    client_msg_id: String(a.clientMsgId),
    blocks: a.blocks ? JSON.parse(JSON.stringify(a.blocks)) : undefined,
    thread_ts: threadTs,
    subtype: threadTs && a.broadcast ? "thread_broadcast" : null,
  });
  if (conv === "CBIG") bigMessages.push(m);
  else if (conv !== "C1") elsewhere.push({ ...m, conversation_id: conv });
  else if (threadTs === ROOT) {
    replies.push(m);
    if (a.broadcast) messages.push(m);
    const root = messages.find((x) => x.ts === ROOT)!;
    root.reply_count = replies.length;
    root.latest_reply = ts;
  } else messages.push(m);
  emit("flek://change", { type: "message", data: { conversation_id: conv, ts } });
  return { conversation_id: conv, ts, client_msg_id: m.client_msg_id, not_stored_locally: false };
}

const downloadPreview = window as unknown as {
  __downloadCalls: string[];
  __savedDownload?: { fileId: string; token: string };
  __revealedDownload?: string;
  __downloadFailure?: "directory" | "save";
};
downloadPreview.__downloadCalls = [];
const downloadReceipts = new Map<string, string>();
let nextReceipt = 0;

async function fetchFile(fileId: string, reveal = false) {
  await new Promise((r) => setTimeout(r, 600));
  if (fileId === "FJPEG_FAIL") throw "file_unavailable";
  const stored = storedFiles().find((f) => f.id === fileId);
  if (stored?.hidden_by_limit) throw "file_unavailable";
  for (const m of [...messages, ...bigMessages, ...replies]) for (const f of m.files) {
    if (f.id !== fileId) continue;
    f.local_path = `/synthetic/${f.name ?? "download"}`;
    emit("flek://change", { type: "message", data: { conversation_id: "C1", ts: m.ts } });
  }
  if (reveal)
    (window as unknown as { __openedFile: string }).__openedFile = String(fileId);
  const source = [...messages, ...bigMessages, ...replies].find((m) => m.files.some((f) => f.id === fileId));
  const file = source?.files.find((f) => f.id === fileId);
  if (!file || !source) throw "file_unavailable";
  return structuredClone({
    ...file,
    file_permalink: file.permalink,
    conversation_id: bigMessages.includes(source) ? "CBIG" : "C1",
    ts: source.ts,
    user_id: source.user_id,
  });
}

export const query = new URLSearchParams(window.location.search);
let previewSettings: Settings = {
  ...defaultSettings,
  language: query.get("lang") === "uk" ? "uk" : "en",
  load_error: query.has("settingsError") ? "settings_unavailable" : null,
  desktop_import_on_start: !query.has("settingsError"),
};
const readyStatus: Status = {
  phase: "ready", team_id: "T1", team: "Demo Team", user_id: "U3", user: "me",
};
let previewStatus: Status = query.has("signin")
  ? { phase: "sign_in", desktop: true, reason: "token_revoked" }
  : readyStatus;

function setPreviewStatus(status: Status) {
  previewStatus = status;
  emit("flek://status", status);
}

// Exercise failure and recovery through the same events as the native forwarder.
(window as unknown as { __settingsError: (failed: boolean) => void }).__settingsError = (failed) => {
  previewSettings.load_error = failed ? "settings_unavailable" : null;
  if (failed) previewSettings.desktop_import_on_start = false;
  emit("flek://change", { type: "settings_error", data: { error: previewSettings.load_error } });
  emit("flek://settings", { ...previewSettings });
};

mockIPC(
  async (cmd, args) => {
    const a = args as Record<string, string | null>;
    switch (cmd) {
      case "get_settings":
        return { ...previewSettings };
      case "set_settings": {
        if (previewSettings.load_error) throw "settings_unavailable";
        previewSettings = JSON.parse(JSON.stringify((args as { settings: Settings }).settings));
        emit("flek://settings", previewSettings);
        return { ...previewSettings };
      }
      case "storage_info":
        return { db_path: "/demo/flek/flek.db", db_bytes: 5242880, cache_bytes: 10485760 };
      case "account_origin":
        return "desktop";
      case "show_storage":
        return;
      case "clear_file_cache":
        for (const m of messages)
          for (const f of m.files) {
            f.local_path = null;
            f.preview_path = null;
          }
        emit("flek://change", { type: "directory" });
        return;
      case "full_resync":
        emit("flek://change", { type: "progress", data: { stage: "idle" } });
        return;
      case "sign_out":
        previewSettings.desktop_import_on_start = false;
        emit("flek://settings", { ...previewSettings });
        setPreviewStatus({ phase: "sign_in", desktop: true, reason: null });
        return;
      case "import_desktop":
        if (!previewSettings.load_error) {
          previewSettings.desktop_import_on_start = true;
          emit("flek://settings", { ...previewSettings });
        }
        setPreviewStatus(readyStatus);
        return;
      case "sign_in":
        setPreviewStatus(readyStatus);
        return;
      case "team_icon":
        return "a1";
      case "direct_messages":
        return directRows();
      case "activity": {
        const request = args as { tab: string; unread: boolean; limit: number; offset: number };
        const currentActivity = activityRows
          .filter((r) => !closed.has(r.conversation_id))
          .map((r) => ({
            ...r,
            unread:
              r.unread &&
              conversations.find((c) => c.id === r.conversation_id)?.notification !== "muted",
          }));
        const rows = currentActivity.filter(
          (r) => (request.tab === "all" || r.kind === request.tab) && (!request.unread || r.unread),
        );
        return {
          items: rows.slice(request.offset, request.offset + request.limit),
          unread_count: currentActivity.filter((r) => r.unread).length,
          has_more: rows.length > request.offset + request.limit,
        };
      }
      case "browse_files": {
        const request = args as {
          query: string;
          kind: string | null;
          sort: string;
          mine: boolean;
          offset: number;
        };
        const rows = storedFiles().filter(
          (f) =>
            (!request.mine || f.user_id === "U3") &&
            (!request.kind || fileType(f.mimetype) === request.kind) &&
            `${f.name ?? ""} ${f.title ?? ""}`.toLowerCase().includes(request.query.toLowerCase()),
        );
        rows.sort((a, b) => {
          if (request.sort === "name") return (a.name ?? "").localeCompare(b.name ?? "");
          if (request.sort === "size") return (b.size ?? 0) - (a.size ?? 0);
          return (b.ts ?? "").localeCompare(a.ts ?? "");
        });
        return rows.slice(request.offset, request.offset + 100);
      }
      case "fetch_file": return fetchFile(String(a.fileId), Boolean((args as { reveal: boolean }).reveal));
      case "download_file": {
        const fileId = String(a.fileId);
        downloadPreview.__downloadCalls.push(fileId);
        if (downloadPreview.__downloadFailure === "directory") throw "downloads_unavailable";
        await fetchFile(fileId);
        if (downloadPreview.__downloadFailure === "save") throw "downloads_save_failed";
        const token = `preview-download-${++nextReceipt}`;
        downloadReceipts.set(token, fileId);
        if (downloadReceipts.size > 100) downloadReceipts.delete(downloadReceipts.keys().next().value!);
        downloadPreview.__savedDownload = { fileId, token };
        return token;
      }
      case "reveal_download": {
        const fileId = downloadReceipts.get(String(a.token));
        if (!fileId) throw "unknown download receipt";
        downloadPreview.__revealedDownload = fileId;
        return;
      }
      case "open_url":
        // Inspect the last native-opener request without leaving the preview.
        (window as unknown as { __openedUrl: string }).__openedUrl = String(a.url);
        return;
      case "subscribe_presence":
        return;
      case "presence_for":
        return JSON.parse(
          JSON.stringify((args as { ids: string[] }).ids.map((id) => presence[id]).filter(Boolean)),
        );
      case "statuses_for":
        return JSON.parse(
          JSON.stringify(
            Object.fromEntries(
              (args as { ids: string[] }).ids.map((id) => [id, profiles[id] ?? null]),
            ),
          ),
        );
      case "set_status":
      case "clear_status": {
        const status =
          cmd === "clear_status"
            ? { status_text: "", status_emoji: "", status_expiration: 0 }
            : (args as { status: object }).status;
        Object.assign(profiles.U3, status);
        emit("flek://change", { type: "directory" });
        return;
      }
      case "set_presence": {
        const away = (args as { away: boolean }).away;
        Object.assign(presence.U3, { manual_away: away, presence: away ? "away" : "active" });
        emit("flek://change", { type: "presence" });
        return;
      }
      case "snooze":
      case "end_snooze": {
        const snooze = cmd === "snooze";
        Object.assign(presence.U3, {
          snooze_enabled: snooze,
          snooze_end: snooze
            ? Math.floor(Date.now() / 1000) + Number((args as { minutes: number }).minutes) * 60
            : 0,
        });
        emit("flek://change", { type: "presence" });
        return;
      }
      case "status":
        return previewStatus;
      case "progress":
        return { stage: "conversations", done: 7, total: 28, current: "general", deep: true };
      case "conversations":
        return structuredClone(conversations.filter((c) => !closed.has(c.id)));
      case "activity_seen": {
        for (const row of activityRows) {
          const thread = row.kind === "reaction" ? null : row.thread_ts;
          if (
            row.conversation_id === a.conversationId &&
            (thread ?? null) === (a.threadTs ?? null) &&
            row.event_ts <= String(a.ts)
          )
            row.unread = false;
        }
        emit("flek://change", { type: "read_marker", data: { conversation_id: a.conversationId } });
        return null;
      }
      case "mark_read": {
        const c = conversations.find((c) => c.id === a.conversationId);
        if (c && (!c.last_read || String(a.ts) > c.last_read)) {
          c.last_read = String(a.ts);
          c.unread = 0;
          for (const row of activityRows) {
            if (row.conversation_id === c.id && row.event_ts <= c.last_read) row.unread = false;
          }
          emit("flek://change", { type: "read_marker", data: { conversation_id: c.id } });
        }
        return;
      }
      case "conversation_preference": {
        const c = conversations.find((c) => c.id === a.conversationId);
        if (c) {
          if (typeof a.starred === "boolean") c.starred = a.starred;
          if (a.notification)
            c.notification = String(a.notification) as Conversation["notification"];
        }
        emit("flek://change", { type: "directory" });
        return;
      }
      case "dismiss_conversation":
        closed.add(String(a.conversationId));
        emit("flek://change", { type: "directory" });
        return;
      case "conversation_link":
        return `https://demo.slack.com/archives/${a.conversationId}`;
      case "channel_info": {
        const c = conversations.find((c) => c.id === a.conversationId)!;
        return {
          conversation: { ...c, name: c.label, purpose: "Synthetic preview channel" },
          created: now - 86400 * 30,
          creator: "U1",
          num_members: 3,
          members: ["U1", "U2", "U3"].map((id) => ({ id, name: people[id] })),
          pins: [{ ts: ROOT, text: "Pinned preview message" }],
        };
      }
      case "people":
        return people;
      case "avatars":
        return { U1: "a1", U4: "a4" };
      case "custom_emoji":
        return {
          images: { custom_party: "e1", shipit: "e2", gdocs: "e2" },
          aliases: { yes: "thumbsup" },
        };
      case "profile":
        return structuredClone(profiles[String(a.userId)] ?? null);
      case "profile_photo":
        // The large photo takes a moment, like a real download.
        await new Promise((r) => setTimeout(r, 600));
        return profiles[String(a.userId)]?.has_photo ? "big1" : null;
      case "open_file":
        (window as unknown as { __openedFile: string }).__openedFile = String(a.fileId);
        return;
      case "document_data":
        if (a.fileId === "FSVG" || a.fileId === "FSVG_THUMB") return new TextEncoder().encode(svgFixture).buffer;
        if (a.fileId === "FXLSX_LIMIT") throw "document_too_large";
        if (a.fileId === "FDOCX_BROKEN") return new Uint8Array([1, 2, 3]).buffer;
        if (a.fileId === "FDOCX" || a.fileId === "FDOCX_REMOTE") return wordFixture();
        if (a.fileId === "FXLSX" || a.fileId === "FXLSX_LARGE") return spreadsheetFixture(a.fileId === "FXLSX_LARGE");
        return Uint8Array.from(pdf, (c) => c.charCodeAt(0)).buffer;
      // Copies, like real IPC: stores wrap what they get, and __reply below
      // must not mutate it behind their back.
      case "thread":
        return structuredClone(
          a.ts === ROOT ? [messages.find((m) => m.ts === ROOT), ...replies] : [],
        );
      case "message":
        return structuredClone(
          [...messages, ...bigMessages, ...replies, ...elsewhere].find((m) => m.ts === a.ts) ?? null,
        );
      case "messages": {
        const rows = timelineRows(String(a.conversationId));
        // Explicit stress mode mounts 10k stored rows at once; normal mode pages.
        if (query.has("stress") && a.conversationId === "CBIG" && !a.before) return structuredClone(rows);
        return structuredClone(rows.filter((m) => !a.before || m.ts < a.before).slice(-Number(a.limit)));
      }
      case "timeline_window": {
        const rows = timelineRows(String(a.conversationId));
        const target = String(a.target);
        const half = Math.floor(Math.min(500, Math.max(2, Number(a.limit))) / 2);
        const older = rows.filter((m) => m.ts < target);
        const newer = rows.filter((m) => m.ts >= target);
        return structuredClone({ rows: [...older.slice(-half), ...newer.slice(0, half)], has_older: older.length > half, has_newer: newer.length > half });
      }
      case "timeline_after":
        return structuredClone(timelineRows(String(a.conversationId)).filter((m) => m.ts > String(a.after)).slice(0, Number(a.limit)));
      case "timeline_range":
        return structuredClone(timelineRows(String(a.conversationId)).filter((m) => m.ts >= String(a.start) && m.ts <= String(a.end)));
      case "date_target": {
        const rows = timelineRows(String(a.conversationId));
        if (rows.length && String(a.end) <= rows[0].ts) return { status: "before_archive" };
        const target = rows.find((m) => m.ts >= String(a.start) && m.ts < String(a.end));
        return target ? { status: "found", ts: target.ts } : { status: "empty" };
      }
      case "set_reaction": {
        const m = [...messages, ...bigMessages, ...replies, ...elsewhere].find((m) => m.ts === a.ts);
        if (!m) throw "message_not_found";
        const name = String(a.name);
        const counts = new Map(
          [...String(m.reactions ?? "").matchAll(/:([^:\s]+(?:::skin-tone-\d)?): (\d+)/g)].map(
            (r) => [r[1], Number(r[2])],
          ),
        );
        const own = new Set(m.own_reactions ?? []);
        const delta = a.on ? (own.has(name) ? 0 : 1) : own.has(name) ? -1 : 0;
        if (a.on) own.add(name);
        else own.delete(name);
        counts.set(name, (counts.get(name) ?? 0) + delta);
        m.own_reactions = [...own];
        m.reactions =
          [...counts]
            .filter(([, n]) => n > 0)
            .map(([name, n]) => `:${name}: ${n}`)
            .join("  ") || null;
        emit("flek://change", {
          type: "reactions",
          data: { conversation_id: String(a.conversationId), ts: m.ts },
        });
        return null;
      }
      case "send_message":
        return sendMessage(a);
      case "search": {
        const q = String(a.query ?? "");
        if (q.includes("before:2024-")) throw 'bad date "2024-", expected YYYY-MM-DD';
        if (q.includes("nothing")) return [];
        const list = a.recent ? [...hits].sort((x, y) => y.ts.localeCompare(x.ts)) : hits;
        return list.slice(0, Number(a.limit ?? 100));
      }
      default:
        return null;
    }
  },
  { shouldMockEvents: true },
);

function timelineRows(id: string) {
  return (id === "C1" ? messages : id === "CBIG" ? bigMessages : elsewhere.filter((m) => m.conversation_id === id))
    .filter((m) => !m.thread_ts || m.thread_ts === m.ts || m.subtype === "thread_broadcast")
    .slice().sort((a, b) => a.ts.localeCompare(b.ts));
}

// Simulates a live reply from the sync engine: `__reply()` in the console.
(window as unknown as { __reply: () => void }).__reply = () => {
  const ts = Math.floor(Date.now() / 1000);
  replies.push(msg(ts, "U2", "Sure, here they are.", { thread_ts: ROOT }));
  const root = messages.find((m) => m.ts === ROOT)!;
  root.reply_count = replies.length;
  root.repliers = ["U2", "U4", "U1", "U5"];
  root.latest_reply = `${ts}.000000`;
  emit("flek://change", { type: "message", data: { conversation_id: "C1", ts: `${ts}.000000` } });
};


// Synthetic connection status: __syncIncomplete(true|false) in the console.
(window as unknown as { __syncIncomplete: (incomplete: boolean) => void }).__syncIncomplete = (incomplete) => {
  emit("flek://change", { type: "connection", data: incomplete
    ? { state: "sync_incomplete", in_secs: 60 }
    : { state: "connected" } });
};
