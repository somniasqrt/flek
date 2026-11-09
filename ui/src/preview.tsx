// UI preview without Tauri or Slack: `npm run dev --workspace ui`, then open
// http://localhost:5173/preview.html. IPC is mocked with made-up data.
import { mockIPC } from "@tauri-apps/api/mocks";
import { render } from "solid-js/web";
import App from "./App";
import { overrideCacheUrl, type Conversation, type FileRef, type Message } from "./api";
import "./styles.css";

const people: Record<string, string> = { U1: "Olena Koval", U2: "Taras Melnyk", U3: "Me" };
const conversations: Conversation[] = [
  { id: "C1", kind: "public_channel", label: "general", is_archived: false, topic: "Announcements", messages: 6, unread: 2, latest_ts: "1762600000.000000" },
  { id: "C2", kind: "private_channel", label: "course-project", is_archived: false, topic: null, messages: 0, unread: 0, latest_ts: null },
  { id: "D1", kind: "im", label: "Olena Koval", is_archived: false, topic: null, messages: 1, unread: 1, latest_ts: "1762600500.000000" },
];
const file = (f: Partial<FileRef>): FileRef => ({
  id: "F", name: null, title: null, mimetype: null, filetype: null, size: null, local_path: null,
  permalink: "https://example.com/f", hidden_by_limit: false, width: null, height: null,
  preview_path: null, preview_width: null, preview_height: null, ...f,
});
const msg = (ts: number, user: string, text: string, extra: Partial<Message> = {}): Message => ({
  ts: `${ts}.000000`, user_id: user, author: people[user], text, subtype: null, reply_count: 0,
  edited: false, hidden_by_limit: false, reactions: null, thread_ts: null, files: [], unfurls: [], ...extra,
});
const messages: Message[] = [
  msg(1762300000, "U1", "# Practice moved online\nHi all :wave: tomorrow's practice is *online*: <https://us06web.zoom.us/j/123456|Join Zoom>\nSlides: <https://docs.google.com/presentation/d/abc/edit>", { reactions: ":+1: 4  :fire: 2" }),
  msg(1762300060, "U1", "Plan:\n- predicates recap\n- test, _half_ the class\n  - variants at 3 times\n1. read chapter 2\n2. try `forall x. P(x)`"),
  msg(1762300120, "U2", "&gt; Will the recording be on the drive?\nYes, <@U1> said so <!here>. See https://github.com/example/repo and :pink_heart:", { edited: true, reply_count: 3 }),
  msg(1762400000, "U3", "```fn main() {\n    println!(\"hi\");\n}```"),
  msg(1762400030, "U3", "", { files: [file({ id: "F1", name: "calendar.png", mimetype: "image/png", local_path: "/tmp/x.png", width: 1200, height: 800 }), file({ id: "F2", name: "Schedule_Monday stream 70236.pdf", mimetype: "application/pdf", filetype: "pdf", size: 2_400_000, local_path: "/tmp/l.pdf", preview_path: "/tmp/p.png", preview_width: 910, preview_height: 1286 })] }),
  msg(1762450000, "U1", "Lecture recording: https://www.youtube.com/watch?v=abc and the plan https://example.org/plan", { unfurls: [
    { service: "YouTube", title: "Lecture 5: predicates", title_link: "https://www.youtube.com/watch?v=abc", text: null, author: null, footer: null, url: "https://www.youtube.com/watch?v=abc", color: null, image: "yt", image_width: 480, image_height: 360, thumb: false, icon: "ic", video: true, fields: [] },
    { service: "Example Org", title: "Course plan 2026", title_link: "https://example.org/plan", text: "Topics, deadlines and *grading* for the autumn term.", author: null, footer: null, url: "https://example.org/plan", color: "#36a64f", image: "th", image_width: 80, image_height: 80, thumb: true, icon: null, video: false, fields: [] },
  ] }),
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
    case "avatars":
      return { U1: "a1" };
    case "messages":
      return a.conversationId === "C1" && !a.before ? messages : [];
    case "plugin:event|listen":
      return 1;
    default:
      return null;
  }
});

// Cached images: a placeholder instead of flekfile:// (no app here).
const avatar =
  "data:image/svg+xml," +
  encodeURIComponent('<svg xmlns="http://www.w3.org/2000/svg" width="72" height="72"><rect width="72" height="72" fill="#e8a33d"/><circle cx="36" cy="28" r="14" fill="#fff"/><rect x="14" y="48" width="44" height="30" rx="15" fill="#fff"/></svg>');
const placeholder =
  "data:image/svg+xml," +
  encodeURIComponent(
    '<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="800"><rect width="100%" height="100%" fill="#8fb3e8"/><text x="50%" y="50%" font-size="80" text-anchor="middle" fill="#fff" font-family="sans-serif">image</text></svg>',
  );
// A two-page PDF for the viewer (pdf.js rebuilds the missing xref table).
const pdfPage = (n: number) =>
  `${n} 0 obj<</Type/Page/Parent 2 0 R/MediaBox[0 0 595 842]/Resources<</Font<</F1 9 0 R>>>>/Contents ${n + 2} 0 R>>endobj\n` +
  `${n + 2} 0 obj<</Length 60>>stream\nBT /F1 36 Tf 72 740 Td (Sample page ${n - 2}) Tj ET\nendstream endobj\n`;
const pdf =
  "%PDF-1.4\n1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n2 0 obj<</Type/Pages/Kids[3 0 R 4 0 R]/Count 2>>endobj\n" +
  pdfPage(3) + pdfPage(4) + "9 0 obj<</Type/Font/Subtype/Type1/BaseFont/Helvetica>>endobj\ntrailer<</Root 1 0 R>>\n%%EOF";
overrideCacheUrl((path) =>
  path === "F2" ? "data:application/pdf;base64," + btoa(pdf) : path === "link/a1" ? avatar : placeholder,
);

render(() => <App />, document.getElementById("root")!);
