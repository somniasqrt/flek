// UI preview without Tauri or Slack: `npm run dev --workspace ui`, then open
// http://localhost:5173/preview.html. IPC is mocked with made-up data.
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import { render } from "solid-js/web";
import App from "./App";
import { overrideCacheUrl, type Conversation, type FileRef, type Hit, type Message, type Profile } from "./api";
import { MARK_CLOSE as C, MARK_OPEN as O } from "./snippet";
import "./styles.css";

const people: Record<string, string> = {
  U1: "Olena Koval", U2: "Taras Melnyk", U3: "Me", U4: "Iryna Bondar", U5: "Andrii Shevchuk",
};
const conversations: Conversation[] = [
  { id: "C1", kind: "public_channel", label: "general", is_archived: false, topic: "Announcements", messages: 6, unread: 2, latest_ts: "1762600000.000000", dm_user_id: null },
  { id: "C2", kind: "private_channel", label: "course-project", is_archived: false, topic: null, messages: 0, unread: 0, latest_ts: null, dm_user_id: null },
  { id: "D1", kind: "im", label: "Olena Koval", is_archived: false, topic: null, messages: 1, unread: 1, latest_ts: "1762600500.000000", dm_user_id: "U1" },
];
const file = (f: Partial<FileRef>): FileRef => ({
  id: "F", name: null, title: null, mimetype: null, filetype: null, size: null, local_path: null,
  permalink: "https://example.com/f", hidden_by_limit: false, width: null, height: null,
  preview_path: null, preview_width: null, preview_height: null, ...f,
});
const msg = (ts: number, user: string, text: string, extra: Partial<Message> = {}): Message => ({
  ts: `${ts}.000000`, user_id: user, author: people[user], text, subtype: null, reply_count: 0,
  repliers: [], latest_reply: null, edited: false, hidden_by_limit: false, reactions: null, thread_ts: null, files: [], unfurls: [], ...extra,
});
// The thread under 1762300120: its last reply a few minutes ago.
const now = Math.floor(Date.now() / 1000);
const ROOT = "1762300120.000000";
const replies: Message[] = [
  msg(1762300400, "U5", "Will it be the same link as last week?", { thread_ts: ROOT }),
  msg(1762300700, "U1", "Yes, same Zoom room. I'll also post the recording in #general afterwards :+1:", { thread_ts: ROOT, reactions: ":pray: 2" }),
  msg(now - 25 * 60, "U4", "Thanks! Could someone share the slides from the previous lecture too?", { thread_ts: ROOT }),
];
const messages: Message[] = [
  msg(1762300000, "U1", "# Practice moved online\nHi all :wave: tomorrow's practice is *online*: <https://us06web.zoom.us/j/123456|Join Zoom>\nSlides: <https://docs.google.com/presentation/d/abc/edit>", { reactions: ":+1: 4  :fire: 2" }),
  msg(1762300060, "U1", "Plan:\n- predicates recap\n- test, _half_ the class\n  - variants at 3 times\n1. read chapter 2\n2. try `forall x. P(x)`"),
  msg(1762300120, "U2", "&gt; Will the recording be on the drive?\nYes, <@U1> said so <!here>. See https://github.com/example/repo and :pink_heart:", { edited: true, reply_count: 3, repliers: ["U4", "U1", "U5"], latest_reply: `${now - 25 * 60}.000000` }),
  msg(1762400000, "U3", "```fn main() {\n    println!(\"hi\");\n}```"),
  msg(1762400030, "U3", "", { files: [file({ id: "F1", name: "calendar.png", mimetype: "image/png", local_path: "/tmp/x.png", width: 1200, height: 800 }), file({ id: "F2", name: "Schedule_Monday stream 70236.pdf", mimetype: "application/pdf", filetype: "pdf", size: 2_400_000, local_path: "/tmp/l.pdf", preview_path: "/tmp/p.png", preview_width: 910, preview_height: 1286 })] }),
  msg(1762400090, "U1", "Variants for the test", { files: [file({ id: "F4", name: "Variants 9 points.pdf", mimetype: "application/pdf", filetype: "pdf", size: 300_000, local_path: "/tmp/v.pdf", preview_path: "/tmp/v.png", preview_width: 910, preview_height: 1286 })] }),
  msg(1762400120, "U1", "", { files: [file({ id: "F5", name: "KM-53 11-30.pdf", mimetype: "application/pdf", filetype: "pdf", local_path: "/tmp/a.pdf", preview_path: "/tmp/a.png" }), file({ id: "F6", name: "KM-51 11-30.pdf", mimetype: "application/pdf", filetype: "pdf", local_path: "/tmp/b.pdf", preview_path: "/tmp/b.png" })] }),
  msg(1762450000, "U1", "Lecture recording: https://www.youtube.com/watch?v=abc and the plan https://example.org/plan", { unfurls: [
    { service: "YouTube", title: "Lecture 5: predicates", title_link: "https://www.youtube.com/watch?v=abc", text: null, author: null, footer: null, url: "https://www.youtube.com/watch?v=abc", color: null, image: "yt", image_width: 480, image_height: 360, thumb: false, icon: "ic", video: true, fields: [] },
    { service: "Example Org", title: "Course plan 2026", title_link: "https://example.org/plan", text: "Topics, deadlines and *grading* for the autumn term.", author: null, footer: null, url: "https://example.org/plan", color: "#36a64f", image: "th", image_width: 80, image_height: 80, thumb: true, icon: null, video: false, fields: [] },
  ] }),
  msg(1762450100, "U5", "Shipping it :shipit: :yes::skin-tone-4: and a :custom_party: with :not_cached:", { reactions: ":custom_party: 3  :+1::skin-tone-3: 1  :gdocs: 2  :zoom: 1" }),
  msg(1762500000, "U2", "Old one ~gone~ but kept :custom_party:", { hidden_by_limit: true, files: [file({ id: "F3", name: "notes.docx", filetype: "docx", size: 48_000, hidden_by_limit: true })] }),
];

// Search: a few fake hits, matched terms marked like core does.
const hit = (conversation_id: string, conversation: string, ts: number, user: string, snippet: string, extra: Partial<Hit> = {}): Hit => ({
  conversation_id, conversation, ts: `${ts}.000000`, thread_ts: null, user_id: user, author: people[user], snippet, hidden_by_limit: false, ...extra,
});
const hits: Hit[] = [
  hit("C1", "general", 1762300000, "U1", `Hi all :wave: tomorrow's ${O}practice${C} is *online*: <https://us06web.zoom.us/j/123456|Join Zoom>`),
  hit("C1", "general", 1762300060, "U1", `Plan: - predicates recap - test, _half_ the class … ${O}practice${C} variants at 3 times`),
  hit("D1", "Olena Koval", 1762600500, "U1", `Чи буде запис ${O}практики${C} на диску? <@U2> казав, що так`),
  hit("C2", "course-project", 1755000000, "U2", `Старий звіт з ${O}практики${C}, Slack його вже ховає`, { hidden_by_limit: true }),
  hit("C1", "general", 1762300200, "U2", `Reply in a thread about the ${O}practice${C} room`, { thread_ts: "1762300120.000000" }),
];

// Profiles: made-up people (fake email), one with a DM and one without.
const profile = (id: string, extra: Partial<Profile>): Profile => ({
  id, name: people[id].toLowerCase().replace(" ", "."), real_name: people[id], display_name: null,
  title: null, email: null, phone: null, pronouns: null, status_text: null, status_emoji: null,
  tz: null, tz_label: null, tz_offset: null, is_bot: false, deleted: false, avatar: null,
  photo: null, has_photo: false, dm_conversation_id: null, ...extra,
});
const profiles: Record<string, Profile> = {
  U1: profile("U1", {
    title: "Доцент кафедри математичної логіки", email: "olena.koval@example.edu",
    phone: "+380 00 000 0000", tz: "Europe/Kyiv", tz_label: "Eastern European Time",
    tz_offset: 7200, avatar: "a1", has_photo: true, dm_conversation_id: "D1",
    status_text: "On a lecture", status_emoji: ":books:",
  }),
  U2: profile("U2", { title: "професор", tz: "America/New_York", tz_label: "Eastern Standard Time", tz_offset: -18000 }),
  U4: profile("U4", { display_name: "iryna", pronouns: "she/her", tz_offset: 3600, avatar: "a4" }),
};

// Messages "sent" from the preview into conversations other than C1.
const elsewhere: (Message & { conversation_id: string })[] = [];
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
    thread_ts: threadTs,
    subtype: threadTs && a.broadcast ? "thread_broadcast" : null,
  });
  if (conv !== "C1") elsewhere.push({ ...m, conversation_id: conv });
  else if (threadTs === ROOT) {
    replies.push(m);
    if (a.broadcast) messages.push(m);
    const root = messages.find((x) => x.ts === ROOT)!;
    root.reply_count = replies.length;
    root.latest_reply = ts;
  } else messages.push(m);
  emit("flek://change", { type: "message", data: { conversation_id: conv, ts } });
  return { conversation_id: conv, ts, client_msg_id: m.client_msg_id };
}

mockIPC(async (cmd, args) => {
  const a = args as Record<string, string | null>;
  switch (cmd) {
    case "status":
      return { phase: "ready", team_id: "T1", team: "Demo Team", user_id: "U3", user: "me" };
    case "progress":
      return { stage: "conversations", done: 7, total: 28, current: "general", deep: true };
    case "conversations":
      return conversations;
    case "people":
      return people;
    case "avatars":
      return { U1: "a1", U4: "a4" };
    case "custom_emoji":
      return { images: { custom_party: "e1", shipit: "e2", gdocs: "e2" }, aliases: { yes: "thumbsup" } };
    case "profile":
      return structuredClone(profiles[String(a.userId)] ?? null);
    case "profile_photo":
      // The large photo takes a moment, like a real download.
      await new Promise((r) => setTimeout(r, 600));
      return profiles[String(a.userId)]?.has_photo ? "big1" : null;
    case "pdf_data":
      return Uint8Array.from(pdf, (c) => c.charCodeAt(0)).buffer;
    // Copies, like real IPC: stores wrap what they get, and __reply below
    // must not mutate it behind their back.
    case "thread":
      return structuredClone(a.ts === ROOT ? [messages.find((m) => m.ts === ROOT), ...replies] : []);
    case "message":
      return structuredClone([...messages, ...replies, ...elsewhere].find((m) => m.ts === a.ts) ?? null);
    case "messages":
      if (a.before) return [];
      return structuredClone(
        a.conversationId === "C1" ? messages : elsewhere.filter((m) => m.conversation_id === a.conversationId),
      );
    case "send_message":
      return sendMessage(a);
    case "search": {
      const q = String(a.query ?? "");
      if (q.includes("before:2024-")) throw "bad date \"2024-\", expected YYYY-MM-DD";
      if (q.includes("nothing")) return [];
      const list = a.recent ? [...hits].sort((x, y) => y.ts.localeCompare(x.ts)) : hits;
      return list.slice(0, Number(a.limit ?? 100));
    }
    default:
      return null;
  }
}, { shouldMockEvents: true });

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

// Cached images: a placeholder instead of flekfile:// (no app here).
const avatar =
  "data:image/svg+xml," +
  encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="72" height="72"><rect width="72" height="72" fill="#e8a33d"/><circle cx="36" cy="28" r="14" fill="#fff"/><rect x="14" y="48" width="44" height="30" rx="15" fill="#fff"/></svg>');
const placeholder =
  "data:image/svg+xml," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="800"><rect width="100%" height="100%" fill="#8fb3e8"/><text x="50%" y="50%" font-size="80" text-anchor="middle" fill="#fff" font-family="sans-serif">image</text></svg>',
  );
// A five-page PDF for the viewer (pdf.js rebuilds the missing xref table).
const pdfPage = (n: number) =>
  `${n} 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]/Resources<</Font<</F1 9 0 R>>>>/Contents ${n + 10} 0 R>>endobj\n` +
  `${n + 10} 0 obj<</Length 60>>stream\nBT /F1 36 Tf 72 740 Td (Sample page ${n - 2}) Tj ET\nendstream endobj\n`;
const pdfPages = [3, 4, 5, 6, 7];
const pdf =
  "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n" +
  `2 0 obj<</Type/Pages/Kids[${pdfPages.map((n) => `${n} 0 R`).join(" ")}]/Count ${pdfPages.length}>>endobj\n` +
  pdfPages.map(pdfPage).join("") +
  "9 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF";
const avatar2 = avatar.replace("%23e8a33d", "%235b8def");
const bigPhoto =
  "data:image/svg+xml," +
  encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="512" height="512"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#f2c14e"/><stop offset="1" stop-color="#d9822b"/></linearGradient></defs><rect width="512" height="512" fill="url(#g)"/><circle cx="256" cy="200" r="96" fill="#fff" opacity="0.92"/><rect x="96" y="330" width="320" height="260" rx="140" fill="#fff" opacity="0.92"/></svg>');
// Custom emoji: a small party hat and a square badge.
const emojiParty =
  "data:image/svg+xml," +
  encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><path d="M32 4 54 58H10Z" fill="#d94fa0"/><circle cx="32" cy="6" r="6" fill="#f5c400"/><path d="M20 34h24M15 46h34" stroke="#fff" stroke-width="5"/></svg>');
const emojiBadge =
  "data:image/svg+xml," +
  encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="64" height="64"><rect x="4" y="4" width="56" height="56" rx="12" fill="#2f8f5b"/><path d="M18 33l10 10 18-22" stroke="#fff" stroke-width="7" fill="none"/></svg>');
const cached: Record<string, string> = {
  "link/a1": avatar,
  "link/a4": avatar2,
  "link/e1": emojiParty,
  "link/e2": emojiBadge,
  "link/big1": bigPhoto,
};
overrideCacheUrl((path) => cached[path] ?? placeholder);

render(() => <App />, document.getElementById("root")!);
