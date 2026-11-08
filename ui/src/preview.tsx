// UI preview without Tauri or Slack: `npm run dev --workspace ui`, then open
// http://localhost:5173/preview.html. IPC is mocked with made-up data.
import { mockIPC } from "@tauri-apps/api/mocks";
import { render } from "solid-js/web";
import App from "./App";
import type { Conversation, FileRef, Message } from "./api";
import "./styles.css";

const people: Record<string, string> = { U1: "Olena Koval", U2: "Taras Melnyk", U3: "Me" };
const conversations: Conversation[] = [
  { id: "C1", kind: "public_channel", label: "general", is_archived: false, topic: "Announcements", messages: 6, unread: 2, latest_ts: "1762600000.000000" },
  { id: "C2", kind: "private_channel", label: "course-project", is_archived: false, topic: null, messages: 0, unread: 0, latest_ts: null },
  { id: "D1", kind: "im", label: "Olena Koval", is_archived: false, topic: null, messages: 1, unread: 1, latest_ts: "1762600500.000000" },
];
const file = (f: Partial<FileRef>): FileRef => ({
  id: "F", name: null, title: null, mimetype: null, filetype: null, size: null, local_path: null,
  permalink: "https://example.com/f", hidden_by_limit: false, width: null, height: null, ...f,
});
const msg = (ts: number, user: string, text: string, extra: Partial<Message> = {}): Message => ({
  ts: `${ts}.000000`, user_id: user, author: people[user], text, subtype: null, reply_count: 0,
  edited: false, hidden_by_limit: false, reactions: null, thread_ts: null, files: [], ...extra,
});
const messages: Message[] = [
  msg(1762300000, "U1", "# Practice moved online\nHi all :wave: tomorrow's practice is *online*: <https://us06web.zoom.us/j/123456|Join Zoom>\nSlides: <https://docs.google.com/presentation/d/abc/edit>", { reactions: ":+1: 4  :fire: 2" }),
  msg(1762300060, "U1", "Plan:\n- predicates recap\n- test, _half_ the class\n  - variants at 3 times\n1. read chapter 2\n2. try `forall x. P(x)`"),
  msg(1762300120, "U2", "&gt; Will the recording be on the drive?\nYes, <@U1> said so <!here>. See https://github.com/example/repo and :pink_heart:", { edited: true, reply_count: 3 }),
  msg(1762400000, "U3", "```fn main() {\n    println!(\"hi\");\n}```"),
  msg(1762400030, "U3", "", { files: [file({ id: "F1", name: "calendar.png", mimetype: "image/png", local_path: "/tmp/x.png", width: 1200, height: 800 }), file({ id: "F2", name: "lecture-5.pdf", mimetype: "application/pdf", filetype: "pdf", size: 2_400_000, local_path: "/tmp/l.pdf" })] }),
  msg(1762500000, "U2", "Old one ~gone~ but kept :custom_party:", { hidden_by_limit: true, files: [file({ id: "F3", name: "notes.docx", filetype: "docx", size: 48_000, hidden_by_limit: true })] }),
];

mockIPC((cmd, args) => {
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
    case "messages":
      return a.conversationId === "C1" && !a.before ? messages : [];
    case "plugin:event|listen":
      return 1;
    default:
      return null;
  }
});

// Cached images: a placeholder instead of asset:// (no Tauri here).
const placeholder =
  "data:image/svg+xml," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="800"><rect width="100%" height="100%" fill="#8fb3e8"/><text x="50%" y="50%" font-size="80" text-anchor="middle" fill="#fff" font-family="sans-serif">image</text></svg>',
  );
(window as unknown as { __TAURI_INTERNALS__: Record<string, unknown> }).__TAURI_INTERNALS__.convertFileSrc =
  () => placeholder;

render(() => <App />, document.getElementById("root")!);
