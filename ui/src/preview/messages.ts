import type { Conversation, FileRef, Message } from "../api";

export const people: Record<string, string> = {
  U1: "Olena Koval",
  U2: "Taras Melnyk",
  U3: "Me",
  U4: "Iryna Bondar",
  U5: "Andrii Shevchuk",
};
export const conversations: Conversation[] = [
  {
    id: "C1",
    kind: "public_channel",
    label: "general",
    is_archived: false,
    topic: "Announcements",
    messages: 6,
    unread: 2,
    latest_ts: "1762600000.000000",
    dm_user_id: null,
  },
  {
    id: "C2",
    kind: "private_channel",
    label: "course-project",
    is_archived: false,
    topic: null,
    messages: 0,
    unread: 0,
    latest_ts: null,
    dm_user_id: null,
  },
  {
    id: "D1",
    kind: "im",
    label: "Olena Koval",
    is_archived: false,
    topic: null,
    messages: 1,
    unread: 1,
    latest_ts: "1762600500.000000",
    dm_user_id: "U1",
  },
];
conversations.push(
  { ...conversations[2], id: "D2", label: people.U2, dm_user_id: "U2", unread: 0 },
  { ...conversations[2], id: "D4", label: people.U4, dm_user_id: "U4", unread: 0 },
);
conversations.push(
  { ...conversations[2], id: "D3", label: "Me", dm_user_id: "U3", unread: 0 },
  {
    ...conversations[2],
    id: "G1",
    kind: "mpim",
    label: "Olena, Taras",
    dm_user_id: null,
    unread: 2,
  },
);
export const file = (f: Partial<FileRef>): FileRef => ({
  id: "F",
  name: null,
  title: null,
  mimetype: null,
  filetype: null,
  size: null,
  can_open: false,
  local_path: null,
  permalink: "https://example.com/f",
  hidden_by_limit: false,
  width: null,
  height: null,
  preview_path: null,
  preview_width: null,
  preview_height: null,
  ...f,
});
export const msg = (ts: number, user: string, text: string, extra: Partial<Message> = {}): Message => ({
  ts: `${ts}.000000`,
  user_id: user,
  author: people[user],
  text,
  subtype: null,
  reply_count: 0,
  repliers: [],
  latest_reply: null,
  edited: false,
  hidden_by_limit: false,
  reactions: null,
  thread_ts: null,
  files: [],
  unfurls: [],
  ...extra,
});
// The thread under 1762300120: its last reply a few minutes ago.
export const now = Math.floor(Date.now() / 1000);
export const ROOT = "1762300120.000000";
export const replies: Message[] = [
  msg(1762300400, "U5", "Will it be the same link as last week?", { thread_ts: ROOT }),
  msg(
    1762300700,
    "U1",
    "Yes, same Zoom room. I'll also post the recording in #general afterwards :+1:",
    { thread_ts: ROOT, reactions: ":pray: 2" },
  ),
  msg(
    now - 25 * 60,
    "U4",
    "Thanks! Could someone share the slides from the previous lecture too?",
    { thread_ts: ROOT },
  ),
];
export const messages: Message[] = [
  msg(
    1762300000,
    "U1",
    "# Practice moved online\nHi all :wave: tomorrow's practice is *online*: <https://us06web.zoom.us/j/123456|Join Zoom>\nSlides: <https://docs.google.com/presentation/d/abc/edit>",
    { reactions: ":+1: 4  :fire: 2" },
  ),
  msg(
    1762300060,
    "U1",
    "Plan:\n- predicates recap\n- test, _half_ the class\n  - variants at 3 times\n1. read chapter 2\n2. try `forall x. P(x)`",
  ),
  msg(
    1762300120,
    "U2",
    "&gt; Will the recording be on the drive?\nYes, <@U1> said so <!here>. See https://github.com/example/repo and :pink_heart:",
    {
      edited: true,
      reply_count: 3,
      repliers: ["U4", "U1", "U5"],
      latest_reply: `${now - 25 * 60}.000000`,
    },
  ),
  msg(1762400000, "U3", '```fn main() {\n    println!("hi");\n}```'),
  msg(1762400030, "U3", "", {
    files: [
      file({
        id: "F1",
        name: "calendar.png",
        mimetype: "image/png",
        can_open: true,
        local_path: "/tmp/x.png",
        width: 1200,
        height: 800,
      }),
      file({
        id: "F2",
        name: "Schedule_Monday stream 70236.pdf",
        mimetype: "application/pdf",
        filetype: "pdf",
        size: 2_400_000,
        can_open: true,
        local_path: "/tmp/l.pdf",
        preview_path: "/tmp/p.png",
        preview_width: 910,
        preview_height: 1286,
      }),
    ],
  }),
  msg(1762400090, "U1", "Variants for the test", {
    files: [
      file({
        id: "F4",
        name: "Variants 9 points.pdf",
        mimetype: "application/pdf",
        filetype: "pdf",
        size: 300_000,
        can_open: true,
        local_path: "/tmp/v.pdf",
        preview_path: "/tmp/v.png",
        preview_width: 910,
        preview_height: 1286,
      }),
    ],
  }),
  msg(1762400120, "U1", "", {
    files: [
      file({
        id: "F5",
        name: "KM-53 11-30.pdf",
        mimetype: "application/pdf",
        filetype: "pdf",
        can_open: true,
        local_path: "/tmp/a.pdf",
        preview_path: "/tmp/a.png",
      }),
      file({
        id: "F6",
        name: "KM-51 11-30.pdf",
        mimetype: "application/pdf",
        filetype: "pdf",
        can_open: true,
        local_path: "/tmp/b.pdf",
        preview_path: "/tmp/b.png",
      }),
    ],
  }),
  msg(
    1762450000,
    "U1",
    "Lecture recording: https://www.youtube.com/watch?v=abc and the plan https://example.org/plan",
    {
      unfurls: [
        {
          service: "YouTube",
          title: "Lecture 5: predicates",
          title_link: "https://www.youtube.com/watch?v=abc",
          text: null,
          author: null,
          footer: null,
          url: "https://www.youtube.com/watch?v=abc",
          color: null,
          image: "yt",
          image_width: 480,
          image_height: 360,
          thumb: false,
          icon: "ic",
          video: true,
          fields: [],
        },
        {
          service: "Example Org",
          title: "Course plan 2026",
          title_link: "https://example.org/plan",
          text: "Topics, deadlines and *grading* for the autumn term.",
          author: null,
          footer: null,
          url: "https://example.org/plan",
          color: "#36a64f",
          image: "th",
          image_width: 80,
          image_height: 80,
          thumb: true,
          icon: null,
          video: false,
          fields: [],
        },
      ],
    },
  ),
  msg(1762450050, "U1", "Facebook preview regression fixture", {
    unfurls: [
      {
        service: "Facebook",
        title: "&#127757; ML WEEK 2025 &ndash; &#1050;&#1055;&#1030;",
        title_link: "https://example.com/facebook-preview",
        text: "&#127757; ML WEEK 2025 &ndash; &#1091;&#1085;&#1110;&#1082;&#1072;&#1083;&#1100;&#1085;&#1080;&#1081; &#1087;&#1088;&#1086;&#1108;&#1082;&#1090;\n\n&laquo;&#1050;&#1055;&#1030;&raquo; &amp; *&#1053;&#1058;&#1059;* — &#x1F4BB;",
        author: null,
        footer: "&#169; Facebook &amp; example",
        url: "https://example.com/facebook-preview",
        color: null,
        image: null,
        image_width: null,
        image_height: null,
        thumb: false,
        icon: null,
        video: false,
        fields: [],
      },
    ],
  }),
  msg(
    1762450100,
    "U5",
    "Shipping it :shipit: :yes::skin-tone-4: and a :custom_party: with :not_cached:",
    { reactions: ":custom_party: 3  :+1::skin-tone-3: 1  :gdocs: 2  :zoom: 1" },
  ),
  msg(1762500000, "U2", "Old one ~gone~ but kept :custom_party:", {
    hidden_by_limit: true,
    files: [
      file({ id: "F3", name: "notes.docx", filetype: "docx", size: 48_000, hidden_by_limit: true }),
    ],
  }),
];

// Synthetic security fixture: no executable contents or real cache path.
messages.push(msg(1762500050, "U2", "File safety preview", {
  files: [file({ id: "FSECURITY", name: "x.terminal", title: "Розклад.pdf", mimetype: "application/pdf", local_path: "/synthetic/x.terminal", can_open: false })],
}));

// Real-shaped received blocks paired with their mrkdwn fallback for regression QA.
const receivedText =
  "<https://us06web.zoom.us/j/123456|Join Zoom> https://example.org/plan <https://example.org/label|Read this> :wave::skin-tone-4: <@U1> *bold underline*";
messages.push(
  msg(1762500100, "U2", receivedText, {
    blocks: [
      {
        type: "rich_text",
        block_id: "received-fixture",
        elements: [
          {
            type: "rich_text_section",
            elements: [
              { type: "link", url: "https://us06web.zoom.us/j/123456", text: "Join Zoom" },
              { type: "text", text: " " },
              { type: "link", url: "https://example.org/plan", text: "https://example.org/plan" },
              { type: "text", text: " " },
              { type: "link", url: "https://example.org/label", text: "Read this" },
              { type: "text", text: " " },
              { type: "emoji", name: "wave", skin_tone: 4 },
              { type: "text", text: " " },
              { type: "user", user_id: "U1" },
              { type: "text", text: " " },
              { type: "text", text: "bold underline", style: { bold: true, underline: true } },
            ],
          },
        ],
      },
    ],
  }),
  msg(1762500110, "U1", receivedText),
  msg(1762500120, "U2", "Unsupported element fallback <@U1>", {
    blocks: [
      {
        type: "rich_text",
        elements: [
          { type: "rich_text_section", elements: [{ type: "channel", channel_id: "C1" }] },
        ],
      },
    ],
  }),
);


// M3 reading fixtures: no remote URLs are loaded by the preview.
export const kitImage = "https://files.slack-edge.com/synthetic/block.png";
messages.push(
  msg(now - 100, "U1", "Block Kit plain fallback", {
    block_images: { [kitImage]: "block-image" },
    blocks: [
      { type: "header", text: { type: "plain_text", text: "Release preview" } },
      { type: "section", text: { type: "mrkdwn", text: "*Ready* for <@U2> :wave:" },
        fields: [{ type: "mrkdwn", text: "*Version*\n3.0" }, { type: "plain_text", text: "Local only" }],
        accessory: { type: "image", image_url: kitImage, alt_text: "Synthetic accessory" } },
      { type: "context", elements: [{ type: "plain_text", text: "Preview context" }, { type: "image", image_url: kitImage, alt_text: "Icon" }] },
      { type: "divider" },
      { type: "image", image_url: kitImage, alt_text: "Synthetic image", title: { type: "plain_text", text: "Cached image" } },
      { type: "actions", elements: [{ type: "button", text: { type: "plain_text", text: "Read-only action" }, action_id: "never-invoked" }] },
    ],
  }),
  msg(now - 90, "U2", "Cached video", { files: [file({ id: "FVIDEO", name: "preview.mp4", mimetype: "video/mp4", filetype: "mp4", local_path: "/synthetic/preview.mp4", preview_path: "/synthetic/poster.png", can_open: true })] }),
  msg(now - 80, "U2", "Video download", { files: [file({ id: "FVIDEO_DOWNLOAD", name: "download.mp4", mimetype: "video/mp4", filetype: "mp4", can_open: true })] }),
  msg(now - 70, "U1", "Huddle (native client only)", { subtype: "huddle_thread" }),
  msg(now - 60, "U1", "Unsupported attachment fallback", { unsupported: true }),
  msg(now - 50, "U1", "Canvas fallback", { files: [file({ id: "FCANVAS", name: "Plan.canvas", filetype: "canvas", mimetype: "application/vnd.slack-docs" })] }),
);
const officeFiles = [
  file({ id: "FXLSX", name: "budget.xlsx", filetype: "xlsx", mimetype: "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", local_path: "/synthetic/budget.xlsx", can_open: true }),
  file({ id: "FDOCX", name: "reading.docx", filetype: "docx", mimetype: "application/vnd.openxmlformats-officedocument.wordprocessingml.document", local_path: "/synthetic/reading.docx", can_open: true }),
];
messages.push(
  msg(now - 45, "U2", "Office viewer: sheets, numbers, dates and cached formula values", { files: [officeFiles[0]] }),
  msg(now - 40, "U2", "Word viewer: headings, formatting, list, table and embedded image", { files: [officeFiles[1]] }),
  msg(now - 35, "U1", "Truncated spreadsheet (row 1001 / column CW)", { files: [file({ ...officeFiles[0], id: "FXLSX_LARGE", name: "large.xlsx" })] }),
  msg(now - 30, "U1", "Broken Word document (friendly error)", { files: [file({ ...officeFiles[1], id: "FDOCX_BROKEN", name: "broken.docx" })] }),
  msg(now - 25, "U1", "Document over 50 MiB (friendly fallback)", { files: [file({ ...officeFiles[0], id: "FXLSX_LIMIT", name: "over-limit.xlsx", size: 60 * 1024 * 1024 })] }),
  msg(now - 20, "U1", "Undownloaded and legacy files keep their existing action", { files: [file({ ...officeFiles[1], id: "FDOCX_REMOTE", name: "remote.docx", local_path: null }), file({ id: "FXLS", name: "legacy.xls", filetype: "xls", mimetype: "application/vnd.ms-excel", local_path: "/synthetic/legacy.xls", can_open: true })] }),
);
replies.push(msg(now - 15, "U2", "Office files in a thread", { thread_ts: ROOT, files: officeFiles }));
const officeRoot = messages.find((m) => m.ts === ROOT)!;
officeRoot.reply_count = replies.length;
officeRoot.latest_reply = replies.at(-1)!.ts;
export const bigMessages = Array.from({ length: 10_000 }, (_, i) => msg(now - (10_000 - i) * 90, `U${i % 3 + 1}`,
  `Synthetic message ${i + 1}. ` + "Variable row height. ".repeat(i % 7),
  i % 200 === 0 ? { files: [file({ id: `FBIG${i}`, name: "image.png", mimetype: "image/png", local_path: "/synthetic/image.png", width: 640, height: 100 + i % 600 })] } : {},
));
conversations.push({ ...conversations[0], id: "CBIG", label: "timeline-10000", messages: bigMessages.length, unread: 0, latest_ts: bigMessages.at(-1)!.ts });
messages.sort((a, b) => a.ts.localeCompare(b.ts));
